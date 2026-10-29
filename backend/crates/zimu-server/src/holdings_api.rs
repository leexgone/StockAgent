//! 持仓与录入 API（P4-T1：手工逐笔录入流程，ADR-0006）。
//!
//! 端点：
//! - `GET  /api/holdings`        组合全景（存储行 + zimu-domain recalc 派生与汇总）
//! - `POST /api/holdings/trade`  记一笔买入/卖出：校验→试算→事务落库，返回收据
//! - `GET  /api/trades?limit=N`  委托/成交流水（最新在前）
//! - `POST /api/account`         首次配置/调整账户
//!
//! 边界：
//! - 校验口径全部在 zimu-store/zimu-domain，本模块只做 HTTP 接线与错误→状态码映射；
//!   拒绝原因带中文说明，前端可直接展示（原型抽屉的提示来自同一批规则）。
//! - 契约（P4-T5）：每个 handler 的 `#[utoipa::path]` 标注即 OpenAPI 文档来源，
//!   错误响应统一 `ApiError`（openapi.rs）；改响应形状 = 改结构体，文档跟着走。
//! - rusqlite Connection 非 Sync：用 std Mutex 串行化访问。操作全是同步短事务，
//!   跨 `.await` 持锁被禁止——本模块处理器无 await，锁在语句结束即释放。

use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use utoipa::{IntoParams, ToSchema};
use zimu_store::{AccountRow, PortfolioView, RecordError, RecordReceipt, Store, TradeRequest, TradeRow};

use crate::openapi::ApiError;

pub type SharedStore = Arc<Mutex<Store>>;

/// 锁中毒（上一持有者 panic）不致命：按中毒前的值继续（into_inner）。
pub(crate) fn lock(store: &SharedStore) -> MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// 存储内部错误 → 500（细节只进日志，不把 SQL 吐给客户端）。调用点负责打日志。
pub(crate) fn store_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError::new("store_error", "存储层内部错误，已记录日志")),
    )
        .into_response()
}

/// RecordError → HTTP。语义分层：输入问题 400、标的状态 404/422、
/// 存储内部错误 500。
fn record_error_response(e: RecordError) -> Response {
    let (status, code, msg) = match &e {
        RecordError::BadInput(m) => (StatusCode::BAD_REQUEST, "bad_input", m.clone()),
        RecordError::NoHolding { code } => (
            StatusCode::NOT_FOUND,
            "no_holding",
            format!("持仓中没有 {code}"),
        ),
        RecordError::NeedMeta { code } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "need_meta",
            format!("{code} 尚未持有：新建持仓需要标的信息（名称/市场/类型）"),
        ),
        RecordError::OutOfLimitRange { price, lo, hi, .. } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "out_of_limit_range",
            format!("委托价 {price} 超出当日涨跌停区间 [{lo}, {hi}]"),
        ),
        RecordError::InsufficientCash { cash_after } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "insufficient_cash",
            format!("现金不足（本笔后将为 {cash_after:.2}）"),
        ),
        RecordError::Store(m) => {
            tracing::error!("存储层错误：{m}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_error",
                "存储层内部错误，已记录日志".to_string(),
            )
        }
    };
    let mut body = ApiError::new(code, msg);
    // 特定拒绝带附加数值：前端抽屉可就地高亮（显示与拦截同一对限价，03 §2）
    match e {
        RecordError::OutOfLimitRange { lo, hi, prec, .. } => {
            body.limit_down = Some(lo);
            body.limit_up = Some(hi);
            body.decimals = Some(prec);
        }
        RecordError::InsufficientCash { cash_after } => body.cash_after = Some(cash_after),
        _ => {}
    }
    (status, Json(body)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/holdings",
    tag = "holdings",
    responses(
        (status = 200, description = "组合全景：存储行 + recalc() 逐笔派生（per，与 holdings 同序）与汇总（agg）+ 账户现金。无行情降级按 03 §5（price 缺失以成本顶替、prev 缺失视为等于 price）", body = PortfolioView),
        (status = 500, description = "存储层内部错误（细节只进日志）", body = ApiError),
    )
)]
pub async fn get_holdings(State(store): State<SharedStore>) -> Response {
    match lock(&store).portfolio() {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("portfolio 读取失败：{e:#}");
            store_error()
        }
    }
}

#[derive(serde::Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TradesQuery {
    /// 行数上限：1–500，缺省 50（防御性截断，避免一次拉爆响应体）
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/trades",
    tag = "holdings",
    params(TradesQuery),
    responses(
        (status = 200, description = "委托/成交流水，最新在前（费用三分项留档可审计）", body = Vec<TradeRow>),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn get_trades(State(store): State<SharedStore>, Query(q): Query<TradesQuery>) -> Response {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match lock(&store).trades(limit) {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("trades 读取失败：{e:#}");
            store_error()
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/holdings/trade",
    tag = "holdings",
    request_body = TradeRequest,
    responses(
        (status = 201, description = "录入收据：trade 行 + 变动后持仓（清仓为 null）+ 试算预览 + warnings（纪律提示不拦提交，如「单票将超过 25% 上限」）", body = RecordReceipt),
        (status = 400, description = "输入问题：数量非整手/超可卖、价格非正、市场码非法", body = ApiError),
        (status = 404, description = "持仓里没有该标的（卖出/减仓）", body = ApiError),
        (status = 422, description = "业务规则拦截：涨跌停越界（out_of_limit_range，带 limit_down/limit_up/decimals）、现金不足（insufficient_cash，带 cash_after）、新建持仓缺 meta（need_meta）", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn record_trade(
    State(store): State<SharedStore>,
    Json(req): Json<TradeRequest>,
) -> Response {
    match lock(&store).record_trade(&req) {
        Ok(r) => (StatusCode::CREATED, Json(r)).into_response(),
        Err(e) => record_error_response(e),
    }
}

/// 首次配置/调整账户（onboarding 只带现金；缺失字段保持原值）。
#[derive(serde::Deserialize, ToSchema)]
pub struct AccountPatch {
    pub name: Option<String>,
    pub init: Option<String>,
    pub style: Option<String>,
    pub broker: Option<String>,
    /// 可用现金（负数拒绝 400——现金下限是业务事实，不是 UI 校验）
    pub cash: Option<f64>,
}

#[utoipa::path(
    post,
    path = "/api/account",
    tag = "holdings",
    request_body = AccountPatch,
    responses(
        (status = 200, description = "更新后的账户（缺失字段保持原值）", body = AccountRow),
        (status = 400, description = "现金为负等输入问题", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn update_account(
    State(store): State<SharedStore>,
    Json(patch): Json<AccountPatch>,
) -> Response {
    let updated = lock(&store).update_account(
        patch.name.as_deref(),
        patch.init.as_deref(),
        patch.style.as_deref(),
        patch.broker.as_deref(),
        patch.cash,
    );
    match updated {
        Ok(acc) => Json(acc).into_response(),
        Err(e) => record_error_response(e),
    }
}
