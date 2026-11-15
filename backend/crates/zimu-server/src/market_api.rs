//! 行情 API（P4-T4）：`/api/market/*` —— `zimu-market` 适配器的 HTTP 接线。
//!
//! 端点：
//! - `GET  /api/market/status`              市场时钟 + sidecar 健康 + 日历可用性（03 §9 生产口径）
//! - `GET  /api/market/quote?codes=`        实时行情透传（类型化契约）
//! - `GET  /api/market/kline/{symbol}`      日线透传；`with_today=true` 合成当日 bar（04 附录 B）
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
//!
//! 契约（P4-T5）：响应全部是类型化结构体（本文件 + zimu-market 模型），
//! `#[utoipa::path]` 标注即 OpenAPI 文档来源；错误响应统一 `ApiError`。

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use zimu_market::calendar::CalendarCache;
use zimu_market::clock::{market_clock, now_shanghai, MarketClock};
use zimu_market::sector::SectorMapCache;
use zimu_market::error::MarketError;
use zimu_market::models::{CalendarPayload, CbListPayload, KlinePayload, QuotePayload};
use zimu_market::{synth, MarketApi};

use crate::holdings_api::SharedStore;
use crate::openapi::ApiError;

// P4-T6：刷新链主体抽到 zimu-agent::service（agent 工具 refresh_quotes 与
// POST /api/market/refresh 走同一条链），响应类型随之迁移并在此再导出——
// OpenAPI 组件名与 wire 形状不变（契约测试逐字节守护）。
pub use zimu_agent::service::{RefreshResponse, SkippedRow};

pub struct MarketState {
    /// sidecar 唯一消费入口（zimu-market，04 附录 B）
    pub api: Arc<dyn MarketApi>,
    /// 日历缓存（1h；status 每秒被轮也不打 sidecar）
    pub calendar: CalendarCache,
    /// 行业映射缓存（24h，P7-T6）：refresh 顺带补空串 sector 的映射输入
    pub sector_map: SectorMapCache,
}

pub type SharedMarket = Arc<MarketState>;

pub fn new_market(api: Arc<dyn MarketApi>) -> SharedMarket {
    Arc::new(MarketState {
        api,
        calendar: CalendarCache::default(),
        sector_map: SectorMapCache::default(),
    })
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
            Json(ApiError::new(
                "market_unavailable",
                format!("行情 sidecar 不可达，行情服务降级中：{m}"),
            )),
        )
            .into_response(),
        MarketError::Upstream {
            status,
            code,
            message,
        } => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(ApiError::new(code, message)),
        )
            .into_response(),
        MarketError::Contract(m) => {
            tracing::error!("行情契约不符（sidecar 与本程序版本漂移，要修代码）：{m}");
            (
                StatusCode::BAD_GATEWAY,
                Json(ApiError::new("market_contract", m)),
            )
                .into_response()
        }
    }
}

// ============================================================================
// GET /api/market/status
// ============================================================================

/// status 探测超时：健康/日历都是本机回环调用，正常毫秒级；sidecar「挂起但
/// 不拒绝连接」时不能被 10s 整请求超时拖死——status 是前端秒级轮询端点，
/// 探测必须有界（探测失败按不可达降级，与拒连同语义）。
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// sidecar 在线状态（status 端点的探测结果；`SidecarHealth` 的原始运维字段不透出）
#[derive(Debug, Serialize, ToSchema)]
pub struct SidecarStatus {
    /// sidecar 基地址（ZIMU_MARKET_URL）
    pub url: String,
    pub online: bool,
    /// sidecar 版本（在线且上报时）
    pub version: Option<String>,
    /// 探测失败原因如实透出（运维排查）；online=true 时为 null
    pub error: Option<String>,
}

/// 交易日历可用性（时钟的判据来源）
#[derive(Debug, Serialize, ToSchema)]
pub struct CalendarStatus {
    /// false = 日历不可用，时钟退工作日启发式（配 clock.calendar_degraded 判读）
    pub available: bool,
    /// 日历源名（sina_calendar / baostock_calendar）；不可用时 null
    pub source: Option<String>,
    /// sidecar 日历自身降级（备源顶上）——如实透传
    pub upstream_degraded: bool,
    /// 窗口内交易日数量；不可用时 null
    pub count: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarketStatusResponse {
    pub sidecar: SidecarStatus,
    /// 市场时钟（03 §9 生产口径；API 只给事实，倒计时文案由前端走针）
    pub clock: MarketClock,
    pub calendar: CalendarStatus,
}

/// 市场时钟 + sidecar 健康 + 日历可用性。永不失败：sidecar 全挂也返回 200，
/// 时钟退工作日启发式（`calendar_degraded: true`，03 §9 降级条款）。
#[utoipa::path(
    get,
    path = "/api/market/status",
    operation_id = "market_status",
    tag = "market",
    responses(
        (status = 200, description = "市场时钟 + sidecar 在线状态 + 日历可用性。永不失败：sidecar 全挂也 200，时钟退工作日启发式并标注 calendar_degraded（前端秒级轮询端点，探测有界 2s）", body = MarketStatusResponse),
    )
)]
pub(crate) async fn status(State(m): State<SharedMarket>) -> Json<MarketStatusResponse> {
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
    Json(MarketStatusResponse {
        sidecar: SidecarStatus {
            url: m.api.base_url().to_string(),
            online,
            version,
            error: health.err().map(|e| e.to_string()),
        },
        clock,
        calendar: CalendarStatus {
            available: view.trade_dates.is_some(),
            source: view.source,
            upstream_degraded: view.upstream_degraded,
            count: view.trade_dates.as_ref().map(|d| d.len()),
        },
    })
}

// ============================================================================
// GET /api/market/quote · /kline/{symbol} · /cb_list · /calendar —— 契约透传
// ============================================================================

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QuoteQuery {
    /// 逗号分隔的标的列表（带 sh/sz 前缀，如 sh600519,sz128136）。业务必填：
    /// 缺失或全空 → 400 bad_params（本地拒绝，不打 sidecar）
    codes: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/market/quote",
    operation_id = "market_quote",
    tag = "market",
    params(QuoteQuery),
    responses(
        (status = 200, description = "实时行情透传（类型化契约）：degraded/attempts 降级可观测，行级 status 四态（ok/halted_or_delisted/empty_or_delisted/no_data）", body = QuotePayload),
        (status = 400, description = "codes 缺失或为空（bad_params，本地拒绝）", body = ApiError),
        (status = 502, description = "sidecar 上游错误契约透传，或响应反序列化失败（market_contract，版本漂移要修代码）", body = ApiError),
        (status = 503, description = "sidecar 不可达（market_unavailable，降级信号）", body = ApiError),
    )
)]
pub(crate) async fn quote(State(m): State<SharedMarket>, Query(q): Query<QuoteQuery>) -> Response {
    let symbols: Vec<String> = q
        .codes
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if symbols.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("bad_params", "codes 不能为空")),
        )
            .into_response();
    }
    match m.api.quote(&symbols).await {
        Ok(p) => Json(p).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct KlineQuery {
    /// bar 类型（当前仅日线；缺省由 sidecar 内部路由并标注 kind_routed）
    kind: Option<String>,
    /// 起始日期 YYYY-MM-DD（缺省用 sidecar 默认窗口）
    start: Option<String>,
    /// 结束日期 YYYY-MM-DD
    end: Option<String>,
    /// true = 用实时行情合成当日 bar（04 附录 B；盘中该 bar 随行就市，非定稿）
    with_today: Option<bool>,
}

/// kline 响应：sidecar 日线负载 + 服务端当日 bar 合成标注（04 附录 B）。
/// flatten 后 wire 形状 = KlinePayload 全部键 + `today_synthesized`。
#[derive(Debug, Serialize, ToSchema)]
pub struct KlineResponse {
    #[serde(flatten)]
    pub payload: KlinePayload,
    /// 本次响应的日线尾部是否合成了当日 bar（with_today 且满足全部合成条件）
    pub today_synthesized: bool,
}

#[utoipa::path(
    get,
    path = "/api/market/kline/{symbol}",
    operation_id = "market_kline",
    tag = "market",
    params(
        ("symbol" = String, Path, description = "标的 symbol（带 sh/sz 前缀，如 sh600519；B7：前缀由显式 mkt 字段拼装，不做号段猜测）"),
        KlineQuery,
    ),
    responses(
        (status = 200, description = "日线透传；with_today=true 且满足合成条件（实时行 date==今天、status=ok、price>0、日线尾部<今天）时追加当日 bar 并置 today_synthesized=true。实时拉取失败不影响日线供给（today_synthesized=false）", body = KlineResponse),
        (status = 502, description = "sidecar 上游错误契约透传，或契约漂移", body = ApiError),
        (status = 503, description = "sidecar 不可达（降级信号）", body = ApiError),
    )
)]
pub(crate) async fn kline(
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

    Json(KlineResponse {
        payload,
        today_synthesized: synthesized,
    })
    .into_response()
}

#[utoipa::path(
    get,
    path = "/api/market/cb_list",
    operation_id = "market_cb_list",
    tag = "market",
    responses(
        (status = 200, description = "转债列表（现券 + 集思录强赎字段）透传；强赎源整体降级时 redeem 键全 null、redeem_source=null（降级不断供）", body = CbListPayload),
        (status = 502, description = "sidecar 上游错误契约透传，或契约漂移", body = ApiError),
        (status = 503, description = "sidecar 不可达（降级信号）", body = ApiError),
    )
)]
pub(crate) async fn cb_list(State(m): State<SharedMarket>) -> Response {
    match m.api.cb_list().await {
        Ok(p) => Json(p).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CalendarQuery {
    /// 起始日期 YYYY-MM-DD（缺省 = 默认窗口今天 ±180 天，走适配层 1h 缓存）
    start: Option<String>,
    /// 结束日期 YYYY-MM-DD
    end: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/market/calendar",
    operation_id = "market_calendar",
    tag = "market",
    params(CalendarQuery),
    responses(
        (status = 200, description = "交易日历透传（升序 ISO 日期，含未来交易日）。默认窗口走适配层缓存（1h）；显式区间直连 sidecar", body = CalendarPayload),
        (status = 502, description = "sidecar 上游错误契约透传，或契约漂移", body = ApiError),
        (status = 503, description = "sidecar 不可达（降级信号）", body = ApiError),
    )
)]
pub(crate) async fn calendar(State(m): State<SharedMarket>, Query(q): Query<CalendarQuery>) -> Response {
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
///
/// P4-T6 起链体在 `zimu-agent::service::refresh_portfolio`——agent 工具
/// `refresh_quotes` 与本端点走同一条链，口径不分叉；这里只剩 HTTP 语义映射。
#[utoipa::path(
    post,
    path = "/api/market/refresh",
    operation_id = "market_refresh",
    tag = "market",
    responses(
        (status = 200, description = "行情回写 + recalc() 全量重算。仅 status=ok 且 price>0 的行回写；limit_pct 仅在数据层未配置时推断（ETF 不推断，不覆盖已配置值）；无行情标的进 skipped 保持旧值", body = RefreshResponse),
        (status = 502, description = "sidecar 上游错误契约透传，或契约漂移", body = ApiError),
        (status = 503, description = "sidecar 不可达：拒绝刷新、不写库（market_unavailable）", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub(crate) async fn refresh(State((store, m)): State<(SharedStore, SharedMarket)>) -> Response {
    use zimu_agent::service::{refresh_portfolio, RefreshError};
    match refresh_portfolio(&store, &*m.api, &m.sector_map).await {
        Ok(r) => Json(r).into_response(),
        Err(RefreshError::Store(msg)) => {
            tracing::error!("存储层错误：{msg}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new("store_error", "存储层内部错误，已记录日志")),
            )
                .into_response()
        }
        Err(RefreshError::Market(e)) => err_response(e),
    }
}
