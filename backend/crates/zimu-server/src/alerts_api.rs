//! 预警与净值快照 API（P4-T7）。
//!
//! 端点：
//! - `GET  /api/alerts?done=&limit=`   预警列表（新在前；done 过滤归档态）
//! - `POST /api/alerts/{id}/done`      归档一条预警（done=1，不删行）
//! - `GET  /api/equity_snapshots`      净值快照（date 升序——净值曲线输入形态）
//!
//! 边界：预警由调度引擎判定并落库（`zimu-agent::scheduler` + `zimu-domain::alert`，
//! 口径 03 §12），本模块只做读取与归档的 HTTP 接线——**不生成、不改写**预警内容；
//! 快照由 15:05 盘后链写入（数字 = `recalc` agg 原值），这里原样读出。
//! 推送留痕（push_log）暂无读端点——wechat 屏（P5-T2，gate C4）落地时再加。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use utoipa::IntoParams;
use zimu_store::{AlertRow, EquitySnapshotRow};

use crate::holdings_api::{lock, store_error, SharedStore};
use crate::openapi::ApiError;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AlertsQuery {
    /// true = 只看已归档 / false = 只看待处理 / 缺省全部
    done: Option<bool>,
    /// 行数上限：1–500，缺省 100
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/alerts",
    tag = "alerts",
    params(AlertsQuery),
    responses(
        (status = 200, description = "预警列表，新在前（ts DESC, id DESC）。由调度引擎在交易日盘中判定落库（03 §12：type 封闭词汇、level up/down/warn、同日同型去重）；type/level/text 全部出自规则函数，端点不改写", body = Vec<AlertRow>),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn get_alerts(State(store): State<SharedStore>, Query(q): Query<AlertsQuery>) -> Response {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    match lock(&store).list_alerts(q.done, limit) {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("alerts 读取失败：{e:#}");
            store_error()
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/alerts/{id}/done",
    tag = "alerts",
    responses(
        (status = 200, description = "归档成功，返回更新后的行（done=true；归档不删行，重复归档幂等）", body = AlertRow),
        (status = 404, description = "没有该 id 的预警（no_alert）", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn mark_alert_done(State(store): State<SharedStore>, Path(id): Path<i64>) -> Response {
    match lock(&store).mark_alert_done(id) {
        Ok(Some(row)) => Json(row).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("no_alert", format!("没有 id={id} 的预警"))),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("预警归档失败：{e:#}");
            store_error()
        }
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EquitySnapshotsQuery {
    /// 条数上限（取**最近** N 条，返回仍按 date 升序）：1–500，缺省 120
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/equity_snapshots",
    tag = "alerts",
    params(EquitySnapshotsQuery),
    responses(
        (status = 200, description = "净值快照，date 升序（净值曲线的输入形态）。每交易日一行，由 15:05 盘后链写入（03 §12）；数字 = zimu-domain::recalc 汇总原值，不舍入、逐位可回溯", body = Vec<EquitySnapshotRow>),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn get_equity_snapshots(
    State(store): State<SharedStore>,
    Query(q): Query<EquitySnapshotsQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(120).clamp(1, 500);
    match lock(&store).equity_snapshots(limit) {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("equity_snapshots 读取失败：{e:#}");
            store_error()
        }
    }
}
