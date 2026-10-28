//! 子木 · zimu-server 库目标：`/api/*` 业务路由装配。
//!
//! bin（`main.rs`）只负责启动接线——robit Agent 装配、SSE 会话端点、静态托管、
//! 日志与监听；业务路由（持仓 + 行情）在这里组装，集成测试用 `api_router()`
//! + 临时端口即可拉起完整 API 面（P4-T4 起）。
//!
//! 分层边界（04 附录 B）：
//! - `holdings_api` —— 存储读写 + `zimu-domain` 校验/试算/重算的 HTTP 接线；
//! - `market_api` —— `zimu-market` 适配器（sidecar 唯一消费入口）的 HTTP 接线。
//!   两层都不定义数字口径：口径只在 `zimu-domain`（03 唯一来源）。

pub mod holdings_api;
pub mod market_api;

use axum::routing::{get, post};
use axum::Router;

/// 持仓 API 路由（P4-T1）
pub fn holdings_router(store: holdings_api::SharedStore) -> Router {
    Router::new()
        .route("/api/holdings", get(holdings_api::get_holdings))
        .route("/api/holdings/trade", post(holdings_api::record_trade))
        .route("/api/trades", get(holdings_api::get_trades))
        .route("/api/account", post(holdings_api::update_account))
        .with_state(store)
}

/// 全部 `/api/*` 业务路由（持仓 + 行情）。不含 `/api/chat*` 与静态托管——
/// 它们依赖 robit 装配与前端构建产物，留在 bin 侧。
pub fn api_router(
    store: holdings_api::SharedStore,
    market: market_api::SharedMarket,
) -> Router {
    holdings_router(store.clone()).merge(market_api::router(store, market))
}
