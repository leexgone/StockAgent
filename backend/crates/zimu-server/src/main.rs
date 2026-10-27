//! 子木 · zimu-server —— 最简运行框架骨架。
//!
//! 当前能力（刻意最小，验证「启动服务 → 浏览器进首页」这条链路）：
//! - `GET /api/health`：健康检查，返回服务名/版本/静态目录状态
//! - 其余路径：静态托管 `frontend/dist`，找不到文件回落 `index.html`（SPA 路由）
//!
//! 后续（见 docs/06-roadmap.md）：
//! - P2-T2：接入 robit agent（阿里云网关 qwen）+ 领域工具
//! - P2-T3：SSE 事件流转出
//! - 部署约束：仅内网监听，默认不开公网入站（ADR-0005）

use std::path::PathBuf;

use anyhow::Context;
use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    static_dir: PathBuf,
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    service: &'static str,
    version: &'static str,
    static_dir: PathBuf,
    /// frontend/dist/index.html 是否存在（false = 前端还没构建）
    static_ready: bool,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        ok: true,
        service: "zimu-server",
        version: env!("CARGO_PKG_VERSION"),
        static_dir: state.static_dir.clone(),
        static_ready: state.static_dir.join("index.html").exists(),
    })
}

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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let static_dir = resolve_static_dir();
    if !static_dir.join("index.html").exists() {
        tracing::warn!(
            "未找到前端构建产物 {}：先在 frontend/ 执行 `npm install && npm run build`；此前仅 /api/health 可用",
            static_dir.display()
        );
    }

    let app = Router::new()
        .route("/api/health", get(health))
        .with_state(AppState { static_dir: static_dir.clone() })
        // SPA 回落：dist 里不存在的路径返回 index.html。
        // 注意必须用 .fallback() 而不是 .not_found_service()：后者会把状态码强制成 404
        //（tower-http 0.6 的设计，body 照常返回），客户端路由刷新就会拿到 404。
        .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(static_dir.join("index.html"))))
        .layer(TraceLayer::new_for_http());

    // 仅内网/本机监听；LAN 部署改 ZIMU_ADDR，公网入站默认禁止（ADR-0005 第 3 条）
    let addr = std::env::var("ZIMU_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("绑定 {addr} 失败"))?;
    tracing::info!("子木服务已监听 http://{addr}（静态目录：{}）", static_dir.display());

    axum::serve(listener, app).await.context("服务异常退出")?;
    Ok(())
}
