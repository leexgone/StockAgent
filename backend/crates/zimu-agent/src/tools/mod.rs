//! 领域工具集（P4-T6）：agent 的账本/行情取数与出数入口。
//!
//! 工具面组成（ADR-0014 起 = 领域 13 件 + robit 标准面，注册 24–30 随配置；
//! ADR-0024 起纪律不进智能体，calc_discipline 退役）：
//! 本模块只管**领域 13 件**（roadmap P4-T6 三件 + 打分/留档两件 + 状态/刷新
//! 两件 + 写账本五件（ADR-0012/0013）+ 诊断一件（P7-T10））；
//! robit 标准面（read/write/edit/ls/find/grep/bash/load_skill/search_history/
//! query_task 恒在，generate_image 随 image provider key、read_image 随 vision
//! 配置按需）由装配线经 `create_tools_from_config_with` 内核取得后叠加本模块
//! （assembly.rs，ADR-0014）——确认关闭自动执行（auto_approve=true）。
//!
//! 领域工具表：
//!
//! | 工具 | 性质 | 口径来源 |
//! |---|---|---|
//! | `market_status` | 只读 | `zimu-market::clock`（03 §9 生产口径） |
//! | `get_quote` | 只读 | sidecar 实时行 + `zimu-domain::price_limit`（03 §2） |
//! | `get_holdings` | 只读 | `store.portfolio()` → `zimu-domain::recalc`（03 §5） |
//! | `simulate_trade` | 只读（试算不落库） | `store.simulate_trade` → fees/dilution/lot（03 §1/§3/§4） |
//! | `record_trade` | **写账本**（trades+持仓+现金，与界面录入同一事务） | `store.record_trade`（03 §1–§4，ADR-0012） |
//! | `adjust_cash` | **写账本**（现金绝对值覆盖，不写流水） | `store.update_account`（ADR-0009/0012） |
//! | `get_trades` | 只读（流水账本，最新在前） | `store.trades` |
//! | `upsert_holding` | **写账本**（快照校正：整行覆盖 + holding_edits 留痕，三不动） | `store.upsert_holding`（03 §13，ADR-0013） |
//! | `remove_holding` | **写账本**（删持仓行：流水保留、已实现不回滚、现金不动） | `store.remove_holding`（03 §13，ADR-0013） |
//! | `refresh_quotes` | 写行情字段 + 补空串行业 | `service::refresh_portfolio`（与 HTTP API 同一条链；行业补全 P7-T6/ADR-0016） |
//! | `calc_advice` | 写 advice_history 数字列 | `zimu-domain::advice`（03 §10 生产口径，ADR-0007） |
//! | `finalize_advice` | 写 advice_history 文案列 | risks 非空校验（03 §10） |
//! | `calc_diagnosis` | 只读 | `store.equity_snapshots/list_advice` → `zimu-domain::bench+perf`（03 §7，P7-T10） |
//!
//! **模型面文案约定（2026-11-28 清理定稿）**：`description()`、参数 schema 的
//! description、返回 JSON 的 note/reason 都是模型直接读的文案——只写「何时调用、
//! 参数契约、返回字段语义、行为纪律、降级如实规则」；上表「口径来源」列的内部编号
//! （docs/03 §、ADR、P/B 系列）、表名、HTTP 端点与实现链描述**不进模型面文案**，
//! 留在本注释与表格作工程追溯（模型看不见 docs/03，引用只会烧 token 并可能被复读进
//! 用户文案）。超长文案一律 `concat!` 按语义分段，避免单行超长。
//!
//! **写账本三纪律（ADR-0012，自动落账 + 读回）**：① 记账数字必须来自用户明示，
//! 缺项先问不猜（禁止用行情价/记忆补数）；② 先 simulate_trade 试算复述、再
//! record_trade 落账、落账后宣读完整收据；③ 流水只增不改，错账反向一笔冲正
//! （note 互相引用）。系统不对接任何交易系统——「永不代下单」是表述纪律（不得
//! 让用户误以为能真实执行），不是工具面限制。门禁两形态并存（ADR-0014）：
//! 写账本五件**软门禁**（自动落账 + 读回 + 人格纪律，ADR-0012/0013 不变）；
//! robit 标准面 write/edit/bash/generate_image 工具层 requires_confirmation=true，
//! 但 ADR-0014 拍板 auto_approve=true **确认关闭自动执行**（重开 = config 翻
//! false + 先补 docs/07 C13 两洞）。快照校正（upsert/remove_holding）
//! 自 ADR-0013 起同样进工具面（初始化映射 / 定期校准 / 场外变动）：整行覆盖
//! 先读后改、删除须对话内两步确认（复述整行 → 用户明确同意，人格纪律）。
//!
//! 全部工具返回 JSON 文本（`ToolResult::success`），失败返回 `ToolResult::error`
//! 并把降级原因如实写给模型（sidecar 不可达/数据缺失），由模型转述给用户——
//! 错误文案面向对话，与 HTTP 契约（`ApiError`）各自表达同一事实。

mod advice;
mod diagnosis;
mod market;
mod portfolio;

use std::sync::Arc;

use robit_agent::{ToolRegistry, ToolResult};
use serde::Serialize;
use zimu_market::error::MarketError;

use crate::AgentDeps;

pub use advice::{CalcAdviceTool, FinalizeAdviceTool};
pub use diagnosis::CalcDiagnosisTool;
pub use market::{GetQuoteTool, MarketStatusTool, RefreshQuotesTool};
pub use portfolio::{
    AdjustCashTool, GetHoldingsTool, GetTradesTool, RecordTradeTool, RemoveHoldingTool,
    SimulateTradeTool, UpsertHoldingTool,
};

/// 存储调用早退助手：anyhow 错误 → 脱敏 `ToolResult::error`。
/// R5 决策记录（robit 0.1.27 起已有 `AgentError::External(#[from] anyhow::Error)`
/// 与 `From<anyhow::Error> for ToolResult`，子木仍走本宏）：官方 From 会把整条
/// anyhow cause 链（可能含 SQL 原文/路径）喂给模型；子木的存储错误策略是
/// **细节进 tracing 日志、模型只见固定话术**——正是 robit R5 文档认可的第二种
/// 推荐形态（sanitize what the model sees），见 docs/07-backlog.md R5。
macro_rules! store_try {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(err) => return Ok($crate::tools::store_err(err)),
        }
    };
}
pub(crate) use store_try;

/// 领域工具名清单（tools_required 校验名单用，与下表逐件对应）。
/// ADR-0024：calc_discipline 退役——纪律不进智能体可见面（Web 端测算展示独有）。
pub const DOMAIN_TOOL_NAMES: [&str; 13] = [
    "market_status", "get_quote", "get_holdings", "simulate_trade",
    "record_trade", "adjust_cash", "get_trades", "upsert_holding", "remove_holding",
    "refresh_quotes", "calc_advice", "finalize_advice", "calc_diagnosis",
];

/// 把全部领域工具（13 件）注册进给定 registry——装配线先经
/// `create_tools_from_config_with` 取 robit 标准面，再调本函数叠加
/// （ADR-0014；zimu-server 与 zimu-agent-cli 共用同一装配线）。
/// 领域名与标准名零冲突（已逐件核对）；同名注册语义为覆盖（robit registry）。
pub fn register_domain_tools(reg: &mut ToolRegistry, deps: Arc<AgentDeps>) {
    reg.register(MarketStatusTool { deps: deps.clone() });
    reg.register(GetQuoteTool { deps: deps.clone() });
    reg.register(GetHoldingsTool { deps: deps.clone() });
    reg.register(SimulateTradeTool { deps: deps.clone() });
    reg.register(RecordTradeTool { deps: deps.clone() });
    reg.register(AdjustCashTool { deps: deps.clone() });
    reg.register(GetTradesTool { deps: deps.clone() });
    reg.register(UpsertHoldingTool { deps: deps.clone() });
    reg.register(RemoveHoldingTool { deps: deps.clone() });
    reg.register(RefreshQuotesTool { deps: deps.clone() });
    reg.register(CalcAdviceTool { deps: deps.clone() });
    reg.register(FinalizeAdviceTool { deps: deps.clone() });
    reg.register(CalcDiagnosisTool { deps });
}

/// 成功返回：JSON pretty 文本（模型可读，键序稳定便于人工核对留档）
pub(crate) fn ok_json(v: &impl Serialize) -> ToolResult {
    match serde_json::to_string_pretty(v) {
        Ok(s) => ToolResult::success(s),
        Err(e) => ToolResult::error(format!("工具输出序列化失败（内部错误）：{e}")),
    }
}

/// 行情三分类 → 面向对话的错误文本（语义与 HTTP `err_response` 一致：
/// Unavailable = sidecar 挂了降级、Upstream = 上游源错误透传、Contract = 版本漂移）
pub(crate) fn market_err(e: MarketError) -> ToolResult {
    match e {
        MarketError::Unavailable(m) => ToolResult::error(format!(
            "行情服务不可达，行情类数字暂不可用：{m}。持仓/留档查询不受影响；请稍后重试，或让用户检查行情服务是否在线。"
        )),
        MarketError::Upstream {
            status,
            code,
            message,
        } => ToolResult::error(format!("行情上游错误（HTTP {status}，{code}）：{message}")),
        MarketError::Contract(m) => ToolResult::error(format!(
            "行情数据契约不符（行情服务与本程序版本不一致，已记录内部错误）：{m}"
        )),
    }
}

/// 存储层错误统一话术（细节进日志，不把 SQL 原文喂给模型）
pub(crate) fn store_err(e: impl std::fmt::Display) -> ToolResult {
    tracing::error!("存储层错误：{e}");
    ToolResult::error("存储层内部错误（已记录日志），本次操作未完成".to_string())
}
