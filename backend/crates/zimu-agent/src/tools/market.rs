//! 行情侧工具：market_status（时钟/时段）、get_quote（实时行 + 涨跌停）、
//! refresh_quotes（回写重算，与 `POST /api/market/refresh` 同一条链）。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use robit_agent::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use zimu_domain::price_limit::{limit_pct, limit_prices};
use zimu_domain::Kind;
use zimu_market::clock::{market_clock, now_shanghai, CalendarView};
use zimu_market::error::MarketError;
use zimu_market::symbols::symbol_of;

use super::{market_err, ok_json, store_err, store_try};
use crate::service::{self, RefreshError};
use crate::AgentDeps;

/// 探测超时：与 server status 端点同款（回环调用毫秒级；sidecar 挂起不拖死会话轮）
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

// ============================================================================
// market_status
// ============================================================================

pub struct MarketStatusTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for MarketStatusTool {
    fn name(&self) -> &str {
        "market_status"
    }

    fn description(&self) -> &str {
        "查市场时钟与行情服务状态：当前是否交易日、所处时段（盘前/集合竞价/盘中/午休/已收盘/休市，03 §9 生产口径：恒 Asia/Shanghai、交易日历优先）、下一事件与下一交易日、行情 sidecar 是否在线、日历是否降级。复盘/盘前类回答先用本工具确定时间背景；日历降级（calendar_degraded=true）时要在文案里如实说明「未排除节假日」。"
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let health = tokio::time::timeout(PROBE_TIMEOUT, self.deps.market.health())
            .await
            .unwrap_or_else(|_| {
                Err(MarketError::Unavailable(format!(
                    "健康探测超时（>{}s）",
                    PROBE_TIMEOUT.as_secs()
                )))
            });
        let (online, version, health_error) = match &health {
            Ok(h) => (h.ok, h.version.clone(), None),
            Err(e) => (false, None, Some(e.to_string())),
        };
        // 时钟输入侧：实拉 → 旧缓存 → 工作日启发式（zimu-market CalendarCache 内置降级链），
        // 探测超时与失败同语义（与 server status 处理器一致）
        let view = tokio::time::timeout(PROBE_TIMEOUT, self.deps.calendar.view_for_clock(&*self.deps.market))
            .await
            .unwrap_or_else(|_| match self.deps.calendar.stale() {
                Some((p, _)) => {
                    let mut v = CalendarView::from_payload(&p);
                    v.upstream_degraded = true;
                    v
                }
                None => CalendarView::unavailable(),
            });
        let clock = market_clock(now_shanghai(), &view);
        Ok(ok_json(&json!({
            "sidecar_url": self.deps.market.base_url(),
            "sidecar_online": online,
            "sidecar_version": version,
            "sidecar_error": health_error,
            "clock": clock,
            "calendar_available": view.trade_dates.is_some(),
            "calendar_source": view.source,
            "calendar_degraded": view.upstream_degraded,
            "note": "clock 只给事实（state/next_event/next_at/next_trade_day），倒计时文案由你按当前时间组织；数字不得改写",
        })))
    }
}

// ============================================================================
// get_quote
// ============================================================================

pub struct GetQuoteTool {
    pub deps: Arc<AgentDeps>,
}

/// 前缀形态判定（B7：显式 sh/sz/bj 前缀 + 数字；不做号段猜测）
pub(crate) fn split_prefixed(token: &str) -> Option<(&str, &str)> {
    let t = token.to_ascii_lowercase();
    for p in ["sh", "sz", "bj"] {
        if let Some(rest) = t.strip_prefix(p) {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                // 返回原大小写的 code 部分（代码全数字，无损）
                return Some((p, &token[token.len() - rest.len()..]));
            }
        }
    }
    None
}

#[async_trait]
impl Tool for GetQuoteTool {
    fn name(&self) -> &str {
        "get_quote"
    }

    fn description(&self) -> &str {
        "查实时行情：现价、昨收、今开、最高、最低、成交量/额、涨跌幅、行情时点 as_of、行级状态（ok/halted_or_delisted/empty_or_delisted/no_data）、来源与降级明细 attempts，并按 03 §2 口径附当日涨停价 limit_up / 跌停价 limit_down（zimu-domain price_limit 计算，含边界）。参数 codes 为逗号分隔列表：持仓内可用裸代码（如 600519），非持仓必须带市场前缀（如 sh600519，B7 不做号段猜测）。回答行情必须引用本工具返回的数字并注明 as_of 时点。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "codes": {
                    "type": "string",
                    "description": "逗号分隔标的列表：裸代码（持仓内）或 sh/sz/bj 前缀完整形态，如 \"600519,sh510300\""
                }
            },
            "required": ["codes"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let raw = args.get("codes").and_then(Value::as_str).unwrap_or("");
        let tokens: Vec<&str> = raw.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        if tokens.is_empty() {
            return Ok(ToolResult::error("codes 不能为空"));
        }
        let holdings = store_try!(crate::lock(&self.deps.store).holdings());

        // symbol 解析：前缀形态直通；裸代码查持仓 mkt；都拼不出 → 逐条如实报错
        let mut symbols: Vec<String> = Vec::new();
        let mut resolve_errors: Vec<String> = Vec::new();
        for t in &tokens {
            if let Some((p, code)) = split_prefixed(t) {
                symbols.push(format!("{p}{code}"));
            } else if let Some(s) = holdings
                .iter()
                .find(|h| h.code == *t)
                .and_then(|h| symbol_of(&h.mkt, &h.code))
            {
                symbols.push(s);
            } else {
                resolve_errors.push(format!(
                    "{t}：不在持仓且未带市场前缀（B7 不做号段猜测），请改用 sh/sz 前缀完整形态"
                ));
            }
        }
        if symbols.is_empty() {
            return Ok(ToolResult::error(format!(
                "没有可查询的标的：{}",
                resolve_errors.join("；")
            )));
        }

        let payload = match self.deps.market.quote(&symbols).await {
            Ok(p) => p,
            Err(e) => return Ok(market_err(e)),
        };

        // 涨跌停拼装（03 §2）：pct 取数据层配置（holding.limit_pct）优先，未配置按
        // 板块推断（ETF 推断不出 → null）；prev 取实时行、缺省回库存
        let quotes: Vec<Value> = payload
            .quotes
            .iter()
            .map(|row| {
                let holding = holdings.iter().find(|h| h.code == row.code);
                let kind = holding
                    .map(|h| h.kind)
                    .or_else(|| serde_json::from_value::<Kind>(json!(row.kind)).ok());
                let pct = holding
                    .and_then(|h| h.limit_pct)
                    .or_else(|| kind.and_then(|k| limit_pct(k, &row.code, false)));
                let prev = row.prev.or_else(|| holding.and_then(|h| h.prev));
                let (mut up, mut down) = (None, None);
                if let (Some(k), Some(p), Some(pc)) = (kind, prev, pct) {
                    if p > 0.0 {
                        let r = limit_prices(k, p, pc);
                        up = r.limit_up;
                        down = r.limit_down;
                    }
                }
                let mut v = serde_json::to_value(row).unwrap_or_else(|_| json!({}));
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("limit_up".into(), json!(up));
                    obj.insert("limit_down".into(), json!(down));
                    obj.insert("limit_pct".into(), json!(pct));
                }
                v
            })
            .collect();

        Ok(ok_json(&json!({
            "as_of": payload.as_of,
            "degraded": payload.degraded,
            "attempts": payload.attempts,
            "quotes": quotes,
            "resolve_errors": resolve_errors,
        })))
    }
}

// ============================================================================
// refresh_quotes
// ============================================================================

pub struct RefreshQuotesTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for RefreshQuotesTool {
    fn name(&self) -> &str {
        "refresh_quotes"
    }

    fn description(&self) -> &str {
        "拉取全部持仓的实时行情并回写数据库，然后按 03 §5 口径全量重算组合（与界面顶栏刷新、POST /api/market/refresh 同一条链）。返回 as_of、updated 回写行数、skipped 未回写明细（停牌/退市等，保持库存旧值）、degraded/attempts 降级明细与重算后的组合全景 portfolio。顺带补全空串行业（P7-T6/ADR-0016：股票查新浪行业板映射、ETF=基金/转债=转债，遍历全部持仓含停牌退市，手工非空值不覆盖；映射失败不阻断行情回写）。生成盘后复盘、建议或任何组合级数字**之前**先调用本工具，确保数字是最新的；sidecar 不可达时如实报错，不写库、不动旧数字。"
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        match service::refresh_portfolio(&self.deps.store, &*self.deps.market, &self.deps.sector_map).await {
            Ok(r) => Ok(ok_json(&r)),
            Err(RefreshError::Store(m)) => {
                tracing::error!("刷新链存储错误：{m}");
                Ok(store_err(m))
            }
            Err(RefreshError::Market(e)) => Ok(market_err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_prefixed;

    #[test]
    fn prefixed_forms() {
        assert_eq!(split_prefixed("sh600519"), Some(("sh", "600519")));
        assert_eq!(split_prefixed("SZ128136"), Some(("sz", "128136")));
        assert_eq!(split_prefixed("bj833171"), Some(("bj", "833171")));
        assert_eq!(split_prefixed("600519"), None);
        assert_eq!(split_prefixed("sh"), None);
        assert_eq!(split_prefixed("sh60051a"), None);
        assert_eq!(split_prefixed("xq600519"), None);
    }
}
