//! OpenAPI 3.1 契约（P4-T5）：`docs/api/openapi.json` 的唯一汇编点。
//!
//! **前后端类型同源**链条：serde 结构体在各定义处 `#[derive(ToSchema)]` →
//! `ApiDoc`（本文件）汇编 → `docs/api/openapi.json`（契约测试断言仓库文件与
//! 代码生成逐字节一致）→ 前端 `npm run gen:api` 生成 `src/api/schema.d.ts`
//! （P5-T1 的 API client 直接消费该类型面）。改契约 = 改结构体，文档跟着走。
//!
//! 路径来源两类：
//! - `holdings` / `market`：`#[utoipa::path]` 直接标注在 lib 的 handler 上，
//!   文档与实现同一函数，不会漂；
//! - `system`（health / chat）：实现在 bin（`main.rs`，依赖 robit 装配与静态
//!   目录状态），这里以 doc-stub 声明契约——`Health`/`ChatUploadForm`/
//!   `ConfirmRequest` 类型定义在本模块供 bin 消费，bin 侧单测断言序列化键集
//!   与 schema 键集一致，漂移没有盲区。`POST /api/chat` 自 ADR-0015 起为
//!   multipart/form-data（`chat_upload` 接收），请求体不在 contract JSON 响应校验面。
//!
//! 验收（P4-T5「样例响应过 schema 校验」）：`tests/contract.rs` 用
//! `fixtures/api/` 样例 +  seeded 测试服务的实时响应逐端点过 jsonschema 校验。

use std::sync::OnceLock;

use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{OpenApi, ToSchema};

// schemas() 宏只接受裸标识符/完整路径，不认 `{…}` 分组——统一 import 后引用。
// （这些 import 全部被 ApiDoc derive 的展开消费，不是死代码。）
use crate::channel_bindings::{BindingRequest, UnbindResult};
use crate::chat_api::{ChatHistory, ChatResetResult};
use crate::holdings_api::{AccountPatch, HoldingEditRequest};
use crate::market_api::{
    CalendarStatus, KlineResponse, MarketStatusResponse, RefreshResponse, SidecarStatus, SkippedRow,
};
use zimu_domain::dilution::TradeOutcome;
use zimu_domain::fees::FeeBreakdown;
use zimu_domain::recalc::{Agg, PerHolding, SectorAgg};
use zimu_domain::{Kind, Side};
use zimu_market::clock::MarketClock;
use zimu_market::models::{
    Attempt, CalendarPayload, CbItem, CbListPayload, KlineBar, KlinePayload, QuotePayload, QuoteRow,
    QuoteStatus,
};
use zimu_store::{
    AccountRow, AdviceRow, AlertRow, ChatMessageRow, ChannelBindingRow, EditOp, EquitySnapshotRow,
    HoldingEditRow, HoldingRow,
    HoldingSnapshot, NewHoldingMeta, NewWatchlist, PortfolioView, PushLogRow, RecordReceipt,
    SimPreview, TradeRequest, TradeRow, WatchlistRow,
};

// ============================================================================
// 共享契约类型
// ============================================================================

/// 统一错误体：`error` 机器可读码、`message` 中文可直接展示。
/// 特定拒绝带附加数值字段（前端就地高亮用，其余场景不出现）：
/// 涨跌停越界 `limit_down/limit_up/decimals`（显示与拦截同一对限价，03 §2）、
/// 现金不足 `cash_after`。
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    /// 错误码：bad_input / no_holding / need_meta / out_of_limit_range /
    /// insufficient_cash / store_error / bad_params / market_unavailable /
    /// market_contract，或 sidecar 上游错误契约的原样透传（如
    /// UPSTREAM_ALL_FAILED / BAD_SYMBOL）
    pub error: String,
    /// 中文说明，可直接展示给用户
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_down: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_up: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cash_after: Option<f64>,
}

impl ApiError {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        ApiError {
            error: error.into(),
            message: message.into(),
            limit_down: None,
            limit_up: None,
            decimals: None,
            cash_after: None,
        }
    }
}

/// `GET /api/health` 响应。实现在 bin（`main.rs`）；类型定义在这里进契约，
/// bin 侧单测断言两边键集一致。
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    pub ok: bool,
    /// 恒 "zimu-server"
    pub service: String,
    /// crate 版本
    pub version: String,
    /// 静态目录（解析后的绝对路径）
    pub static_dir: String,
    /// frontend/dist/index.html 是否存在（false = 前端还没构建，仅 /api/* 可用）
    pub static_ready: bool,
    /// Agent 是否在线（false = 缺 robit 配置或启动失败，/api/chat* 降级 503）
    pub agent_online: bool,
    /// QQ 通道在线状态（T5）：false = 未启用（[zimu.channels].qq 缺省停用）/
    /// 未连上/已退出——health 如实透出，不装在线
    pub qq_online: bool,
    /// 行情 sidecar 基地址（是否在线由 /api/market/status 探测，health 不打网络）
    pub market_url: String,
}

/// `POST /api/chat` 请求体（multipart/form-data，ADR-0015；接收在 lib `chat_upload`，
/// bin 接线）。`text` 可为空（仅发附件时）；`files` 落盘 media/ 后注入 Agent——
/// 图像走 vision、非图像标注本地路径由标准工具 read/bash 自动解析（ADR-0014）。
#[derive(Debug, ToSchema)]
pub struct ChatUploadForm {
    /// 用户消息文本（可为空——仅发附件时；回复经 SSE 流出）
    pub text: String,
    /// 上传文件（多选，二进制；单文件 ≤ max_media_bytes 缺省 20MB，超限 413）
    #[schema(format = Binary)]
    pub files: Vec<String>,
}

/// `POST /api/chat/confirm` 请求体（实现在 bin）
#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfirmRequest {
    pub tool_call_id: String,
    pub approved: bool,
}

// ============================================================================
// system 端点的 doc-stub（实现在 bin，契约声明在 lib）
// ============================================================================

#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/api/health",
    operation_id = "health",
    tag = "system",
    responses(
        (status = 200, description = "服务健康。不打网络探测 sidecar——行情在线状态看 /api/market/status", body = Health),
    )
)]
fn health_doc() {}

#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/api/chat/stream",
    operation_id = "chat_stream",
    tag = "system",
    responses(
        (status = 200, content_type = "text/event-stream",
         description = "SSE 事件流：首帧 connected（宿主元数据：profile/model/tools），随后为 robit-web WebEvent（text_delta / thinking_delta / tool_call / tool_result / skill_triggered / async_tool_completed / confirm_request / turn_complete / error），wire schema 由 robit-web 定义（v0.2.1，wire 与 v0.1.29 逐字节一致；thinking_delta 为思考型模型的展示性增量帧，无显式结束标记，turn_complete 收口）"),
        (status = 503, description = "Agent 未启动（缺 robit 配置）"),
    )
)]
fn chat_stream_doc() {}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/api/chat",
    operation_id = "chat_send",
    tag = "system",
    request_body(content = ChatUploadForm, content_type = "multipart/form-data",
        description = "multipart 表单：text 字段（可空）+ files 文件字段（多选，二进制）。落盘 media/ 后注入 Agent（ADR-0015）"),
    responses(
        (status = 202, description = "已接受，回复经 SSE 流出"),
        (status = 400, description = "消息为空（无文本且无附件）/ 附件超数量 / 空文件 / 表单解析失败（text/plain）"),
        (status = 413, description = "文件超过大小上限 max_media_bytes（text/plain）"),
        (status = 503, description = "Agent 未启动或会话受理失败（text/plain）"),
    )
)]
fn chat_doc() {}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/api/chat/confirm",
    operation_id = "chat_confirm",
    tag = "system",
    request_body = ConfirmRequest,
    responses(
        (status = 200, description = "确认已回填"),
        (status = 404, description = "没有匹配的待确认工具调用（text/plain）"),
        (status = 503, description = "Agent 未启动（text/plain）"),
    )
)]
fn chat_confirm_doc() {}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/api/chat/cancel",
    operation_id = "chat_cancel",
    tag = "system",
    responses(
        (status = 202, description = "已请求取消当前轮次"),
        (status = 503, description = "Agent 未启动（text/plain）"),
    )
)]
fn chat_cancel_doc() {}

// ============================================================================
// ApiDoc 汇编
// ============================================================================

#[derive(OpenApi)]
#[openapi(
    info(
        title = "子木 · 股票智能助理 API",
        version = env!("CARGO_PKG_VERSION"),
        description = "子木常驻 Web 服务的 HTTP API（ADR-0005）。数字口径唯一来源是 docs/03（zimu-domain 实现、黄金向量锁死），本服务不另定义口径。本文档由 serde 结构体经 utoipa 生成，契约测试保证实时与样例响应过 schema 校验（cargo test -p zimu-server --test contract）；前端 TS 类型自本文档生成（frontend: npm run gen:api）。降级边界（04 附录 B）：行情 sidecar 缺席时 /api/market/* 返回 503 market_unavailable，持仓/录入/会话/静态托管照常。",
    ),
    servers(
        (url = "/", description = "本机部署（默认 http://127.0.0.1:8080，ZIMU_ADDR 覆盖；仅内网监听，ADR-0005）"),
    ),
    tags(
        (name = "system", description = "健康检查、契约文档与 Agent SSE 会话"),
        (name = "holdings", description = "持仓与录入（P4-T1，ADR-0006 手工逐笔先行；校验与派生全部走 zimu-domain）+ 直接维护（ADR-0009 快照校正，03 §13：整行覆盖 upsert / 删除 / holding_edits 留痕——不动现金、不写 trades）"),
        (name = "market", description = "行情（P4-T4，经 zimu-market 适配器——sidecar 唯一消费入口；错误三分类见 04 附录 B）"),
        (name = "alerts", description = "预警、净值快照与推送留痕（P4-T7 + P5-T2）：内容由调度引擎判定落库（口径 03 §12，判定函数在 zimu-domain::alert），端点只读取与归档"),
        (name = "advice", description = "建议留档（P5-T2 读端点；写入在 agent 工具链，ADR-0007——数字归规则、文字归模型）"),
        (name = "watchlist", description = "自选（P5-T2，B13 定稿）：观察标的静态属性 CRUD；03 §12 预警判定范围扩到自选（无持仓形态），行情不回写本表"),
    ),
    paths(
        health_doc, chat_stream_doc, chat_doc, chat_confirm_doc, chat_cancel_doc,
        crate::chat_api::get_history,
        crate::chat_api::reset_session,
        crate::channel_bindings::list_bindings,
        crate::channel_bindings::bind,
        crate::channel_bindings::unbind,
        openapi_doc,
        crate::holdings_api::get_holdings,
        crate::holdings_api::record_trade,
        crate::holdings_api::upsert_holding,
        crate::holdings_api::delete_holding,
        crate::holdings_api::get_holding_edits,
        crate::holdings_api::get_trades,
        crate::holdings_api::update_account,
        crate::market_api::status,
        crate::market_api::quote,
        crate::market_api::kline,
        crate::market_api::cb_list,
        crate::market_api::calendar,
        crate::market_api::refresh,
        crate::alerts_api::get_alerts,
        crate::alerts_api::mark_alert_done,
        crate::alerts_api::get_equity_snapshots,
        crate::alerts_api::get_push_logs,
        crate::advice_api::get_advice,
        crate::watchlist_api::get_watchlist,
        crate::watchlist_api::add_watchlist,
        crate::watchlist_api::remove_watchlist,
    ),
    components(schemas(
        ApiError, Health, ChatUploadForm, ConfirmRequest,
        ChatHistory, ChatResetResult, ChatMessageRow,
        BindingRequest, ChannelBindingRow, UnbindResult,
        AccountPatch,
        CalendarStatus, KlineResponse, MarketStatusResponse, RefreshResponse,
        SidecarStatus, SkippedRow,
        Kind, Side,
        TradeOutcome, FeeBreakdown,
        Agg, PerHolding, SectorAgg,
        AccountRow, HoldingRow, NewHoldingMeta, PortfolioView, RecordReceipt,
        SimPreview, TradeRequest, TradeRow,
        HoldingEditRequest, HoldingEditRow, HoldingSnapshot, EditOp,
        AlertRow, EquitySnapshotRow, AdviceRow, PushLogRow,
        WatchlistRow, NewWatchlist,
        MarketClock,
        Attempt, CalendarPayload, CbItem, CbListPayload, KlineBar, KlinePayload,
        QuotePayload, QuoteRow, QuoteStatus,
    ))
)]
pub struct ApiDoc;

/// OpenAPI 文档的稳定序列化（pretty JSON + 末尾换行）。
/// `docs/api/openapi.json` 即此输出，契约测试断言逐字节一致——文档永不手改。
pub fn openapi_json() -> String {
    let mut buf =
        serde_json::to_vec_pretty(&ApiDoc::openapi()).expect("OpenAPI 文档序列化不应失败");
    buf.push(b'\n');
    String::from_utf8(buf).expect("OpenAPI 文档是合法 UTF-8")
}

/// `GET /api/openapi.json` —— 运行时提供本契约（与仓库内 docs/api/openapi.json
/// 同源同内容；前端离线生成类型读仓库文件，在线工具读这里）。
#[utoipa::path(
    get,
    path = "/api/openapi.json",
    tag = "system",
    responses(
        (status = 200, content_type = "application/json", description = "本契约文档（OpenAPI 3.1，由代码生成）"),
    )
)]
pub async fn openapi_doc() -> Json<&'static Value> {
    static DOC: OnceLock<Value> = OnceLock::new();
    Json(DOC.get_or_init(|| {
        serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI 文档序列化不应失败")
    }))
}

#[cfg(test)]
mod tests {
    /// 契约文档再生成入口：改结构体/标注后跑
    /// `cargo test -p zimu-server --lib -- --ignored regenerate_openapi_json`，
    /// 由 `tests/contract.rs` 的 up-to-date 断言守住「忘了再生成」。
    #[test]
    #[ignore = "写仓库文件：再生成 docs/api/openapi.json"]
    fn regenerate_openapi_json() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../docs/api/openapi.json");
        std::fs::create_dir_all(path.parent().expect("docs/api 有父目录")).expect("建目录");
        std::fs::write(&path, super::openapi_json()).expect("写 openapi.json");
    }
}
