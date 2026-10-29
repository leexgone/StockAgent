//! 自选 API（P5-T2，B13 定稿 2026-10-29：schema v2 watchlist 表，03 §12
//! 预警判定范围扩到自选）。
//!
//! 端点：
//! - `GET    /api/watchlist`        自选全量（加入时间序）
//! - `POST   /api/watchlist`        添加/更新自选（按 code upsert，幂等）
//! - `DELETE /api/watchlist/{code}` 移出自选（返回被移除行）
//!
//! 边界：自选表只存观察标的的静态属性（holdings 静态列同款 + note）；
//! 行情**不回写本表**（调度器分钟级拉取仅用于预警判定，§12），前端展示价
//! 走 `/api/market/quote`。预警判定按「无持仓形态」（cost/stop/target_hi
//! 不喂，序 3/4/5 不参与）——那是调度器与 `zimu-domain::alert` 的事，
//! 本模块只是 CRUD 接线。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use zimu_store::{NewWatchlist, WatchlistError, WatchlistRow};

use crate::holdings_api::{lock, store_error, SharedStore};
use crate::openapi::ApiError;

/// added_at 用上海时刻（与 alerts/push_log 的 ts 同口径）
fn now_sh() -> String {
    zimu_market::clock::now_shanghai()
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// WatchlistError → HTTP：输入问题 400、存储内部错误 500（细节只进日志）
fn watchlist_error_response(e: WatchlistError) -> Response {
    match e {
        WatchlistError::BadInput(m) => {
            (StatusCode::BAD_REQUEST, Json(ApiError::new("bad_input", m))).into_response()
        }
        WatchlistError::Store(m) => {
            tracing::error!("watchlist 存储错误：{m}");
            store_error()
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/watchlist",
    tag = "watchlist",
    responses(
        (status = 200, description = "自选全量，加入时间序（added_at, code）。观察标的的静态属性（无 shares/cost——非持仓）；行情不回写本表，展示价走 /api/market/quote", body = Vec<WatchlistRow>),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn get_watchlist(State(store): State<SharedStore>) -> Response {
    match lock(&store).watchlist() {
        Ok(v) => Json(v).into_response(),
        Err(e) => watchlist_error_response(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/watchlist",
    tag = "watchlist",
    request_body = NewWatchlist,
    responses(
        (status = 200, description = "添加/更新后的行。按 code upsert 幂等：重复添加 = 更新名称/元信息/备注，added_at 保留首次值", body = WatchlistRow),
        (status = 400, description = "输入问题：code/name 空、市场码非法（bad_input，B7 显式 mkt）", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn add_watchlist(
    State(store): State<SharedStore>,
    Json(req): Json<NewWatchlist>,
) -> Response {
    match lock(&store).add_watchlist(&req, &now_sh()) {
        Ok(row) => Json(row).into_response(),
        Err(e) => watchlist_error_response(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/watchlist/{code}",
    tag = "watchlist",
    params(
        ("code" = String, Path, description = "标的代码（6 位）"),
    ),
    responses(
        (status = 200, description = "移除成功，返回被移除行。只动自选表——持仓、预警历史与留档不受影响", body = WatchlistRow),
        (status = 404, description = "自选里没有该代码（no_watchlist_item）", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn remove_watchlist(
    State(store): State<SharedStore>,
    Path(code): Path<String>,
) -> Response {
    match lock(&store).remove_watchlist(&code) {
        Ok(Some(row)) => Json(row).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "no_watchlist_item",
                format!("自选里没有 {code}"),
            )),
        )
            .into_response(),
        Err(e) => watchlist_error_response(e),
    }
}
