//! 持仓与录入 API（P4-T1：手工逐笔录录入流程，ADR-0006）。
//!
//! 端点：
//! - `GET  /api/holdings`        组合全景（存储行 + zimu-domain recalc 派生与汇总）
//! - `POST /api/holdings/trade`  记一笔买入/卖出：校验→试算→事务落库，返回收据
//! - `GET  /api/trades?limit=N`  委托/成交流水（最新在前）
//!
//! 边界：
//! - 校验口径全部在 zimu-store/zimu-domain，本模块只做 HTTP 接线与错误→状态码映射；
//!   拒绝原因带中文说明，前端可直接展示（原型抽屉的提示来自同一批规则）。
//! - rusqlite Connection 非 Sync：用 std Mutex 串行化访问。操作全是同步短事务，
//!   跨 `.await` 持锁被禁止——本模块处理器无 await，锁在语句结束即释放。

use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use zimu_store::{RecordError, Store, TradeRequest};

pub type SharedStore = Arc<Mutex<Store>>;

/// 锁中毒（上一持有者 panic）不致命：按中毒前的值继续（into_inner）。
fn lock(store: &SharedStore) -> MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// RecordError → HTTP。语义分层：输入问题 400、标的状态 404/422、
/// 存储内部错误 500（不把 SQL 细节原样吐给客户端，只给类型与日志）。
fn record_error_response(e: RecordError) -> Response {
    let (status, code, msg) = match &e {
        RecordError::BadInput(m) => (StatusCode::BAD_REQUEST, "bad_input", m.clone()),
        RecordError::NoHolding { code } => (
            StatusCode::NOT_FOUND,
            "no_holding",
            format!("持仓中没有 {code}"),
        ),
        RecordError::NeedMeta { code } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "need_meta",
            format!("{code} 尚未持有：新建持仓需要标的信息（名称/市场/类型）"),
        ),
        RecordError::OutOfLimitRange { price, lo, hi, .. } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "out_of_limit_range",
            format!("委托价 {price} 超出当日涨跌停区间 [{lo}, {hi}]"),
        ),
        RecordError::InsufficientCash { cash_after } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "insufficient_cash",
            format!("现金不足（本笔后将为 {cash_after:.2}）"),
        ),
        RecordError::Store(m) => {
            tracing::error!("存储层错误：{m}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_error",
                "存储层内部错误，已记录日志".to_string(),
            )
        }
    };
    let mut body = json!({ "error": code, "message": msg });
    // 涨跌停拒绝额外带区间数值：前端抽屉可就地高亮（显示与拦截同一对限价，03 §2）
    match e {
        RecordError::OutOfLimitRange { lo, hi, prec, .. } => {
            let o = body.as_object_mut().unwrap();
            o.insert("limit_down".into(), json!(lo));
            o.insert("limit_up".into(), json!(hi));
            o.insert("decimals".into(), json!(prec));
        }
        RecordError::InsufficientCash { cash_after } => {
            body.as_object_mut()
                .unwrap()
                .insert("cash_after".into(), json!(cash_after));
        }
        _ => {}
    }
    (status, Json(body)).into_response()
}

pub async fn get_holdings(State(store): State<SharedStore>) -> Response {
    let view = lock(&store).portfolio();
    match view {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("portfolio 读取失败：{e:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, "存储层内部错误").into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct TradesQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

pub async fn get_trades(State(store): State<SharedStore>, Query(q): Query<TradesQuery>) -> Response {
    // 上限 500 防御性截断，避免一次拉爆响应体
    let rows = lock(&store).trades(q.limit.clamp(1, 500));
    match rows {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("trades 读取失败：{e:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, "存储层内部错误").into_response()
        }
    }
}

pub async fn record_trade(
    State(store): State<SharedStore>,
    Json(req): Json<TradeRequest>,
) -> Response {
    let receipt = lock(&store).record_trade(&req);
    match receipt {
        Ok(r) => (StatusCode::CREATED, Json(r)).into_response(),
        Err(e) => record_error_response(e),
    }
}

/// 首次配置/调整账户（onboarding 只带现金；缺失字段保持原值）。
#[derive(serde::Deserialize)]
pub struct AccountPatch {
    name: Option<String>,
    init: Option<String>,
    style: Option<String>,
    broker: Option<String>,
    cash: Option<f64>,
}

pub async fn update_account(
    State(store): State<SharedStore>,
    Json(patch): Json<AccountPatch>,
) -> Response {
    let updated = lock(&store).update_account(
        patch.name.as_deref(),
        patch.init.as_deref(),
        patch.style.as_deref(),
        patch.broker.as_deref(),
        patch.cash,
    );
    match updated {
        Ok(acc) => Json(acc).into_response(),
        Err(e) => record_error_response(e),
    }
}
