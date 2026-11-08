//! 子木 · Web 通道接线（P2-T3 起；P2-T5 桥体下沉 robit-web；P4-T6 工具面成形；
//! T1/ADR-0011 装配段抽入 `zimu_agent::assembly::AgentCore`；T3 会话真值上服务端）。
//!
//! 本文件只剩通道私有物：`WebBridge` 构造、Agent 任务 spawn、recorder 任务、
//! LlmClient / ToolRegistry / SkillRegistry 的装配在 zimu-agent::assembly（Web 桥、
//! IM 通道、调试 REPL 共用同一条线）；`Frontend` 差异是 CliFrontend 换成 robit-web 的
//! `WebFrontend`：
//!
//! * 事件下行：AgentEvent → `WebEvent`（官方 JSON schema）→ broadcast 扇出，
//!   多浏览器连接可同时观看；SSE 编码在宿主 `main.rs`；**同流分一支进 recorder**
//!   落 `chat_messages`（T3：档案状态机在 `chat_record`，thinking/通知不入库）；
//! * 输入上行：`POST /api/chat` → `AgentBridge::submit_user_text`（**先落库再注入**）
//!   → `WebBridge::send_user_text` → `Agent::run`；调度器复盘走同一个包装，同源留痕；
//! * 工具确认：`confirm_request` 事件推给前端，`POST /api/chat/confirm` 经
//!   `WebBridge::resolve_confirm` 回填；responder 被 drop 一律按拒绝处理。
//!
//! 复活（T3）：启动时从档案取 user/assistant 文本对喂 `Agent::with_history`
//! （robit 内部 sanitize+truncate，喂全量即可）；空档案退化为 `Agent::new`
//! 原语义。工具轨迹不回灌（陈旧数字不进上下文，03「派生数字现算」）。
//!
//! 领域工具（14 个注册，含写账本五件 ADR-0012/0013）与 Skill 模板（仓库根 `skills/`）都在 `zimu-agent` crate；
//! 工具与 HTTP API 共用同一 store/market 句柄——`refresh_quotes` 工具与
//! `POST /api/market/refresh` 是同一条链（`zimu_agent::service`）。
//!
//! 边界：Web 通道单会话（单 Agent、单确认槽位）；IM 通道（per-chat_id，T5）与
//! 档案端点（`chat_api`）在同一 AgentCore 上继续演进（ADR-0011）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use robit_agent::event::new_session_id;
use robit_agent::storage::{message_to_chat_message, MessageData};
use robit_agent::Agent;
use robit_ai::{ChatCompletionRequestMessage, RobitConfig};
use robit_web::{WebBridge, WebEvent};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use zimu_agent::assembly::AgentCore;
use zimu_agent::SharedStore;
use zimu_market::MarketApi;

// bin 模块引用 lib 目标（zimu-server 双目标布局：crate:: 是 bin 自己）；
// 锁助手走 zimu-agent::lock（公共面，中毒不毒化与 holdings_api 私有 lock 同语义）
use zimu_agent::lock;
use zimu_server::chat_record::{ts_now, RecorderCore, RowSpec, WEB_SESSION};

// ============================================================================
// AgentBridge —— main 持有的接线端子（通用桥体在 robit_web::WebBridge）
// ============================================================================

pub struct AgentBridge {
    /// 事件订阅 / 消息注入 / 确认回填都在 WebBridge 上
    pub web: WebBridge,
    /// 会话档案写入口（与 HTTP API 同一份 store；submit_user_text 先落库后注入）
    pub store: SharedStore,
    /// 装配三件套的公共句柄（T5：QQ 通道 ChatbotManager 与 Web 共用同一
    /// LlmClient/ToolRegistry/SkillRegistry——共享 Arc 多 Agent 是框架自家姿势）
    pub client: Arc<robit_ai::LlmClient>,
    pub registry: Arc<robit_agent::ToolRegistry>,
    pub skills_registry: Arc<robit_agent::SkillRegistry>,
    /// 宿主元数据：SSE 首帧 connected 用
    pub profile: String,
    pub model: String,
    pub tools: Vec<String>,
    /// 已加载的 Skill 名（启动日志/诊断）
    pub skills: Vec<String>,
    _agent: JoinHandle<()>,
    _recorder: JoinHandle<()>,
}

impl AgentBridge {
    /// 注入一条用户消息：**先落档案再进通道**。
    /// - 档案写失败 → 不注入（拒绝"模型记得、档案没有"的分叉）；
    /// - 档案成功、通道失败（agent 刚退出）→ 行留在档案里，重启复活时经
    ///   `with_history` 回灌——档案即队列，语义诚实。
    pub async fn submit_user_text(&self, text: String) -> anyhow::Result<()> {
        lock(&self.store)
            .append_chat_message(WEB_SESSION, "user", &text, None, &ts_now())
            .context("会话档案写入失败，消息未注入")?;
        self.web
            .send_user_text(text)
            .await
            .map_err(|_| anyhow!("Agent 已退出，消息未注入"))
    }
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
    // 复活历史（T3）：在 assemble 消耗 store 前取档案；空档案走 Agent::new 原路
    let history = load_history(&store)?;
    if !history.is_empty() {
        tracing::info!("会话复活：从档案回灌 {history} 条 user/assistant 消息", history = history.len());
    }

    let core = AgentCore::assemble(working_dir, config, store.clone(), market)?;

    // 诊断日志与 T1 拆分前逐字等价（冒烟对账锚点：模型行 + 工具/Skill 行）
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

    // Agent::new 的参数在 move core 字段之前逐个取好（借用/部分移动互斥）
    let memory = core.memory()?;
    let context_config = core.context_config();
    let context_window = core.context_window();
    let working_dir = core.working_dir.clone();
    let auto_approve = core.auto_approve;
    // 三件套公共句柄（T5 QQ 通道消费；Arc clone 廉价）
    let client_arc = core.client.clone();
    let tools_arc = core.tools.clone();
    let skills_arc = core.skills.clone();

    let mut web = WebBridge::new(256, 16);
    let frontend = web.frontend();
    let message_rx = web.take_message_rx();

    // recorder（T3）：订阅同一 broadcast 落档案。订阅发生在 Agent 启动前，
    // broadcast 语义「订阅前不补发」——从此刻起的事件全部进档案。
    let recorder_rx = web.subscribe();
    let recorder_store = store.clone();
    let recorder = tokio::spawn(async move {
        let mut recorder_rx = recorder_rx;
        let mut core = RecorderCore::new();
        loop {
            match recorder_rx.recv().await {
                Ok(ev) => {
                    let rows = core.on_event(&ev);
                    if !rows.is_empty() {
                        persist_rows(&recorder_store, rows, &ev);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("会话档案 recorder 落后 {n} 帧：当轮标注「档案不完整」");
                    core.on_lagged();
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::info!("WebBridge 已释放，档案 recorder 退出");
                    break;
                }
            }
        }
    });

    let agent = tokio::spawn(async move {
        let agent = if history.is_empty() {
            Agent::new(
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
            )
        } else {
            Agent::with_history(
                core.client,
                core.tools,
                core.skills,
                frontend,
                context_config.as_ref(),
                context_window,
                working_dir,
                auto_approve,
                HashMap::new(),
                new_session_id(),
                history,
                memory,
            )
        };
        agent.run(message_rx).await;
        tracing::warn!("Agent 运行循环已退出（/api/chat 将返回 503）");
    });

    Ok(AgentBridge {
        web,
        store,
        client: client_arc,
        registry: tools_arc,
        skills_registry: skills_arc,
        profile,
        model,
        tools,
        skills,
        _agent: agent,
        _recorder: recorder,
    })
}

/// 档案行落库（一行一事务——档案量小，串行锁与全站同纪律）。
fn persist_rows(store: &SharedStore, rows: Vec<RowSpec>, last_ev: &WebEvent) {
    let st = lock(store);
    for row in rows {
        let (role, text, tool_json) = match row {
            RowSpec::Tool { tool_json } => ("tool", String::new(), Some(tool_json)),
            RowSpec::Assistant { text } => ("assistant", text, None),
        };
        if let Err(e) = st.append_chat_message(WEB_SESSION, role, &text, tool_json.as_deref(), &ts_now()) {
            // 落档失败不吞：喊出来（后续 SSE 照常——展示与档案两条腿各走各的）
            tracing::warn!("会话档案写入失败（role={role}，末帧={last_ev:?}）：{e:#}");
        }
    }
}

/// 复活链路（T3）：chat_messages 全量 → user/assistant 文本对 →
/// robit `message_to_chat_message` → `Agent::with_history` 输入。
/// tool 行只供 GET 展示，不进这里（陈旧数字不回灌，03「派生数字现算」）。
fn load_history(store: &SharedStore) -> anyhow::Result<Vec<ChatCompletionRequestMessage>> {
    let rows = {
        let st = lock(store);
        // i64::MAX = 全量（store 内部饱和加探页；档案体量受个人使用上限约束）
        st.chat_messages(WEB_SESSION, i64::MAX, None)?.0
    };
    let mut history: Vec<ChatCompletionRequestMessage> = Vec::with_capacity(rows.len());
    for r in rows.iter().rev() {
        if r.role != "user" && r.role != "assistant" {
            continue;
        }
        let data = MessageData {
            id: r.id,
            role: r.role.clone(),
            content: r.text.clone(),
            tool_name: None,
            tool_call_id: None,
            tool_info: None,
            created_at: r.ts.clone(),
        };
        match message_to_chat_message(&data) {
            Ok(m) => history.push(m),
            Err(e) => tracing::warn!("档案行 #{} 转换失败（跳过该条）：{e}", r.id),
        }
    }
    Ok(history)
}
