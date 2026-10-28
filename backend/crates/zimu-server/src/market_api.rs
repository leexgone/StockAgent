//! 行情 API（P4-T4）：`/api/market/*` —— `zimu-market` 适配器的 HTTP 接线。
//!
//! 端点：
//! - `GET  /api/market/status`              市场时钟 + sidecar 健康 + 日历可用性（03 §9 生产口径）
//! - `GET  /api/market/quote?codes=`        实时行情透传（类型化契约）
//! - `GET  /api/market/kline/{symbol}`      日线透传；`with_today=1` 合成当日 bar（04 附录 B）
//! - `GET  /api/market/cb_list`             转债列表（现券 + 强赎）透传
//! - `GET  /api/market/calendar`            交易日历透传（默认窗口走适配层缓存）
//! - `POST /api/market/refresh`             行情回写持仓 → `zimu-domain::recalc` 全量重算
//!
//! 降级边界（04 附录 B「适配器边界」）：
//! - sidecar 不可达 → 行情端点 503 `market_unavailable`，`/api/holdings*` 照常
//!   （库存陈旧行情字段），时钟退工作日启发式并如实标注 `calendar_degraded`。
//! - sidecar 上游错误（如 502 `UPSTREAM_ALL_FAILED`、400 `BAD_SYMBOL`）原样透传
//!   状态码与错误契约——本层不重定义错误语义。
//! - 本模块只做接线与过滤，**不定义任何数字口径**：limit_pct 推断走
//!   `zimu-domain::price_limit`，组合派生走 `zimu-domain::recalc`（经 store.portfolio）。

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use zimu_market::calendar::CalendarCache;
use zimu_market::clock::{market_clock, now_shanghai};
use zimu_market::error::MarketError;
use zimu_market::models::QuoteStatus;
use zimu_market::symbols::symbol_of;
use zimu_market::{synth, MarketApi};
use zimu_store::{QuoteUpdate, Store};

use crate::holdings_api::SharedStore;

pub struct MarketState {
    /// sidecar 唯一消费入口（zimu-market，04 附录 B）
    pub api: Arc<dyn MarketApi>,
    /// 日历缓存（1h；status 每秒被轮也不打 sidecar）
    pub calendar: CalendarCache,
}

pub type SharedMarket = Arc<MarketState>;

pub fn new_market(api: Arc<dyn MarketApi>) -> SharedMarket {
    Arc::new(MarketState {
        api,
        calendar: CalendarCache::default(),
    })
}

fn lock(store: &SharedStore) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// 路由装配：market 单状态路由 + refresh 双状态路由，merge 后擦除状态。
pub fn router(store: SharedStore, market: SharedMarket) -> Router {
    let market_only = Router::new()
        .route("/api/market/status", get(status))
        .route("/api/market/quote", get(quote))
        .route("/api/market/kline/{symbol}", get(kline))
        .route("/api/market/cb_list", get(cb_list))
        .route("/api/market/calendar", get(calendar))
        .with_state(market.clone());
    let both = Router::new()
        .route("/api/market/refresh", post(refresh))
        .with_state((store, market));
    market_only.merge(both)
}

/// MarketError → HTTP（三分类各自的语义，见 zimu-market::error）
fn err_response(e: MarketError) -> Response {
    match e {
        MarketError::Unavailable(m) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "market_unavailable",
                "message": format!("行情 sidecar 不可达，行情服务降级中：{m}"),
            })),
        )
            .into_response(),
        MarketError::Upstream {
            status,
            code,
            message,
        } => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(json!({ "error": code, "message": message })),
        )
            .into_response(),
        MarketError::Contract(m) => {
            tracing::error!("行情契约不符（sidecar 与本程序版本漂移，要修代码）：{m}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "market_contract", "message": m })),
            )
                .into_response()
        }
    }
}

fn store_error(e: impl std::fmt::Display) -> Response {
    tracing::error!("存储层错误：{e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "store_error", "message": "存储层内部错误，已记录日志" })),
    )
        .into_response()
}

// ============================================================================
// GET /api/market/status
// ============================================================================

/// status 探测超时：健康/日历都是本机回环调用，正常毫秒级；sidecar「挂起但
/// 不拒绝连接」时不能被 10s 整请求超时拖死——status 是前端秒级轮询端点，
/// 探测必须有界（探测失败按不可达降级，与拒连同语义）。
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// 市场时钟 + sidecar 健康 + 日历可用性。永不失败：sidecar 全挂也返回 200，
/// 时钟退工作日启发式（`calendar_degraded: true`，03 §9 降级条款）。
async fn status(State(m): State<SharedMarket>) -> Json<Value> {
    let health = tokio::time::timeout(PROBE_TIMEOUT, m.api.health())
        .await
        .unwrap_or_else(|_| {
            Err(MarketError::Unavailable(format!(
                "健康探测超时（>{}s）",
                PROBE_TIMEOUT.as_secs()
            )))
        });
    let (online, version) = match &health {
        Ok(h) => (h.ok, h.version.clone()),
        Err(_) => (false, None),
    };
    if let Err(e) = &health {
        tracing::debug!("sidecar 健康探测失败（降级运行）：{e}");
    }
    let view = tokio::time::timeout(PROBE_TIMEOUT, m.calendar.view_for_clock(&*m.api))
        .await
        .unwrap_or_else(|_| {
            // 超时与失败同语义：旧日历缓存仍是降级缓冲，全无才退工作日启发式
            tracing::warn!("交易日历探测超时（>{}s）", PROBE_TIMEOUT.as_secs());
            match m.calendar.stale() {
                Some((p, _)) => {
                    let mut v = zimu_market::clock::CalendarView::from_payload(&p);
                    v.upstream_degraded = true;
                    v
                }
                None => zimu_market::clock::CalendarView::unavailable(),
            }
        });
    let clock = market_clock(now_shanghai(), &view);
    Json(json!({
        "sidecar": {
            "url": m.api.base_url(),
            "online": online,
            "version": version,
            // 探测失败原因如实透出（运维排查）；online=true 时为 null
            "error": health.err().map(|e| e.to_string()),
        },
        "clock": clock,
        "calendar": {
            "available": view.trade_dates.is_some(),
            "source": view.source,
            "upstream_degraded": view.upstream_degraded,
            "count": view.trade_dates.as_ref().map(|d| d.len()),
        },
    }))
}

// ============================================================================
// GET /api/market/quote · /kline/{symbol} · /cb_list · /calendar —— 契约透传
// ============================================================================

#[derive(Deserialize)]
struct QuoteQuery {
    codes: String,
}

async fn quote(State(m): State<SharedMarket>, Query(q): Query<QuoteQuery>) -> Response {
    let symbols: Vec<String> = q
        .codes
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if symbols.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "bad_params", "message": "codes 不能为空" })),
        )
            .into_response();
    }
    match m.api.quote(&symbols).await {
        Ok(p) => Json(p).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Deserialize)]
struct KlineQuery {
    kind: Option<String>,
    start: Option<String>,
    end: Option<String>,
    /// 1/true = 用实时行情合成当日 bar（04 附录 B；盘中该 bar 随行就市，非定稿）
    with_today: Option<bool>,
}

async fn kline(
    State(m): State<SharedMarket>,
    Path(symbol): Path<String>,
    Query(q): Query<KlineQuery>,
) -> Response {
    let mut payload = match m
        .api
        .kline(
            &symbol,
            q.kind.as_deref(),
            q.start.as_deref(),
            q.end.as_deref(),
        )
        .await
    {
        Ok(p) => p,
        Err(e) => return err_response(e),
    };

    let mut synthesized = false;
    if q.with_today.unwrap_or(false) {
        // 实时拉取失败不影响日线供给（诚实降级：today_synthesized=false）
        if let Ok(qp) = m.api.quote(std::slice::from_ref(&symbol)).await {
            if let Some(row) = qp.quotes.iter().find(|r| r.symbol == symbol) {
                let today = now_shanghai().date_naive().format("%Y-%m-%d").to_string();
                synthesized = synth::merge_today(&mut payload, row, &today);
            }
        }
    }

    match serde_json::to_value(&payload) {
        Ok(mut body) => {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("today_synthesized".into(), json!(synthesized));
            }
            Json(body).into_response()
        }
        Err(e) => err_response(MarketError::Contract(format!("kline 序列化失败：{e}"))),
    }
}

async fn cb_list(State(m): State<SharedMarket>) -> Response {
    match m.api.cb_list().await {
        Ok(p) => Json(p).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Deserialize)]
struct CalendarQuery {
    start: Option<String>,
    end: Option<String>,
}

async fn calendar(State(m): State<SharedMarket>, Query(q): Query<CalendarQuery>) -> Response {
    // 默认窗口（今天 ±180）走适配层缓存；显式区间直连 sidecar
    match m
        .calendar
        .get(&*m.api, q.start.as_deref(), q.end.as_deref())
        .await
    {
        Ok(p) => Json(p).into_response(),
        Err(e) => err_response(e),
    }
}

// ============================================================================
// POST /api/market/refresh —— 行情回写 + 服务端 recalc() 链
// ============================================================================

/// 拉全部持仓实时行情 → 回写 `holdings.price/prev/name/limit_pct` →
/// `store.portfolio()` 全量重算（派生与汇总只出自 `zimu-domain::recalc`，03 §5）。
/// sidecar 不可达 → 503（不写库、不动旧数字）；个别标的无行情 → skipped 留原因，
/// 该行保持库存陈旧值（降级不断供）。
async fn refresh(State((store, m)): State<(SharedStore, SharedMarket)>) -> Response {
    let holdings = match lock(&store).holdings() {
        Ok(h) => h,
        Err(e) => return store_error(e),
    };

    // symbol 拼装（B7：显式 mkt 字段，拼不出前缀即拒绝，不发出去挨 400）
    let mut symbols: Vec<String> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut pairs: Vec<(String, usize)> = Vec::new();
    for (i, h) in holdings.iter().enumerate() {
        match symbol_of(&h.mkt, &h.code) {
            Some(s) => {
                if !symbols.contains(&s) {
                    symbols.push(s.clone());
                }
                pairs.push((s, i));
            }
            None => skipped.push(json!({
                "code": h.code,
                "reason": format!("mkt={} 拼不出市场前缀（B7：仅 SH/SZ）", h.mkt),
            })),
        }
    }

    // 空持仓/全拼不出：不打 sidecar，直接回重算视图
    if symbols.is_empty() {
        let view = match lock(&store).portfolio() {
            Ok(v) => v,
            Err(e) => return store_error(e),
        };
        return Json(json!({
            "as_of": null, "updated": 0, "skipped": skipped,
            "degraded": false, "attempts": [], "portfolio": view,
        }))
        .into_response();
    }

    let payload = match m.api.quote(&symbols).await {
        Ok(p) => p,
        Err(e) => return err_response(e),
    };

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
                skipped.push(json!({
                    "code": h.code,
                    "reason": format!("行情行状态 {status}，保持库存旧值"),
                }));
            }
            None => skipped.push(json!({
                "code": h.code,
                "reason": "行情响应缺该标的行",
            })),
        }
    }

    let updated = match lock(&store).apply_quotes(&updates) {
        Ok(n) => n,
        Err(e) => return store_error(e),
    };
    let view = match lock(&store).portfolio() {
        Ok(v) => v,
        Err(e) => return store_error(e),
    };

    Json(json!({
        "as_of": payload.as_of,
        "updated": updated,
        "skipped": skipped,
        "degraded": payload.degraded,
        "attempts": payload.attempts,
        "portfolio": view,
    }))
    .into_response()
}
