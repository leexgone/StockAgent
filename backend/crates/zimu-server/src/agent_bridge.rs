//! 子木 · robit Agent ↔ Web SSE 桥（P2-T3，见 docs/06-roadmap.md）。
//!
//! Agent 装配方式与 `zimu-agent-cli` 完全相同（LlmClient / ToolRegistry /
//! SkillRegistry / Agent::new），差异只在 `Frontend` 实现——CliFrontend 换成
//! [`SseFrontend`]：
//!
//! * 事件下行：Agent 事件 → `broadcast` 通道 → `GET /api/chat/stream`（SSE），
//!   多浏览器连接可同时观看；
//! * 输入上行：`POST /api/chat` → `mpsc` 通道 → `Agent::run`；
//! * 工具确认：`request_tool_confirmation` 挂起等待 oneshot——确认请求经
//!   [`SseEvent::ConfirmRequest`] 推给前端，`POST /api/chat/confirm` 回填答复。
//!   （robit 0.1.25 的确认只走该方法；`FrontendMessage::ConfirmationResponse`
//!   是保留变体，Agent 收到会 warn。）
//!
//! spike 边界：进程级单会话（单 Agent、单确认槽位）；多会话与持久化留给 P4+。
//! echo 工具与 CLI 里那份重复——spike 期容忍，P4-T6 领域工具成形时抽公共 crate。
//!
//! 「Web 桥该不该进 robit」的评估素材：本文件的 SseEvent 映射、确认槽位、
//! broadcast 扇出都是宿主侧重复劳动，见 docs/07-backlog.md R 系列。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use robit_agent::{
    resolve_memory_settings, Agent, AgentEvent, Frontend, FrontendMessage, SkillRegistry, Tool,
    ToolCallInfo, ToolContext, ToolRegistry, ToolResult,
};
use robit_ai::{LlmClient, RobitConfig};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;

// ============================================================================
// SSE 出站事件 —— AgentEvent 的 JSON 投影
// ============================================================================

/// robit 的 `AgentEvent` 不带 Serialize（也不该由 robit 决定线上格式），
/// 转成子木自己的 wire 类型。前端按 `type` 字段分发（snake_case）。
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    /// SSE 连接建立后的首帧：前端据此显示模型与工具面
    Connected {
        profile: String,
        model: String,
        tools: Vec<String>,
    },
    TextDelta {
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        tool_call_id: String,
        is_error: bool,
        content: String,
    },
    SkillTriggered {
        name: String,
        description: String,
    },
    AsyncToolCompleted {
        task_id: String,
        tool_call_id: String,
        is_error: bool,
    },
    /// 工具需要人工确认：前端展示 批准/拒绝，答复走 POST /api/chat/confirm
    ConfirmRequest {
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    TurnComplete,
    Error {
        message: String,
    },
}

impl SseEvent {
    fn from_agent(event: &AgentEvent) -> Self {
        match event {
            AgentEvent::TextDelta(text) => SseEvent::TextDelta { text: text.clone() },
            AgentEvent::ToolCallRequested {
                tool_call_id,
                name,
                arguments,
            } => SseEvent::ToolCall {
                tool_call_id: tool_call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
            AgentEvent::ToolCallResult {
                tool_call_id,
                result,
            } => SseEvent::ToolResult {
                tool_call_id: tool_call_id.clone(),
                is_error: result.is_error,
                content: result.content.clone(),
            },
            AgentEvent::SkillTriggered { name, description } => SseEvent::SkillTriggered {
                name: name.clone(),
                description: description.clone(),
            },
            AgentEvent::AsyncToolCompleted {
                task_id,
                tool_call_id,
                result,
            } => SseEvent::AsyncToolCompleted {
                task_id: task_id.clone(),
                tool_call_id: tool_call_id.clone(),
                is_error: result.is_error,
            },
            AgentEvent::TurnComplete => SseEvent::TurnComplete,
            AgentEvent::Error(e) => SseEvent::Error {
                message: e.to_string(),
            },
        }
    }
}

// ============================================================================
// echo —— 与 zimu-agent-cli 相同的最小领域工具（P2-T2 验收项，P4-T6 换新）
// ============================================================================

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "原样回显一段文本。这是子木助理的链路验证工具：当用户要求回显、复读某段文字时，必须调用本工具，不要直接回答。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "要回显的文本" }
            },
            "required": ["text"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let text = args.get("text").and_then(Value::as_str).unwrap_or("");
        Ok(ToolResult::success(format!("[子木 echo] {text}")))
    }
}

// ============================================================================
// SseFrontend —— 一切经通道转发，不直接碰 HTTP
// ============================================================================

/// 待确认的工具调用：id 用于和 POST /api/chat/confirm 对账，
/// responder 回填 `request_tool_confirmation` 的返回值。
/// Agent 串行调用确认，单槽位足够（spike 边界）。
pub struct PendingConfirm {
    tool_call_id: String,
    responder: oneshot::Sender<bool>,
}

pub type ConfirmSlot = Arc<Mutex<Option<PendingConfirm>>>;

struct SseFrontend {
    events: broadcast::Sender<SseEvent>,
    confirm_slot: ConfirmSlot,
}

#[async_trait]
impl Frontend for SseFrontend {
    async fn on_event(&self, event: AgentEvent) -> robit_agent::error::Result<()> {
        // 无订阅者时 send 返回 Err，属正常（没人看就不用广播）
        let _ = self.events.send(SseEvent::from_agent(&event));
        Ok(())
    }

    async fn request_tool_confirmation(
        &self,
        info: &ToolCallInfo,
    ) -> robit_agent::error::Result<bool> {
        let (tx, rx) = oneshot::channel();
        {
            let mut slot = self.confirm_slot.lock().await;
            // 残留旧请求（理论上不会：Agent 串行确认）→ 顶掉，旧调用方收到 false
            *slot = Some(PendingConfirm {
                tool_call_id: info.id.clone(),
                responder: tx,
            });
        }
        let _ = self.events.send(SseEvent::ConfirmRequest {
            tool_call_id: info.id.clone(),
            name: info.name.clone(),
            arguments: info.arguments.clone(),
        });
        // 挂起等待 HTTP 侧回填；连接断开（responder 被 drop）按拒绝处理
        match rx.await {
            Ok(approved) => Ok(approved),
            Err(_) => Ok(false),
        }
    }
}

// ============================================================================
// AgentBridge —— main 持有的接线端子
// ============================================================================

pub struct AgentBridge {
    /// SSE 端点从这里 subscribe
    pub events: broadcast::Sender<SseEvent>,
    /// POST /api/chat 往这里投递用户消息
    pub message_tx: mpsc::Sender<FrontendMessage>,
    /// POST /api/chat/confirm 通过它回填确认答复
    pub confirm_slot: ConfirmSlot,
    pub profile: String,
    pub model: String,
    pub tools: Vec<String>,
    _handle: JoinHandle<()>,
}

/// 装配并启动 Agent（与 zimu-agent-cli 同一套顺序）。
/// `config` 被消耗：`Agent::new` 持有 `config.app.context` 的引用，
/// 必须在任务内构造后一起 move 进去。
pub fn spawn_agent(working_dir: PathBuf, config: RobitConfig) -> anyhow::Result<AgentBridge> {
    let client = Arc::new(
        LlmClient::from_config(&config, None)
            .context("创建 LLM 客户端失败：检查 providers / default_model 配置")?,
    );
    let profile = client.profile().to_string();
    let model = client.model().to_string();
    // 脱敏诊断（与 CLI 同款）：401 时区分「${ENV_VAR} 未替换」与「key 失效」
    tracing::info!(
        "Agent 模型：{}/{}，api_key: {}",
        profile,
        model,
        mask_secret(&client.resolved().api_key)
    );

    let auto_approve = config
        .app
        .as_ref()
        .and_then(|a| a.auto_approve)
        .unwrap_or(false);

    // 最小工具集：只注册 echo，不走 bootstrap()（spike 边界，同 CLI）
    let mut tools = ToolRegistry::new();
    tools.register(EchoTool);
    let tools = Arc::new(tools);
    let skills = Arc::new(SkillRegistry::new(Vec::new(), &["echo"]));

    let memory = resolve_memory_settings(&config, &working_dir)?;

    let (events, _) = broadcast::channel::<SseEvent>(256);
    let (message_tx, message_rx) = mpsc::channel::<FrontendMessage>(16);
    let confirm_slot: ConfirmSlot = Arc::new(Mutex::new(None));
    let frontend = Arc::new(SseFrontend {
        events: events.clone(),
        confirm_slot: confirm_slot.clone(),
    });

    let handle = tokio::spawn(async move {
        let context_config = config.app.as_ref().and_then(|a| a.context.as_ref());
        let context_window = client.resolved().context_window;
        let agent = Agent::new(
            client,
            tools,
            skills,
            frontend,
            context_config,
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
        events,
        message_tx,
        confirm_slot,
        profile,
        model,
        tools: vec!["echo".to_string()],
        _handle: handle,
    })
}

/// 回填工具确认答复。返回 false = 当前没有匹配的待确认请求。
pub async fn resolve_confirm(slot: &ConfirmSlot, tool_call_id: &str, approved: bool) -> bool {
    let mut guard = slot.lock().await;
    let matched = guard
        .as_ref()
        .map(|p| p.tool_call_id == tool_call_id)
        .unwrap_or(false);
    if matched {
        if let Some(pending) = guard.take() {
            let _ = pending.responder.send(approved);
        }
    }
    matched
}

/// 脱敏展示密钥：只留前 3 后 4 位与长度，日志永不出现完整值。
fn mask_secret(s: &str) -> String {
    if s.chars().count() > 10 {
        let head: String = s.chars().take(3).collect();
        let tail: String = s.chars().skip(s.chars().count() - 4).collect();
        format!("{head}***{tail}（len {}）", s.chars().count())
    } else {
        "***（长度异常，疑似未解析）".to_string()
    }
}
