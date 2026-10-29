//! 子木 · zimu-server 库目标：`/api/*` 业务路由装配。
//!
//! bin（`main.rs`）只负责启动接线——robit Agent 装配、SSE 会话端点、静态托管、
//! 日志与监听；业务路由（持仓 + 行情）在这里组装，集成测试用 `api_router()`
//! + 临时端口即可拉起完整 API 面（P4-T4 起）。
//!
//! 分层边界（04 附录 B）：
//! - `holdings_api` —— 存储读写 + `zimu-domain` 校验/试算/重算的 HTTP 接线；
//! - `market_api` —— `zimu-market` 适配器（sidecar 唯一消费入口）的 HTTP 接线；
//! - `alerts_api` —— 预警/净值快照的读取与归档（P4-T7；内容由调度引擎判定落库，
//!   判定口径在 `zimu-domain::alert`，端点不生成不改写）。
//!   各层都不定义数字口径：口径只在 `zimu-domain`（03 唯一来源）。
//! - `openapi` —— API 契约汇编（P4-T5）：全部响应/请求 serde 结构体 →
//!   OpenAPI 3.1（`docs/api/openapi.json` 由测试保证与代码一致），前端 TS
//!   类型自该文档生成（前后端类型同源）。

pub mod alerts_api;
pub mod holdings_api;
pub mod market_api;
pub mod openapi;

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

/// 预警与净值快照路由（P4-T7）：调度引擎落库，端点只读取与归档
pub fn alerts_router(store: holdings_api::SharedStore) -> Router {
    Router::new()
        .route("/api/alerts", get(alerts_api::get_alerts))
        .route("/api/alerts/{id}/done", post(alerts_api::mark_alert_done))
        .route(
            "/api/equity_snapshots",
            get(alerts_api::get_equity_snapshots),
        )
        .with_state(store)
}

/// 全部 `/api/*` 业务路由（持仓 + 行情 + 预警 + 契约文档）。不含 `/api/chat*` 与
/// 静态托管——它们依赖 robit 装配与前端构建产物，留在 bin 侧。
pub fn api_router(
    store: holdings_api::SharedStore,
    market: market_api::SharedMarket,
) -> Router {
    Router::new()
        .route("/api/openapi.json", get(openapi::openapi_doc))
        .merge(holdings_router(store.clone()))
        .merge(alerts_router(store.clone()))
        .merge(market_api::router(store, market))
}
