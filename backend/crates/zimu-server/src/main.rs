//! 子木 · zimu-server —— 常驻 Web 服务（P2-T3：接入 robit Agent，SSE 事件流）。
//!
//! 端点：
//! - `GET  /api/health`       健康检查（含 agent_online）
//! - `GET  /api/chat/stream`  SSE：Agent 事件流（首帧 connected，之后 text_delta / tool_* / …）
//! - `POST /api/chat`         发送用户消息 `{ "text": "…" }`
//! - `POST /api/chat/confirm` 工具确认答复 `{ "tool_call_id": "…", "approved": true }`
//! - `POST /api/chat/cancel`  取消当前轮次
//! - 其余路径                 静态托管 `frontend/dist`，回落 `index.html`（SPA 路由）
//!
//! 启动（在 `backend/` 下）：`cargo run -p zimu-server`
//! robit 配置读取与 zimu-agent-cli 相同（`<cwd>/.robit/config.toml` → `~/.robit/config.toml`）；
//! 找不到配置时**服务照常起**，仅 /api/chat* 返回 503（静态首页仍可用）。
//!
//! 后续（见 docs/06-roadmap.md）：P2-T5 评估把 Web 桥沉淀进 robit；
//! 部署约束：仅内网监听，默认不开公网入站（ADR-0005）。

mod agent_bridge;

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use agent_bridge::{resolve_confirm, spawn_agent, AgentBridge, SseEvent};
use anyhow::Context;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{extract::State, Json, Router};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    static_dir: PathBuf,
    /// None = robit 配置缺失或 Agent 启动失败：chat 端点降级 503，静态托管照常
    agent: Option<Arc<AgentBridge>>,
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    service: &'static str,
    version: &'static str,
    static_dir: PathBuf,
    /// frontend/dist/index.html 是否存在（false = 前端还没构建）
    static_ready: bool,
    /// Agent 是否在线（false = 缺 robit 配置或启动失败）
    agent_online: bool,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        ok: true,
        service: "zimu-server",
        version: env!("CARGO_PKG_VERSION"),
        static_dir: state.static_dir.clone(),
        static_ready: state.static_dir.join("index.html").exists(),
        agent_online: state.agent.is_some(),
    })
}

// ============================================================================
// /api/chat* —— P2-T3 SSE 会话端点
// ============================================================================

#[derive(Deserialize)]
struct ChatRequest {
    text: String,
}

#[derive(Deserialize)]
struct ConfirmRequest {
    tool_call_id: String,
    approved: bool,
}

/// SSE 事件流。多客户端可同时观看（broadcast 扇出）；订阅前的事件不会补发，
/// 首帧固定是 connected。
async fn chat_stream(State(state): State<AppState>) -> Result<Response, StatusCode> {
    let Some(agent) = state.agent.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let hello = SseEvent::Connected {
        profile: agent.profile.clone(),
        model: agent.model.clone(),
        tools: agent.tools.clone(),
    };
    let rx = agent.events.subscribe();
    let stream = tokio_stream::iter(vec![sse_json(&hello)]).chain(BroadcastStream::new(rx).map(
        |res| match res {
            Ok(event) => sse_json(&event),
            // 消费端太慢丢帧：告知前端而不是断流
            Err(lagged) => Ok(Event::default().comment(format!("stream lagged: {lagged}"))),
        },
    ));
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

fn sse_json(event: &SseEvent) -> Result<Event, Infallible> {
    Ok(Event::default()
        .json_data(event)
        .unwrap_or_else(|e| {
            Event::default()
                .data(format!(r#"{{"type":"error","message":"SSE 序列化失败: {e}"}}"#))
        }))
}

async fn chat(State(state): State<AppState>, Json(req): Json<ChatRequest>) -> Response {
    let Some(agent) = state.agent.clone() else {
        return agent_offline();
    };
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, "消息为空").into_response();
    }
    match agent.message_tx.send(text.into()).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => agent_offline(),
    }
}

async fn chat_confirm(State(state): State<AppState>, Json(req): Json<ConfirmRequest>) -> Response {
    let Some(agent) = state.agent.clone() else {
        return agent_offline();
    };
    if resolve_confirm(&agent.confirm_slot, &req.tool_call_id, req.approved).await {
        StatusCode::OK.into_response()
    } else {
        (StatusCode::NOT_FOUND, "没有匹配的待确认工具调用").into_response()
    }
}

async fn chat_cancel(State(state): State<AppState>) -> Response {
    let Some(agent) = state.agent.clone() else {
        return agent_offline();
    };
    match agent
        .message_tx
        .send(robit_agent::FrontendMessage::Cancel)
        .await
    {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => agent_offline(),
    }
}

fn agent_offline() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Agent 未启动：检查 robit 配置（~/.robit/config.toml 与 .env）后重启服务",
    )
        .into_response()
}

// ============================================================================
// 启动装配
// ============================================================================

/// 静态目录：`ZIMU_STATIC_DIR` 覆盖；默认按编译期 manifest 目录定位到
/// `仓库根/frontend/dist`（backend/crates/zimu-server → ../../..），
/// 因此从任何工作目录 `cargo run` 都能找到。
/// 词法规范化掉 `..` 分量：health 输出的路径可读，也避免下游对非常规路径的边界行为。
fn resolve_static_dir() -> PathBuf {
    let raw = std::env::var("ZIMU_STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../frontend/dist"));
    let mut out = PathBuf::new();
    for comp in raw.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Agent 的工作目录 = 启动目录（会话库/记忆写 <cwd>/.robit/，已 gitignore），
    // 与 zimu-agent-cli 一致：请在 backend/ 下启动。
    let working_dir =
        std::fs::canonicalize(std::env::current_dir().context("无法读取当前工作目录")?)?;

    // 日志二选一：robit 的 init_logging 会独占全局 subscriber（宿主无法叠加自己的
    // fmt 层——R 系列痛点，见 docs/07-backlog.md）。有 robit 配置就交给它
    //（附带 <cwd>/.robit/logs 文件日志），没有则回落服务自己的 tracing_subscriber。
    let agent = match robit_ai::load_config(None) {
        Ok(config) => {
            robit_ai::init_logging(
                config.app.as_ref(),
                "zimu_server",
                &working_dir,
                &["tower_http=debug"],
            );
            match spawn_agent(working_dir.clone(), config) {
                Ok(bridge) => Some(Arc::new(bridge)),
                Err(e) => {
                    tracing::error!("Agent 启动失败，/api/chat* 将返回 503：{e:#}");
                    None
                }
            }
        }
        Err(e) => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,tower_http=debug".into()),
                )
                .init();
            tracing::warn!("未找到 robit 配置（{e}）：/api/chat* 返回 503，静态托管照常");
            None
        }
    };

    let static_dir = resolve_static_dir();
    if !static_dir.join("index.html").exists() {
        tracing::warn!(
            "未找到前端构建产物 {}：先在 frontend/ 执行 `npm install && npm run build`；此前仅 /api/* 可用",
            static_dir.display()
        );
    }

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/chat/stream", get(chat_stream))
        .route("/api/chat", post(chat))
        .route("/api/chat/confirm", post(chat_confirm))
        .route("/api/chat/cancel", post(chat_cancel))
        .with_state(AppState {
            static_dir: static_dir.clone(),
            agent,
        })
        // SPA 回落：dist 里不存在的路径返回 index.html。
        // 注意必须用 .fallback() 而不是 .not_found_service()：后者会把状态码强制成 404
        //（tower-http 0.6 的设计，body 照常返回），客户端路由刷新就会拿到 404。
        .fallback_service(
            ServeDir::new(&static_dir).fallback(ServeFile::new(static_dir.join("index.html"))),
        )
        .layer(TraceLayer::new_for_http());

    // 仅内网/本机监听；LAN 部署改 ZIMU_ADDR，公网入站默认禁止（ADR-0005 第 3 条）
    let addr = std::env::var("ZIMU_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("绑定 {addr} 失败"))?;
    tracing::info!(
        "子木服务已监听 http://{addr}（静态目录：{}）",
        static_dir.display()
    );

    axum::serve(listener, app).await.context("服务异常退出")?;
    Ok(())
}
