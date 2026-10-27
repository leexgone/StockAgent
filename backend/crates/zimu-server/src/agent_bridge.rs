//! 子木 · robit Agent 装配（P2-T3 起；P2-T5 共同演进演练后桥体已下沉 robit-web）。
//!
//! Agent 装配方式与 `zimu-agent-cli` 完全相同（LlmClient / ToolRegistry /
//! SkillRegistry / Agent::new），差异只在 `Frontend` 实现——CliFrontend 换成
//! robit-web 的 `WebFrontend`（robit v0.1.26 首发，见其 CHANGELOG）：
//!
//! * 事件下行：AgentEvent → `WebEvent`（官方 JSON schema）→ broadcast 扇出，
//!   多浏览器连接可同时观看；SSE 编码在宿主 `main.rs`；
//! * 输入上行：`POST /api/chat` → `WebBridge::send_user_text` → `Agent::run`；
//! * 工具确认：`confirm_request` 事件推给前端，`POST /api/chat/confirm` 经
//!   `WebBridge::resolve_confirm` 回填；responder 被 drop 一律按拒绝处理。
//!
//! 原先宿主侧手写的 SseEvent 投影 / SseFrontend / 确认槽位已删除——那正是
//! docs/07-backlog.md R1 记录的重复劳动，P2-T5 把它通用化进了 robit-web
//! （改 robit → tag v0.1.26 → 子木 pin 升级，回路首次走通）。
//!
//! spike 边界：进程级单会话（单 Agent、单确认槽位）；多会话与持久化留给 P4+。
//! echo 工具与 CLI 里那份重复——spike 期容忍，P4-T6 领域工具成形时抽公共 crate。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use robit_agent::{
    resolve_memory_settings, Agent, SkillRegistry, Tool, ToolContext, ToolRegistry, ToolResult,
};
use robit_ai::{LlmClient, RobitConfig};
use robit_web::WebBridge;
use serde_json::{json, Value};
use tokio::task::JoinHandle;

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
// AgentBridge —— main 持有的接线端子（通用桥体在 robit_web::WebBridge）
// ============================================================================

pub struct AgentBridge {
    /// 事件订阅 / 消息注入 / 确认回填都在 WebBridge 上
    pub web: WebBridge,
    /// 宿主元数据：SSE 首帧 connected 用
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

    let mut web = WebBridge::new(256, 16);
    let frontend = web.frontend();
    let message_rx = web.take_message_rx();

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
        web,
        profile,
        model,
        tools: vec!["echo".to_string()],
        _handle: handle,
    })
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
