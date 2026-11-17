//! 通道绑定管理 API（T5/ADR-0011 拍板⑤）：IM 入站白名单的界面管理面（数据在
//! `zimu.db channel_bindings`，`BoundOnly` decorator 判定同源）。
//!
//! 端点：
//! - `GET    /api/channel_bindings?channel=`       活跃绑定列表（bound_at 升序）
//! - `GET    /api/channel_bindings/pending`        待绑定观察队列（ADR-0017 D3 补记：
//!   未绑定来消息自动入列，绑定卡一键绑定——观察自动化、授权仍人工）
//! - `POST   /api/channel_bindings`                绑定/重绑（幂等 upsert；成功即
//!   从待绑定队列移除）
//! - `DELETE /api/channel_bindings/{channel}/{chat_id}`  解绑（置 active=0 留痕）
//!
//! 边界（拍板⑤）：本模块只管**绑定关系**（谁能对子木说话）——连接参数（凭据）
//! 在服务器配置文件编辑，这里不出现、不读写。真机绑定流程（QQ/微信同款）：
//! 未绑定来消息 → 待绑定队列自动列出（日志仍留案底兜底）→ 界面一键绑定。
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
use crate::pending_bindings::{PendingAttempt, PendingBindings};

/// 绑定路由共享状态：store（真值）+ 待绑定观察队列（内存态）。
type BindingsState = (SharedStore, PendingBindings);

/// `POST /api/channel_bindings` 请求体。
#[derive(Debug, Deserialize, ToSchema)]
pub struct BindingRequest {
    /// 通道词汇表：'qq' / 'wechat'（ADR-0017）；非空即可入库（向前兼容）
    pub channel: String,
    /// 平台会话 id（QQ 形如 private:{openid} / group:{openid}；
    /// 微信恒 private:{ilink_user_id}，仅 1:1）
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
    State((store, _pending)): State<BindingsState>,
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
    get,
    path = "/api/channel_bindings/pending",
    operation_id = "list_pending_bindings",
    tag = "system",
    responses(
        (status = 200, description = "待绑定观察队列（最近活跃在前，上限 20 条）：未绑定会话来消息被 BoundOnly 丢弃时自动入列（去重累计 hits）——绑定卡一键绑定的数据源。内存态不落库：重启清零，绑定成功即移除。入列 ≠ 放行，授权仍是 POST 绑定（ADR-0017 D3 补记）", body = Vec<PendingAttempt>),
    )
)]
async fn list_pending(State((_store, pending)): State<BindingsState>) -> Response {
    Json(pending.list()).into_response()
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
    State((store, pending)): State<BindingsState>,
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
    // 授权完成：待绑定观察记录使命结束（ADR-0017 D3 补记）
    pending.remove(channel, chat_id);
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
    State((store, _pending)): State<BindingsState>,
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

/// 绑定管理路由（进 `api_router`；store + 待绑定队列，agent 在线与否都可用）。
pub fn router(store: SharedStore, pending: PendingBindings) -> Router {
    Router::new()
        .route("/api/channel_bindings", get(list_bindings).post(bind))
        // 静态段 pending 先于 {channel} 注册（matchit 静态优先，GET 无冲突）
        .route("/api/channel_bindings/pending", get(list_pending))
        .route("/api/channel_bindings/{channel}/{chat_id}", axum::routing::delete(unbind))
        .with_state((store, pending))
}
