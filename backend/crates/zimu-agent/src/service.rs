//! 应用服务层：agent 工具与 HTTP API **共用**的编排逻辑。
//!
//! `refresh_portfolio` 原为 zimu-server `market_api::refresh` 处理器的内联主体，
//! P4-T6 抽到这里——`POST /api/market/refresh` 与 agent 的 `refresh_quotes` 工具
//! 走同一条链，口径不分叉：symbol 拼装（B7 显式 mkt）→ 行情过滤（04 附录 B：
//! 仅 status=ok 且 price>0 回写；limit_pct 仅数据层未配置时推断，走
//! `zimu-domain::price_limit`）→ `apply_quotes` → **行业补全 pass（P7-T6）** →
//! `store.portfolio()`（`zimu-domain::recalc` 全量重算）。
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
use zimu_market::sector::SectorMapCache;
use zimu_market::symbols::symbol_of;
use zimu_market::MarketApi;
use zimu_store::{HoldingRow, PortfolioView, QuoteUpdate};

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
    sectors: &SectorMapCache,
) -> Result<RefreshResponse, RefreshError> {
    refresh_portfolio_detailed(store, api, sectors)
        .await
        .map(|(r, _)| r)
}

/// 与 [`refresh_portfolio`] 同一条链，额外返回本轮 quote 原始行——调度器
/// （P4-T7）复用回写链的同时还要 volume/prev 等判定事实（03 §12），这些
/// 不进 wire 契约，所以不扩 `RefreshResponse` 而是走第二返回值。
pub async fn refresh_portfolio_detailed(
    store: &SharedStore,
    api: &dyn MarketApi,
    sectors: &SectorMapCache,
) -> Result<(RefreshResponse, Vec<zimu_market::models::QuoteRow>), RefreshError> {
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
        return Ok((
            RefreshResponse {
                as_of: None,
                updated: 0,
                skipped,
                degraded: false,
                attempts: Vec::new(),
                portfolio: view,
            },
            Vec::new(),
        ));
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
    // 行业补全（P7-T6/ADR-0016）：顺带挂刷新链，遍历全量持仓（含停牌退市），仅补
    // 空串 sector。永不 Err——映射/写库失败都不阻断行情回写（recalc 在补全后跑，
    // 响应 portfolio 自带新 sector）。
    fill_missing_sectors(store, api, sectors, &holdings).await;
    let view = lock(store).portfolio().map_err(store_err)?;

    let resp = RefreshResponse {
        as_of: Some(payload.as_of),
        updated,
        skipped,
        degraded: payload.degraded,
        attempts: payload.attempts,
        portfolio: view,
    };
    Ok((resp, payload.quotes))
}

fn store_err(e: impl std::fmt::Display) -> RefreshError {
    RefreshError::Store(e.to_string())
}

/// ETF/转债单列类别（ADR-0016/03 §6）：按 kind 判定，不查行业映射。
const SECTOR_ETF: &str = "基金";
const SECTOR_CB: &str = "转债";

/// 行业补全 pass（P7-T6/ADR-0016）：遍历**全部持仓**（含停牌/退市，不被 quote
/// 状态门控——sector 是标的属性），仅补空串 sector。ETF/转债按 kind 归类；股票查
/// `SectorMapCache`（失败退 stale，仍无则跳过股票补全）。手工非空值永不覆盖
/// （store `fill_sectors` 的 `WHERE sector=''` 承接）。**永不 Err**：内部吞错记日志，
/// 行情回写不受影响；不写 holding_edits（数据层富化非用户校正，行情回写同款）。
async fn fill_missing_sectors(
    store: &SharedStore,
    api: &dyn MarketApi,
    sectors: &SectorMapCache,
    holdings: &[HoldingRow],
) {
    use zimu_domain::Kind;
    let mut fills: Vec<(String, String)> = Vec::new();
    let mut need_map = false;
    for h in holdings {
        if !h.sector.is_empty() {
            continue; // 只补空串：手工/已补值永不覆盖
        }
        match h.kind {
            Kind::Etf => fills.push((h.code.clone(), SECTOR_ETF.into())),
            Kind::Cb => fills.push((h.code.clone(), SECTOR_CB.into())),
            Kind::Stock => need_map = true,
        }
    }
    if need_map {
        let map = match sectors.get(api).await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!("行业映射获取失败，本轮跳过股票 sector 补全：{e}");
                sectors.stale().map(|(p, _)| p)
            }
        };
        if let Some(p) = map {
            for h in holdings
                .iter()
                .filter(|h| matches!(h.kind, Kind::Stock) && h.sector.is_empty())
            {
                if let Some(sec) = p.sectors.get(&h.code) {
                    fills.push((h.code.clone(), sec.clone()));
                } // 未映射（新股/退市）：留空串，不伪造
            }
        }
    }
    if fills.is_empty() {
        return;
    }
    let ts = now_shanghai().format("%Y-%m-%d %H:%M").to_string();
    match lock(store).fill_sectors(&fills, &ts) {
        Ok(n) if n > 0 => tracing::info!("sector 自动补全 {n} 行（P7-T6）"),
        Err(e) => tracing::warn!("sector 补全写库失败（行情回写不受影响）：{e}"),
        _ => {}
    }
}
