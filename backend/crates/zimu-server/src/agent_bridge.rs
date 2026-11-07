//! 子木 · Web 通道接线（P2-T3 起；P2-T5 桥体下沉 robit-web；P4-T6 工具面成形；
//! T1/ADR-0011 装配段抽入 `zimu_agent::assembly::AgentCore`）。
//!
//! 本文件只剩通道私有物：`WebBridge` 构造、Agent 任务 spawn、connected 元数据。
//! LlmClient / ToolRegistry / SkillRegistry 的装配在 zimu-agent::assembly（Web 桥、
//! IM 通道、调试 REPL 共用同一条线）；`Frontend` 差异是 CliFrontend 换成 robit-web 的
//! `WebFrontend`：
//!
//! * 事件下行：AgentEvent → `WebEvent`（官方 JSON schema）→ broadcast 扇出，
//!   多浏览器连接可同时观看；SSE 编码在宿主 `main.rs`；
//! * 输入上行：`POST /api/chat` → `WebBridge::send_user_text` → `Agent::run`；
//! * 工具确认：`confirm_request` 事件推给前端，`POST /api/chat/confirm` 经
//!   `WebBridge::resolve_confirm` 回填；responder 被 drop 一律按拒绝处理。
//!
//! 领域工具（9 个）与 Skill 模板（仓库根 `skills/`）都在 `zimu-agent` crate；
//! 工具与 HTTP API 共用同一 store/market 句柄——`refresh_quotes` 工具与
//! `POST /api/market/refresh` 是同一条链（`zimu_agent::service`）。
//!
//! 边界：Web 通道单会话（单 Agent、单确认槽位）；IM 通道（per-chat_id，T5）与
//! 会话服务端真值（T3）在同一 AgentCore 上继续演进（ADR-0011）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use robit_agent::Agent;
use robit_ai::RobitConfig;
use robit_web::WebBridge;
use tokio::task::JoinHandle;
use zimu_agent::assembly::AgentCore;
use zimu_agent::SharedStore;
use zimu_market::MarketApi;

// ============================================================================
// AgentBridge —— main 持有的接线端子（通用桥体在 robit_web::WebBridge）
// ============================================================================

pub struct AgentBridge {
    /// 事件订阅 / 消息注入 / 确认回填都在 WebBridge 上
    pub web: WebBridge,
    /// 宿主元数据：SSE 首帧 connected 用
    pub profile: String,
    pub model: String,
    pub tools: Vec<String>,
    /// 已加载的 Skill 名（启动日志/诊断）
    pub skills: Vec<String>,
    _handle: JoinHandle<()>,
}

/// 装配并启动 Web 通道 Agent（原料 = `AgentCore`，与 CLI/IM 通道同一装配线）。
/// `config` 被消耗进 core：`Agent::new` 需要 `config.app.context` 的引用，
/// core 持有一份（RobitConfig 不可 Clone，见 assembly 模块头）。
/// store/market 与 HTTP API 是同一份句柄（工具数字与端点数字同源）。
pub fn spawn_agent(
    working_dir: PathBuf,
    config: RobitConfig,
    store: SharedStore,
    market: Arc<dyn MarketApi>,
) -> anyhow::Result<AgentBridge> {
    let core = AgentCore::assemble(working_dir, config, store, market)?;

    // 诊断日志与拆分前逐字等价（冒烟对账锚点：模型行 + 工具/Skill 行）
    tracing::info!(
        "Agent 模型：{}/{}，api_key: {}",
        core.profile,
        core.model,
        core.api_key_masked()
    );
    tracing::info!(
        "领域工具 {} 个：{}；Skill {} 个：{}",
        core.tool_names.len(),
        core.tool_names.join(", "),
        core.skill_names.len(),
        if core.skill_names.is_empty() { "（无）".into() } else { core.skill_names.join(", ") }
    );

    let profile = core.profile.clone();
    let model = core.model.clone();
    let tools = core.tool_names.clone();
    let skills = core.skill_names.clone();

    // Agent::new 的参数在 move core 之前逐个取好（借用/部分移动互斥）
    let memory = core.memory()?;
    let context_config = core.context_config();
    let context_window = core.context_window();
    let working_dir = core.working_dir.clone();
    let auto_approve = core.auto_approve;

    let mut web = WebBridge::new(256, 16);
    let frontend = web.frontend();
    let message_rx = web.take_message_rx();

    let handle = tokio::spawn(async move {
        let agent = Agent::new(
            core.client,
            core.tools,
            core.skills,
            frontend,
            context_config.as_ref(),
            context_window,
            working_dir,
            auto_approve,
            HashMap::new(),
            memory,
        );
        agent.run(message_rx).await;
        tracing::warn!("Agent 运行循环已退出（/api/chat 将返回 503）");
    });

    Ok(AgentBridge {
        web,
        profile,
        model,
        tools,
        skills,
        _handle: handle,
    })
}
