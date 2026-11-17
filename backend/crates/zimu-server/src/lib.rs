//! 子木 · zimu-server 库目标：`/api/*` 业务路由装配。
//!
//! bin（`main.rs`）只负责启动接线——robit Agent 装配、SSE 会话端点、静态托管、
//! 日志与监听；业务路由（持仓 + 行情）在这里组装，集成测试用 `api_router()`
//! + 临时端口即可拉起完整 API 面（P4-T4 起）。
//!
//! 分层边界（04 附录 B）：
//! - `holdings_api` —— 存储读写 + `zimu-domain` 校验/试算/重算的 HTTP 接线；
//! - `market_api` —— `zimu-market` 适配器（sidecar 唯一消费入口）的 HTTP 接线；
//! - `alerts_api` —— 预警/净值快照/推送留痕的读取与归档（P4-T7 + P5-T2 补
//!   push_logs 读端点；内容由调度引擎判定落库，判定口径在 `zimu-domain::alert`，
//!   端点不生成不改写）；
//! - `advice_api` —— 建议留档读取（P5-T2；写入在 agent 工具链，ADR-0007）；
//! - `watchlist_api` —— 自选 CRUD（P5-T2，B13 定稿：schema v2 watchlist 表，
//!   03 §12 预警范围扩到自选；行情不回写自选表）。
//!   各层都不定义数字口径：口径只在 `zimu-domain`（03 唯一来源）。
//! - `openapi` —— API 契约汇编（P4-T5）：全部响应/请求 serde 结构体 →
//!   OpenAPI 3.1（`docs/api/openapi.json` 由测试保证与代码一致），前端 TS
//!   类型自该文档生成（前后端类型同源）。

pub mod advice_api;
pub mod alerts_api;
pub mod channel_adapter;
pub mod channel_bindings;
pub mod channel_push;
pub mod chat_api;
pub mod chat_record;
pub mod chat_upload;
pub mod config_flags;
pub mod holdings_api;
pub mod market_api;
pub mod openapi;
pub mod pending_bindings;
pub mod watchlist_api;

#[cfg(test)]
mod testutil;

use axum::routing::{delete, get, post, put};
use axum::Router;

/// 持仓 API 路由（P4-T1 交易录入 + ADR-0009 直接维护）
pub fn holdings_router(store: holdings_api::SharedStore) -> Router {
    Router::new()
        .route("/api/holdings", get(holdings_api::get_holdings))
        .route("/api/holdings/trade", post(holdings_api::record_trade))
        // 直接维护（03 §13 快照校正）：upsert + 删除；留痕读取供「维护记录」卡
        .route(
            "/api/holdings/{code}",
            put(holdings_api::upsert_holding).delete(holdings_api::delete_holding),
        )
        .route("/api/holding_edits", get(holdings_api::get_holding_edits))
        .route("/api/trades", get(holdings_api::get_trades))
        .route("/api/account", post(holdings_api::update_account))
        .with_state(store)
}

/// 预警、净值快照与推送留痕路由（P4-T7 + P5-T2）：调度引擎落库，端点只读取与归档
pub fn alerts_router(store: holdings_api::SharedStore) -> Router {
    Router::new()
        .route("/api/alerts", get(alerts_api::get_alerts))
        .route("/api/alerts/{id}/done", post(alerts_api::mark_alert_done))
        .route(
            "/api/equity_snapshots",
            get(alerts_api::get_equity_snapshots),
        )
        .route("/api/push_logs", get(alerts_api::get_push_logs))
        .with_state(store)
}

/// 建议留档路由（P5-T2）：agent 工具链写入（ADR-0007），端点只读取
pub fn advice_router(store: holdings_api::SharedStore) -> Router {
    Router::new()
        .route("/api/advice", get(advice_api::get_advice))
        .with_state(store)
}

/// 自选路由（P5-T2，B13 定稿）：CRUD；预警判定范围扩展在调度器（§12）
pub fn watchlist_router(store: holdings_api::SharedStore) -> Router {
    Router::new()
        .route("/api/watchlist", get(watchlist_api::get_watchlist))
        .route("/api/watchlist", post(watchlist_api::add_watchlist))
        .route(
            "/api/watchlist/{code}",
            delete(watchlist_api::remove_watchlist),
        )
        .with_state(store)
}

/// 全部 `/api/*` 业务路由（持仓 + 行情 + 预警 + 会话档案 + 契约文档）。不含
/// `/api/chat`（发送）与 `/api/chat/stream|confirm|cancel`——那些依赖 robit 装配
/// 留在 bin 侧；`/api/chat/history|reset` 只依赖 store + ResetHook，在这里装配
/// （T3：档案是服务端真值，agent 离线也能读历史）。
pub fn api_router(
    store: holdings_api::SharedStore,
    market: market_api::SharedMarket,
    chat_reset: chat_api::ResetHook,
    pending: pending_bindings::PendingBindings,
) -> Router {
    Router::new()
        .route("/api/openapi.json", get(openapi::openapi_doc))
        .merge(holdings_router(store.clone()))
        .merge(alerts_router(store.clone()))
        .merge(advice_router(store.clone()))
        .merge(watchlist_router(store.clone()))
        .merge(chat_api::router(store.clone(), chat_reset))
        .merge(channel_bindings::router(store.clone(), pending))
        .merge(market_api::router(store, market))
}
