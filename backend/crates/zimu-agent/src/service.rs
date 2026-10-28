//! 应用服务层：agent 工具与 HTTP API **共用**的编排逻辑。
//!
//! `refresh_portfolio` 原为 zimu-server `market_api::refresh` 处理器的内联主体，
//! P4-T6 抽到这里——`POST /api/market/refresh` 与 agent 的 `refresh_quotes` 工具
//! 走同一条链，口径不分叉：symbol 拼装（B7 显式 mkt）→ 行情过滤（04 附录 B：
//! 仅 status=ok 且 price>0 回写；limit_pct 仅数据层未配置时推断，走
//! `zimu-domain::price_limit`）→ `apply_quotes` → `store.portfolio()`
//! （`zimu-domain::recalc` 全量重算）。
//!
//! 响应类型（`SkippedRow` / `RefreshResponse`）随迁移进本 crate，wire 形状与
//! OpenAPI 组件名不变（server 侧 `pub use` 再导出，契约文档逐字节一致）。

use std::collections::HashMap;

use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use zimu_market::clock::now_shanghai;
use zimu_market::error::MarketError;
use zimu_market::models::{Attempt, QuoteStatus};
use zimu_market::symbols::symbol_of;
use zimu_market::MarketApi;
use zimu_store::{PortfolioView, QuoteUpdate};

use crate::{lock, SharedStore};

/// 未回写标的的明细行（降级不断供：对应持仓保持库存旧值）
#[derive(Debug, Serialize, ToSchema)]
pub struct SkippedRow {
    pub code: String,
    /// 原因：拼不出市场前缀（B7）/ 行情行状态异常（停牌、退市、no_data）/ 响应缺行
    pub reason: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RefreshResponse {
    /// 行情时点（sidecar as_of 透传）；空持仓未打 sidecar 时为 null
    pub as_of: Option<String>,
    /// 实际回写的持仓行数
    pub updated: usize,
    /// 未回写标的明细（保持库存旧值）
    pub skipped: Vec<SkippedRow>,
    /// sidecar 兜底链降级标注（透传）
    pub degraded: bool,
    /// 逐源尝试明细（透传，降级可观测）
    pub attempts: Vec<Attempt>,
    /// 回写后 `recalc()` 全量重算的组合全景（03 §5；数字与 /api/holdings 同源）
    pub portfolio: PortfolioView,
}

/// 刷新链失败二分类：存储（调用方给 500/store_error）与行情（三分类原样透传）
pub enum RefreshError {
    Store(String),
    Market(MarketError),
}

/// 拉全部持仓实时行情 → 回写 `holdings.price/prev/name/limit_pct` →
/// `store.portfolio()` 全量重算（派生与汇总只出自 `zimu-domain::recalc`，03 §5）。
/// sidecar 不可达 → `RefreshError::Market`（不写库、不动旧数字）；个别标的无行情
/// → skipped 留原因，该行保持库存陈旧值（降级不断供）。
pub async fn refresh_portfolio(
    store: &SharedStore,
    api: &dyn MarketApi,
) -> Result<RefreshResponse, RefreshError> {
    let holdings = lock(store).holdings().map_err(store_err)?;

    // symbol 拼装（B7：显式 mkt 字段，拼不出前缀即拒绝，不发出去挨 400）
    let mut symbols: Vec<String> = Vec::new();
    let mut skipped: Vec<SkippedRow> = Vec::new();
    let mut pairs: Vec<(String, usize)> = Vec::new();
    for (i, h) in holdings.iter().enumerate() {
        match symbol_of(&h.mkt, &h.code) {
            Some(s) => {
                if !symbols.contains(&s) {
                    symbols.push(s.clone());
                }
                pairs.push((s, i));
            }
            None => skipped.push(SkippedRow {
                code: h.code.clone(),
                reason: format!("mkt={} 拼不出市场前缀（B7：仅 SH/SZ）", h.mkt),
            }),
        }
    }

    // 空持仓/全拼不出：不打 sidecar，直接回重算视图
    if symbols.is_empty() {
        let view = lock(store).portfolio().map_err(store_err)?;
        return Ok(RefreshResponse {
            as_of: None,
            updated: 0,
            skipped,
            degraded: false,
            attempts: Vec::new(),
            portfolio: view,
        });
    }

    let payload = api
        .quote(&symbols)
        .await
        .map_err(RefreshError::Market)?;

    // 回写过滤（04 附录 B）：仅 status==ok 且 price>0 的行更新价格；
    // limit_pct 仅在数据层未配置时推断（st=false——ST 识别依赖行情字段，03 §2）
    let by_sym: HashMap<&str, &zimu_market::models::QuoteRow> =
        payload.quotes.iter().map(|r| (r.symbol.as_str(), r)).collect();
    let ts = now_shanghai().format("%Y-%m-%d %H:%M").to_string();
    let mut updates: Vec<QuoteUpdate> = Vec::new();
    for (sym, i) in &pairs {
        let h = &holdings[*i];
        match by_sym.get(sym.as_str()) {
            Some(q) if q.status == QuoteStatus::Ok && q.price.is_some_and(|p| p > 0.0) => {
                let limit_pct = if h.limit_pct.is_some() {
                    None // 数据层已配置：COALESCE 语义下传 None 保持原值
                } else {
                    zimu_domain::price_limit::limit_pct(h.kind, &h.code, false)
                };
                updates.push(QuoteUpdate {
                    code: h.code.clone(),
                    price: q.price,
                    prev: q.prev.filter(|p| *p > 0.0),
                    name: q.name.clone(),
                    limit_pct,
                    ts: ts.clone(),
                });
            }
            Some(q) => {
                let status = serde_json::to_value(q.status)
                    .unwrap_or_else(|_| json!(format!("{:?}", q.status)));
                skipped.push(SkippedRow {
                    code: h.code.clone(),
                    reason: format!("行情行状态 {status}，保持库存旧值"),
                });
            }
            None => skipped.push(SkippedRow {
                code: h.code.clone(),
                reason: "行情响应缺该标的行".into(),
            }),
        }
    }

    let updated = lock(store).apply_quotes(&updates).map_err(store_err)?;
    let view = lock(store).portfolio().map_err(store_err)?;

    Ok(RefreshResponse {
        as_of: Some(payload.as_of),
        updated,
        skipped,
        degraded: payload.degraded,
        attempts: payload.attempts,
        portfolio: view,
    })
}

fn store_err(e: impl std::fmt::Display) -> RefreshError {
    RefreshError::Store(e.to_string())
}
