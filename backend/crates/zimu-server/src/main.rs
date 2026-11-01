//! 子木 · zimu-server —— 常驻 Web 服务（P2-T3：接入 robit Agent，SSE 事件流）。
//!
//! 端点：
//! - `GET  /api/health`       健康检查（含 agent_online / market_url）
//! - `GET  /api/chat/stream`  SSE：Agent 事件流（首帧 connected，之后 text_delta / tool_* / …）
//! - `POST /api/chat`         发送用户消息 `{ "text": "…" }`
//! - `POST /api/chat/confirm` 工具确认答复 `{ "tool_call_id": "…", "approved": true }`
//! - `POST /api/chat/cancel`  取消当前轮次
//! - `/api/holdings*` `/api/trades` `/api/account`   持仓录入 API（P4-T1，见 lib holdings_api）
//! - `/api/market/*`          行情 API（P4-T4，见 lib market_api：status/quote/kline/
//!                            cb_list/calendar/refresh——sidecar 挂了整体降级不瘫）
//! - `/api/alerts*` `/api/equity_snapshots` `/api/push_logs`  预警、净值快照与推送
//!                            留痕（P4-T7 + P5-T2，见 lib alerts_api：内容由调度引擎
//!                            判定落库，03 §12；端点只读取与归档）
//! - `/api/advice`             建议留档读取（P5-T2，见 lib advice_api；写入在 agent
//!                            工具链，ADR-0007 数字归规则、文字归模型）
//! - `/api/watchlist*`         自选 CRUD（P5-T2，B13 定稿：schema v2，03 §12
//!                            预警范围扩到自选；调度器判定，行情不回写自选表）
//! - `GET  /api/openapi.json` API 契约文档（P4-T5，见 lib openapi）
//! - 其余路径                 静态托管 `frontend/dist`，回落 `index.html`（SPA 路由）
//!
//! bin 侧还负责启动**调度与预警引擎**（P4-T7，`zimu_agent::scheduler`）：
//! 交易日盘中分钟级轮询 + 15:05 盘后链（快照 + /复盘 注入会话）；
//! `ZIMU_SCHEDULER_DISABLE=1` 停用。
//!
//! health/chat 的响应与请求类型定义在 lib（`openapi.rs`）并进 OpenAPI 契约；
//! bin 侧单测断言 Health 序列化键集与 schema 键集一致（doc-stub 不留漂移盲区）。
//!
//! 业务路由在库目标（`lib.rs` 的 `api_router`）里装配，bin 只剩启动接线——
//! 集成测试用同一函数拉起 API 面。
//!
//! 启动（在 `backend/` 下）：`cargo run -p zimu-server`
//! 停止：**Ctrl+C 优雅关闭**——中止调度器 → 翻 watch 通知断开全部 SSE 长连接
//! → axum 等在途请求完成 → 退出（SQLite 连接与 Agent 会话随进程释放；
//! SSE 流永不自然结束，不主动断开的话 graceful shutdown 会一直等它）。
//! 数据库位置：`ZIMU_DB_PATH` 覆盖；默认 `<cwd>/zimu.db`（*.db 已在 .gitignore，永不入库）。
//! 行情 sidecar：`ZIMU_MARKET_URL` 覆盖（默认 http://127.0.0.1:8300），
//! `ZIMU_MARKET_TIMEOUT_MS` 覆盖超时（默认 10000）；sidecar 不在也能启动。
//! robit 配置读取与 zimu-agent-cli 相同（`<cwd>/.robit/config.toml` → `~/.robit/config.toml`）；
//! 找不到配置时**服务照常起**，仅 /api/chat* 返回 503（静态首页仍可用）。
//!
//! P2-T5 起：事件投影 / 确认槽位 / 扇出由 robit-web（v0.1.26）承担，
//! 本文件只剩 axum 接线——SSE 编码、connected 首帧（宿主元数据）、
//! POST 端点到 `WebBridge` 的路由。部署约束：仅内网监听，默认不开
//! 公网入站（ADR-0005）。

mod agent_bridge;

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use agent_bridge::{spawn_agent, AgentBridge};
use anyhow::Context;
use zimu_market::client::{HttpMarketClient, DEFAULT_BASE_URL, DEFAULT_TIMEOUT};
use zimu_market::MarketApi as _; // base_url() 是 trait 方法
use zimu_server::holdings_api::SharedStore;
use zimu_server::openapi::{ChatRequest, ConfirmRequest, Health};
use zimu_server::{api_router, market_api};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{extract::State, Json, Router};
use serde::Serialize;
use serde_json::json;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};
use tokio_stream::{Stream, StreamExt};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer as _;

#[derive(Clone)]
struct AppState {
    static_dir: PathBuf,
    /// None = robit 配置缺失或 Agent 启动失败：chat 端点降级 503，静态托管照常
    agent: Option<Arc<AgentBridge>>,
    /// 行情 sidecar 基地址（health 诊断透出；在线状态看 /api/market/status）
    market_url: String,
    /// 优雅关闭通知：Ctrl+C 后翻 true，SSE 长连接随之终止（见 shutdown_signal）
    shutdown_rx: watch::Receiver<bool>,
    // 持仓存储（P4-T1）与行情路由（P4-T4）不从这里走：api_router 用独立状态 merge 进来
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        ok: true,
        service: "zimu-server".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        static_dir: state.static_dir.display().to_string(),
        static_ready: state.static_dir.join("index.html").exists(),
        agent_online: state.agent.is_some(),
        market_url: state.market_url.clone(),
    })
}

// ============================================================================
// /api/chat* —— P2-T3 SSE 会话端点（请求类型在 lib openapi.rs，进契约）
// ============================================================================

/// SSE 事件流。多客户端可同时观看（broadcast 扇出）；订阅前的事件不会补发，
/// 首帧固定是 connected。
async fn chat_stream(State(state): State<AppState>) -> Result<Response, StatusCode> {
    let Some(agent) = state.agent.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    // connected 首帧是宿主元数据（robit-web 不知道子木的 profile/tools 面），
    // wire 形状沿用 P2-T3 起前端 ChatPanel 消费的约定；skills 键为 P4-T6 追加
    //（前端未消费时忽略，additive 不破坏）
    let hello = json!({
        "type": "connected",
        "profile": agent.profile,
        "model": agent.model,
        "tools": agent.tools,
        "skills": agent.skills,
    });
    let rx = agent.web.subscribe();
    let events = tokio_stream::iter(vec![sse_json(&hello)]).chain(BroadcastStream::new(rx).map(
        |res| match res {
            Ok(event) => sse_json(&event),
            // 消费端太慢丢帧：告知前端而不是断流
            Err(lagged) => Ok(Event::default().comment(format!("stream lagged: {lagged}"))),
        },
    ));
    // 优雅关闭时终止长连接：axum 的 graceful shutdown 会等在途响应全部完成，
    // 而 broadcast 流永不自然结束——Ctrl+C 后 watch 翻 true，信号流让响应收尾。
    let stream = TakeUntilSignal {
        inner: events,
        signal: WatchStream::from_changes(state.shutdown_rx.clone()),
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// 带终止信号的流适配器：等价 futures-util 的 `take_until_other`
///（tokio-stream 的 StreamExt 没有该组合子；不为一个组合子引入 futures-util 直接依赖）。
/// 信号流产出任意一项即终止输出；两条流都要求 Unpin（tokio-stream 适配器均满足）。
struct TakeUntilSignal<S, U> {
    inner: S,
    signal: U,
}

impl<S: Stream + Unpin, U: Stream + Unpin> Stream for TakeUntilSignal<S, U> {
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if std::pin::Pin::new(&mut self.signal).poll_next(cx).is_ready() {
            return Poll::Ready(None); // 关闭信号已到：SSE 响应就此收尾
        }
        std::pin::Pin::new(&mut self.inner).poll_next(cx)
    }
}

fn sse_json(event: &impl Serialize) -> Result<Event, Infallible> {
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
    match agent.web.send_user_text(text).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => agent_offline(),
    }
}

async fn chat_confirm(State(state): State<AppState>, Json(req): Json<ConfirmRequest>) -> Response {
    let Some(agent) = state.agent.clone() else {
        return agent_offline();
    };
    if agent.web.resolve_confirm(&req.tool_call_id, req.approved).await {
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
        .web
        .send_message(robit_agent::FrontendMessage::Cancel)
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

/// Ctrl+C 优雅关闭序列：先中止调度器（停止盘中轮询/盘后链的后台写入），
/// 再翻 watch 断开全部 SSE 长连接，让 axum 等在途请求完成后退出。
/// 注意：信号已被 tokio 接管，连按第二次 Ctrl+C 不会强杀进程——等排空即可；
/// 真卡死就用系统手段（任务管理器 / `taskkill`）。
async fn shutdown_signal(scheduler: Option<JoinHandle<()>>, sse: watch::Sender<bool>) {
    if let Err(e) = tokio::signal::ctrl_c().await {
        // 监听失败（极罕见）：服务继续运行，但 Ctrl+C 已被 tokio 接管、无人响应，
        // 如实报错并挂起，避免误触发关闭。
        tracing::error!("Ctrl+C 信号监听失败：{e}——优雅关闭不可用，需系统手段停止进程");
        std::future::pending::<()>().await;
    }
    tracing::info!("收到 Ctrl+C：停止调度器、断开 SSE 长连接、等待在途请求完成…");
    if let Some(h) = scheduler {
        h.abort();
    }
    // 无接收者（从未有 SSE 客户端连过）时 send 返回 Err，属正常情况
    let _ = sse.send(true);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Agent 的工作目录 = 启动目录（会话库/记忆写 <cwd>/.robit/，已 gitignore），
    // 与 zimu-agent-cli 一致：请在 backend/ 下启动。
    let working_dir =
        std::fs::canonicalize(std::env::current_dir().context("无法读取当前工作目录")?)?;

    // 日志（robit 0.1.27 起走 R3 可组合原语，单一初始化路径）：宿主 fmt 层 +
    // robit 文件日志层（log_file 未启用/打开失败返回 None，Option 自身实现 Layer
    // 可直接 with）+ build_log_filter 声明宿主 workspace crate 清单——zimu_agent
    // （调度器/工具）与 zimu_market（适配器）不再靠 additional_directives 手拼
    // "{crate}={level}" 字符串放行（旧 R3 痛点：不显式放行就被静默吞掉）。
    // 无 robit 配置（load_config Err）时 app_config = None：filter 退 info 默认、
    // 文件层为 None——服务照常起，不再需要「二选一」分支。
    // 配置加载与 Agent 启动拆开（P4-T6）：store/market 先于 Agent 装配——
    // 领域工具与 HTTP API 共用同一份句柄，spawn_agent 需要它们。
    let robit_config = robit_ai::load_config(None);
    let app_config = robit_config.as_ref().ok().and_then(|c| c.app.as_ref());
    let filter = robit_ai::build_log_filter(
        app_config,
        &["zimu_server", "zimu_agent", "zimu_market"],
        &["tower_http=debug"],
    );
    let file_layer = robit_ai::log_file_layer(app_config, &working_dir, filter.clone());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter))
        .with(file_layer)
        .init();
    robit_ai::install_panic_hook();

    let static_dir = resolve_static_dir();
    if !static_dir.join("index.html").exists() {
        tracing::warn!(
            "未找到前端构建产物 {}：先在 frontend/ 执行 `npm install && npm run build`；此前仅 /api/* 可用",
            static_dir.display()
        );
    }

    // 持仓数据库（P4-T1）：ZIMU_DB_PATH 覆盖，默认 <cwd>/zimu.db——*.db 已被 .gitignore 挡住，
    // 真实持仓永不入库（git）。打开即跑迁移（zimu-store::migrate，幂等）。
    let db_path = std::env::var("ZIMU_DB_PATH").unwrap_or_else(|_| "zimu.db".into());
    let store = SharedStore::new(Mutex::new(
        zimu_store::Store::open(&db_path).with_context(|| format!("打开持仓数据库失败：{db_path}"))?,
    ));
    tracing::info!(
        "持仓数据库就绪：{}（schema v{}）",
        db_path,
        store.lock().unwrap_or_else(|e| e.into_inner()).version()?
    );

    // 行情适配器（P4-T4）：sidecar 唯一消费入口。sidecar 不在也能起服务——
    // 行情端点届时 503 降级（market_unavailable），持仓/会话/静态托管照常。
    let market_client = Arc::new(HttpMarketClient::from_env().unwrap_or_else(|e| {
        tracing::warn!("按环境装配行情适配器失败（{e}），回落默认配置");
        HttpMarketClient::new(DEFAULT_BASE_URL, DEFAULT_TIMEOUT)
            .expect("默认配置的行情客户端构建不应失败")
    }));
    let market_url = market_client.base_url().to_string();
    tracing::info!("行情适配器就绪：{market_url}（sidecar 不在线时 /api/market/* 降级 503）");
    let market = market_api::new_market(market_client.clone());

    // Agent（P4-T6：领域工具 + Skill 模板在 zimu-agent crate，装配见 agent_bridge）。
    // 工具与 HTTP API 拿同一份 store/market 句柄——refresh_quotes 工具与
    // POST /api/market/refresh 是同一条链，agent 出的数字与端点同源。
    let agent = match robit_config {
        Ok(config) => {
            match spawn_agent(working_dir.clone(), config, store.clone(), market_client.clone()) {
                Ok(bridge) => Some(Arc::new(bridge)),
                Err(e) => {
                    tracing::error!("Agent 启动失败，/api/chat* 将返回 503：{e:#}");
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("未找到 robit 配置（{e}）：/api/chat* 返回 503，静态托管照常");
            None
        }
    };

    // 调度与预警引擎（P4-T7，03 §12）：交易日盘中分钟级轮询（行情刷新 → 预警
    // 判定 → alerts 落库）+ 15:05 盘后链（equity_snapshots 快照 + /复盘 注入
    // agent 会话，push_log 留痕）。复盘回调接 WebBridge；agent 离线传 None——
    // 快照照写，push_log 如实记未触发。ZIMU_SCHEDULER_DISABLE=1 整体停用
    // （开发/测试用；手动 refresh 与 agent 工具不受影响）。
    // 调度器句柄留给 shutdown_signal：Ctrl+C 时 abort（后台任务先停，再排空 HTTP）
    let scheduler_handle: Option<JoinHandle<()>> =
        if std::env::var("ZIMU_SCHEDULER_DISABLE").is_err() {
            use std::future::Future;
            use std::pin::Pin;
            use zimu_agent::scheduler::{ReviewTrigger, POLL_INTERVAL};

            let review: Option<ReviewTrigger> = agent.as_ref().map(|bridge| {
                let bridge = bridge.clone();
                Arc::new(move |text: String| -> Pin<Box<dyn Future<Output = bool> + Send>> {
                    let bridge = bridge.clone();
                    Box::pin(async move { bridge.web.send_user_text(text).await.is_ok() })
                }) as ReviewTrigger
            });
            let core = zimu_agent::scheduler::SchedulerCore::new(
                // 独立 AgentDeps（自己的 CalendarCache）：调度与工具面/HTTP 侧解耦，
                // 多一个 1h TTL 缓存实例无害（与 agent_bridge 同款取舍）
                zimu_agent::AgentDeps::new(store.clone(), market_client.clone()),
                review,
            );
            Some(tokio::spawn(zimu_agent::scheduler::run(
                core,
                POLL_INTERVAL,
            )))
        } else {
            tracing::warn!("ZIMU_SCHEDULER_DISABLE 已设：预警调度器停用（03 §12 调度口径）");
            None
        };

    // SSE 优雅关闭通道：Ctrl+C 后 shutdown_signal 翻 true，chat_stream 的 take_until 收尾
    let (sse_shutdown_tx, sse_shutdown_rx) = watch::channel(false);

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/chat/stream", get(chat_stream))
        .route("/api/chat", post(chat))
        .route("/api/chat/confirm", post(chat_confirm))
        .route("/api/chat/cancel", post(chat_cancel))
        .with_state(AppState {
            static_dir: static_dir.clone(),
            agent,
            market_url,
            shutdown_rx: sse_shutdown_rx,
        })
        // 业务路由（持仓 P4-T1 + 行情 P4-T4）在库目标装配，各自带独立状态
        .merge(api_router(store, market))
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

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(scheduler_handle, sse_shutdown_tx))
        .await
        .context("服务异常退出")?;

    // serve 返回 = 监听已关、在途请求已排空、SSE 已断开；AppState 随 Router 释放
    // → WebBridge drop → Agent 消息通道关闭、运行循环自然退出；SQLite 连接随
    // Arc 释放关闭。运行时退出会中止残余任务，这里无需逐个点名。
    tracing::info!("子木服务已停止，再见");
    Ok(())
}

#[cfg(test)]
mod tests {
    //! health/chat 实现在 bin、契约 doc-stub 在 lib（openapi.rs）——这里断言
    //! 两边没有漂移：bin 构造的 `Health` 序列化键集 == OpenAPI schema 键集。
    //!（chat 请求体直接消费 lib 类型，天然同源无需断言。）

    #[test]
    fn health_shape_matches_openapi_schema() {
        let sample = zimu_server::openapi::Health {
            ok: true,
            service: "zimu-server".into(),
            version: "0.0.1".into(),
            static_dir: "frontend/dist".into(),
            static_ready: true,
            agent_online: false,
            market_url: "http://127.0.0.1:8300".into(),
        };
        let value = serde_json::to_value(&sample).expect("Health 可序列化");
        let doc: serde_json::Value =
            serde_json::from_str(&zimu_server::openapi::openapi_json()).expect("契约文档可解析");
        let schema = doc
            .pointer("/components/schemas/Health")
            .expect("契约里必须有 Health schema");
        let mut schema_keys: Vec<&str> = schema["properties"]
            .as_object()
            .expect("Health schema 是对象")
            .keys()
            .map(|s| s.as_str())
            .collect();
        let mut actual_keys: Vec<&str> = value
            .as_object()
            .expect("Health 序列化为对象")
            .keys()
            .map(|s| s.as_str())
            .collect();
        schema_keys.sort_unstable();
        actual_keys.sort_unstable();
        assert_eq!(
            schema_keys, actual_keys,
            "main.rs 的 Health 构造与 openapi.rs 契约 schema 键集不一致"
        );
    }
}
