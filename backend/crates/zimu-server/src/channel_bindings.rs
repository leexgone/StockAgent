//! 通道绑定管理 API（T5/ADR-0011 拍板⑤）：IM 入站白名单的界面管理面（数据在
//! `zimu.db channel_bindings`，`BoundOnly` decorator 判定同源）。
//!
//! 端点：
//! - `GET    /api/channel_bindings?channel=`       活跃绑定列表（bound_at 升序）
//! - `POST   /api/channel_bindings`                绑定/重绑（幂等 upsert）
//! - `DELETE /api/channel_bindings/{channel}/{chat_id}`  解绑（置 active=0 留痕）
//!
//! 边界（拍板⑤）：本模块只管**绑定关系**（谁能对子木说话）——连接参数（凭据）
//! 在服务器配置文件编辑，这里不出现、不读写；QQ 的真机绑定流程 =
//! 未绑定来消息的日志里抄 chat_id → POST 绑定（wechat 屏的二维码流程随 C4/
//! 微信模块到货再挂，接口词汇 channel 已含 wechat 扩展位）。
//! kind 校验在前（400），存储 CHECK 兜底；解绑未命中 → 404（幂等语义同预警归档）。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use zimu_store::ChannelBindingRow;

use crate::chat_record::ts_now;
use crate::holdings_api::{lock, store_error, SharedStore};
use crate::openapi::ApiError;

/// `POST /api/channel_bindings` 请求体。
#[derive(Debug, Deserialize, ToSchema)]
pub struct BindingRequest {
    /// 通道词汇表：'qq'（'wechat' 随模块到货启用）；非空即可入库（向前兼容）
    pub channel: String,
    /// 平台会话 id（QQ 形如 private:{openid} / group:{openid}）
    pub chat_id: String,
    /// 'private' | 'group'
    pub kind: String,
    /// 展示名（可选）
    #[serde(default)]
    pub label: String,
}

/// 解绑结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct UnbindResult {
    pub unbound: bool,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BindingsQuery {
    /// 只看某通道；缺省全部通道
    channel: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/channel_bindings",
    tag = "system",
    params(BindingsQuery),
    responses(
        (status = 200, description = "活跃绑定列表（bound_at 升序）。绑定是入站白名单：未绑定会话的来消息被 BoundOnly 静默丢弃（日志留 chat_id 供绑定）", body = Vec<ChannelBindingRow>),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
async fn list_bindings(
    State(store): State<SharedStore>,
    Query(q): Query<BindingsQuery>,
) -> Response {
    match lock(&store).channel_bindings(q.channel.as_deref()) {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => {
            tracing::error!("channel_bindings 读取失败：{e:#}");
            store_error()
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/channel_bindings",
    operation_id = "bind_channel",
    tag = "system",
    request_body = BindingRequest,
    responses(
        (status = 200, description = "绑定/重绑成功（同 channel+chat_id 幂等 upsert：刷新 kind/label/bound_at 并激活）。返回生效行", body = ChannelBindingRow),
        (status = 400, description = "参数非法：channel/chat_id 为空或 kind 不是 private/group", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
async fn bind(
    State(store): State<SharedStore>,
    Json(req): Json<BindingRequest>,
) -> Response {
    let channel = req.channel.trim();
    let chat_id = req.chat_id.trim();
    if channel.is_empty() || chat_id.is_empty() {
        return bad_input("channel 与 chat_id 不能为空");
    }
    if req.kind != "private" && req.kind != "group" {
        return bad_input("kind 只接受 private / group");
    }
    let bound_at = ts_now();
    let result = lock(&store)
        .bind_channel(channel, chat_id, &req.kind, req.label.trim(), &bound_at)
        .map_err(|e| {
            tracing::error!("通道绑定写入失败：{e:#}");
            store_error()
        });
    if result.is_err() {
        return result.unwrap_err();
    }
    Json(ChannelBindingRow {
        channel: channel.to_string(),
        chat_id: chat_id.to_string(),
        kind: req.kind,
        label: req.label.trim().to_string(),
        bound_at,
        active: true,
    })
    .into_response()
}

#[utoipa::path(
    delete,
    path = "/api/channel_bindings/{channel}/{chat_id}",
    operation_id = "unbind_channel",
    tag = "system",
    params(
        ("channel" = String, Path, description = "通道（如 qq）"),
        ("chat_id" = String, Path, description = "平台会话 id"),
    ),
    responses(
        (status = 200, description = "解绑成功（置 active=0 留痕，不删行——推送审计要能对上号）", body = UnbindResult),
        (status = 404, description = "没有该活跃绑定", body = ApiError),
    )
)]
async fn unbind(
    State(store): State<SharedStore>,
    Path((channel, chat_id)): Path<(String, String)>,
) -> Response {
    match lock(&store).unbind_channel(&channel, &chat_id) {
        Ok(true) => Json(UnbindResult { unbound: true }).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "no_binding",
                format!("没有活跃绑定 {channel}/{chat_id}"),
            )),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("通道解绑失败：{e:#}");
            store_error()
        }
    }
}

fn bad_input(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError::new("bad_input", msg)),
    )
        .into_response()
}

/// 绑定管理路由（进 `api_router`；纯 store 状态，agent 在线与否都可用）。
pub fn router(store: SharedStore) -> Router {
    Router::new()
        .route("/api/channel_bindings", get(list_bindings).post(bind))
        .route("/api/channel_bindings/{channel}/{chat_id}", axum::routing::delete(unbind))
        .with_state(store)
}
