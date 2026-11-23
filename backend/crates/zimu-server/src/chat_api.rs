//! Web 会话档案 API（T3/ADR-0011）：读取与重置服务端会话真值。
//!
//! 端点：
//! - `GET  /api/chat/history?limit=&before_id=`  会话档案（时间**升序**返回，
//!   前端水合直接用；分页游标向"更早"走）——**不依赖 agent 在线**：档案是
//!   store 的，服务重启/agent 缺席时前端仍能水合历史（比旧 localStorage 强）。
//! - `POST /api/chat/reset`  清空 Web 会话档案 + 通知 agent 清内存历史 +
//!   push_log 留痕（Web 的 /new 语义）。
//! - `POST /api/chat/push_last`  手动问答推送（P7-T8，03 §12）：取档案最后
//!   一条**已归档** assistant 消息，经注入的通道闭包推给微信/QQ 全部绑定
//!   会话（kind `chat_push`，全文透传——长度守卫归 robit adapter，R16）。
//!
//! 边界：档案由 recorder/submit 链路写入（`chat_record` 状态机定义存什么），
//! 本模块只读与整会话清空，**不改写单条**；`tool` 行只供展示，复活回灌只取
//! user/assistant（陈旧数字不回灌上下文，03「派生数字现算」）。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use zimu_store::ChatMessageRow;

use crate::channel_push::ChannelPush;
use crate::chat_record::{ts_now, WEB_SESSION};
use crate::holdings_api::{lock, store_error, SharedStore};
use crate::openapi::ApiError;

/// 会话重置钩子：通知 agent 清空内存历史（bin 接 WebBridge；形状照
/// `scheduler::ReviewTrigger`——lib 不依赖 bin 的 AgentBridge，测试注入 fake）。
/// 返回值 = 是否已通知到在线 agent（离线 → false，档案清空照常，如实留痕）。
pub type ResetHook =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

#[derive(Clone)]
struct ChatState {
    store: SharedStore,
    reset: ResetHook,
    /// 问答推送通道闭包（P7-T8）：bin 按实际 spawn 的通道装配（make_channel_push，
    /// 全文透传——长度守卫归 robit adapter，ADR-0020）；空 vec = 双通道均未启用。
    /// 形状照 ResetHook 注入范式——lib 不依赖 bin 的 adapter 类型，测试注入
    /// recording 闭包即可断言 kind/正文/留痕。
    push: Vec<ChannelPush>,
}

/// `GET /api/chat/history` 响应（升序档案 + 更早页指针）。
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatHistory {
    /// 时间升序（旧→新）；role 词汇 user/assistant/tool
    pub messages: Vec<ChatMessageRow>,
    /// 是否还有更早的记录（配 `before_id` 翻页）
    pub has_more: bool,
}

/// `POST /api/chat/reset` 响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatResetResult {
    /// 清掉的档案行数
    pub cleared: usize,
    /// 是否已通知在线 agent 清内存历史（false = agent 离线，档案照常已清）
    pub agent_signaled: bool,
}

/// `POST /api/chat/push_last` 响应（P7-T8）。
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatPushResult {
    /// 是否送达至少一个通道（OR 语义，与预警/摘要复合 pusher 同款）
    pub ok: bool,
    /// 人读结果说明（可直接 toast）；逐会话送达细节在 push_log（kind chat_push）
    pub detail: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    /// 行数上限：1–500，缺省 100（取最近 N 条后按时间升序返回）
    limit: Option<i64>,
    /// 只取该 id 之前（更早）的记录——游标翻页
    before_id: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/chat/history",
    tag = "system",
    params(HistoryQuery),
    responses(
        (status = 200, description = "Web 会话档案（服务端真值，ADR-0011）：时间升序 + has_more 页尾标记。thinking 段与通知类不在档案内；tool 行为展示快照（{name,arguments,result,is_error}），复活回灌只取 user/assistant。不依赖 agent 在线。", body = ChatHistory),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
async fn get_history(
    State(st): State<ChatState>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = lock(&st.store).chat_messages(WEB_SESSION, limit, q.before_id);
    match rows {
        Ok((mut rows, has_more)) => {
            rows.reverse();
            Json(ChatHistory { messages: rows, has_more }).into_response()
        }
        Err(e) => {
            tracing::error!("chat history 读取失败：{e:#}");
            store_error()
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/chat/reset",
    operation_id = "chat_reset",
    tag = "system",
    responses(
        (status = 200, description = "会话档案已清空；agent_signaled=false 表示 agent 离线（档案照常清，push_log 如实记）。agent 在线时同时向其发送清历史指令。", body = ChatResetResult),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
async fn reset_session(State(st): State<ChatState>) -> Response {
    let cleared = {
        let guard = lock(&st.store);
        match guard.clear_chat_messages(WEB_SESSION) {
            Ok(n) => n,
            Err(e) => {
                tracing::error!("chat reset 清档案失败：{e:#}");
                return store_error();
            }
        }
    };
    // 通知 agent 清内存历史（robit 的 /clear 语义；回执文本由 recorder 特判丢弃）
    let signaled = (st.reset)().await;
    let detail = format!(
        "Web 会话档案清空 {cleared} 条；agent 内存历史{}",
        if signaled { "已通知清除" } else { "未在线，未通知" }
    );
    if let Err(e) = lock(&st.store).insert_push_log(&ts_now(), "agent", "web_session_reset", signaled, &detail) {
        // 留痕失败不影响重置结果本身，但要吼出来（与调度器「降级如实」纪律一致）
        tracing::warn!("chat reset 留痕失败：{e:#}");
    }
    tracing::info!("Web 会话已重置：{detail}");
    Json(ChatResetResult { cleared, agent_signaled: signaled }).into_response()
}

#[utoipa::path(
    post,
    path = "/api/chat/push_last",
    operation_id = "chat_push_last",
    tag = "system",
    responses(
        (status = 200, description = "手动问答推送（P7-T8，03 §12）：取档案最后一条已归档 assistant 消息推给微信/QQ 全部绑定会话（kind chat_push，全文透传——长度守卫归 robit adapter，R16）。ok=送达至少一通道；双通道均未启用 ok=false 如实说明；逐会话细节在 push_log。流式中途点击取上一条完成消息，不等待在途轮次。", body = ChatPushResult),
        (status = 400, description = "档案里没有可推送的助手消息（bad_input）", body = ApiError),
        (status = 500, description = "存储层内部错误", body = ApiError),
    )
)]
async fn push_last(State(st): State<ChatState>) -> Response {
    // 档案新序在前：扫第一条非空 assistant 行即「最后一条已归档回答」。
    // 500 = history 端点同款上限；tool/user 行跳过（tool 内容在 tool_json 不推）。
    let rows = match lock(&st.store).chat_messages(WEB_SESSION, 500, None) {
        Ok((rows, _)) => rows,
        Err(e) => {
            tracing::error!("chat push_last 读档案失败：{e:#}");
            return store_error();
        }
    };
    let last = rows
        .iter()
        .find(|r| r.role == "assistant" && !r.text.trim().is_empty());
    let Some(row) = last else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("bad_input", "会话还没有可推送的助手消息")),
        )
            .into_response();
    };
    if st.push.is_empty() {
        return Json(ChatPushResult {
            ok: false,
            detail: "微信/QQ 通道均未启用，无法推送（通道开关见 backend/.robit/config.toml）".into(),
        })
        .into_response();
    }
    // 正文头部行让接收端区分手动转发与自动推送（03 §12）；主题带档案 ts 可回查。
    let text = format!("子木问答 · 应主人要求转发最后一条回答\n\n{}", row.text);
    let subject = format!("问答推送 {}", row.ts);
    let total = st.push.len();
    let mut delivered = 0usize;
    for ch in st.push.iter() {
        if ch("chat_push", text.clone(), subject.clone()).await {
            delivered += 1;
        }
    }
    let ok = delivered > 0;
    let detail = if ok {
        format!("已推送至 {delivered}/{total} 个通道（逐会话细节见推送日志）")
    } else {
        "全部通道推送失败（细节见推送日志）".to_string()
    };
    tracing::info!("问答推送：{detail}");
    Json(ChatPushResult { ok, detail }).into_response()
}

/// Web 会话档案路由（进 `api_router` 装配；与 bin 侧 `/api/chat*` SSE 端点
/// 路径不同族不冲突——那些依赖 agent 在线，这三个只依赖 store + 注入钩子）。
pub fn router(store: SharedStore, reset: ResetHook, push: Vec<ChannelPush>) -> Router {
    Router::new()
        .route("/api/chat/history", get(get_history))
        .route("/api/chat/reset", post(reset_session))
        .route("/api/chat/push_last", post(push_last))
        .with_state(ChatState { store, reset, push })
}
