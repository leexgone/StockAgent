//! 建议留档读取 API（P5-T2）。
//!
//! 端点：`GET /api/advice?code=&limit=` —— advice_history 留档行，新在前。
//!
//! 边界：留档由 agent 工具链写入（P4-T6，ADR-0007——数字列 = `zimu-domain::advice`
//! 规则函数产出直接落库，不经模型转写；reason/risks 由 `finalize_advice` 回填
//! 模型文案）。本模块只做读取的 HTTP 接线，**不生成、不改写**任何内容；
//! `reason` 为空串 = 数字已留档、文案未回填（前端如实展示占位）。

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use utoipa::IntoParams;
use zimu_store::AdviceRow;

use crate::holdings_api::{lock, store_error, SharedStore};
use crate::openapi::ApiError;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AdviceQuery {
    /// 按标的代码过滤（如 600519）；缺省 = 全部标的
    code: Option<String>,
    /// 行数上限：1–500，缺省 50
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/advice",
    tag = "advice",
    params(AdviceQuery),
    responses(
        (status = 200, description = "建议留档行，新在前（generated_at DESC, id DESC）。数字列 = 规则函数产出（ADR-0007 数字不经模型转写）；reason 空串 = 文案未回填", body = Vec<AdviceRow>),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
pub async fn get_advice(State(store): State<SharedStore>, Query(q): Query<AdviceQuery>) -> Response {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match lock(&store).list_advice(q.code.as_deref(), limit) {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("advice 读取失败：{e:#}");
            store_error()
        }
    }
}
