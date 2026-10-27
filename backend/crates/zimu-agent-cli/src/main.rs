//! 子木 · zimu-agent-cli —— P2-T2 robit 集成 spike（见 docs/06-roadmap.md）。
//!
//! 验证三件事：
//! 1. crates.io 发布的 robit 核心组件（`robit-ai` / `robit-agent` 0.1.25）可被子木直接消费，
//!    无需 path override；
//! 2. 最小 Agent 会话经阿里云网关（DashScope compatible-mode）接通 qwen 模型，可流式对话；
//! 3. 自定义领域工具（`echo`）能注册进 robit 的 `ToolRegistry` 并被模型正确调用。
//!
//! 用法（在 `backend/` 下）：`cargo run -p zimu-agent-cli`
//!
//! 配置读取顺序：`<cwd>/.robit/config.toml` → `~/.robit/config.toml`
//!（api_key 支持 `${ENV_VAR}` 替换，`.env` 自 `~/.robit/.env` 与 `<cwd>/.robit/.env` 加载）。
//! 会话库与记忆文件写在 `<cwd>/.robit/memory/`，已被仓库 `.gitignore` 覆盖。
//!
//! P2-T3 把会话搬进 zimu-server 时，替换的只是 `CliFrontend`（换成 SSE 桥），
//! Agent / ToolRegistry / LlmClient 的装配方式原样复用；本 REPL 保留为调试入口。

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use robit_agent::{
    resolve_memory_settings, Agent, AgentEvent, Frontend, FrontendMessage, SkillRegistry, Tool,
    ToolCallInfo, ToolContext, ToolRegistry, ToolResult,
};
use robit_ai::{init_logging, load_config, LlmClient};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

// ============================================================================
// echo —— 最小领域工具（P2-T2 验收项）
// ============================================================================

/// 回显工具：领域工具集的最小样例，验证「子木实现 robit `Tool` trait → 注册 →
/// 模型 function-calling 调用」这条链路。P4-T6 起换成 get_quote / calc_discipline 等。
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
// main —— 装配 Agent 并跑 REPL
// ============================================================================

fn main() -> anyhow::Result<()> {
    // 工作目录 = 启动目录：会话库/记忆/项目级配置都以它为根
    let working_dir = std::fs::canonicalize(std::env::current_dir().context("无法读取当前工作目录")?)?;

    let config = load_config(None)
        .context("加载 robit 配置失败：需要 <cwd>/.robit/config.toml 或 ~/.robit/config.toml")?;
    init_logging(config.app.as_ref(), "zimu_agent", &working_dir, &[]);

    let auto_approve = config
        .app
        .as_ref()
        .and_then(|a| a.auto_approve)
        .unwrap_or(false);

    let client = Arc::new(
        LlmClient::from_config(&config, None)
            .context("创建 LLM 客户端失败：检查 providers / default_model 配置")?,
    );
    println!(
        "子木 Agent（P2-T2 spike）| profile: {} | model: {}",
        client.profile(),
        client.model()
    );
    // 脱敏诊断：确认 ${ENV_VAR} 替换是否生效（401 时区分「替换失败」与「key 失效」）
    println!("api_key: {}", mask_secret(&client.resolved().api_key));
    println!("已注册工具：echo | 输入消息开始对话，exit 或 /exit 退出\n");

    let context_config = config.app.as_ref().and_then(|a| a.context.as_ref());
    let context_window = client.resolved().context_window;

    // 最小工具集：只注册 echo，不走 bootstrap()（那会装配 read/bash 等通用工具）。
    // spike 只验证集成链路；领域工具面在 P4-T6 按 Skill 模板扩。
    let mut tools = ToolRegistry::new();
    tools.register(EchoTool);
    let tools = Arc::new(tools);
    let skills = Arc::new(SkillRegistry::new(Vec::new(), &["echo"]));

    // 通道约定（与 robit examples/robit-agent 相同）：
    //   event   Agent → 主任务（stdout 只由主任务写，避免并发打印交错）
    //   message 主任务 → Agent（用户输入）
    //   confirm Agent → 主任务（工具确认；echo 不需要，但 trait 要求实现）
    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(64);
    let (message_tx, message_rx) = mpsc::channel::<FrontendMessage>(16);
    let (confirm_tx, mut confirm_rx) = mpsc::channel::<(String, oneshot::Sender<bool>)>(4);

    let frontend = Arc::new(CliFrontend {
        event_tx,
        confirm_tx,
    });

    let memory = resolve_memory_settings(&config, &working_dir)?;

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

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let agent_handle = tokio::spawn(async move {
            agent.run(message_rx).await;
        });

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin).lines();

        let mut pending_confirm: Option<oneshot::Sender<bool>> = None;

        print!("> ");
        let _ = std::io::stdout().flush();

        loop {
            tokio::select! {
                maybe_event = event_rx.recv() => {
                    if let Some(event) = maybe_event {
                        let done = matches!(event, AgentEvent::TurnComplete);
                        render_event(&event);
                        if done {
                            print!("> ");
                            let _ = std::io::stdout().flush();
                        }
                    } else {
                        break; // event_tx 已掉落（Agent 退出）
                    }
                }

                maybe_req = confirm_rx.recv(), if pending_confirm.is_none() => {
                    if let Some((prompt, tx)) = maybe_req {
                        print!("{}", prompt);
                        let _ = std::io::stdout().flush();
                        pending_confirm = Some(tx);
                    }
                }

                result = reader.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            let trimmed = line.trim().to_string();

                            // 有待确认的工具调用时，本行输入优先作为确认答复
                            if let Some(tx) = pending_confirm.take() {
                                let answer = trimmed.to_lowercase();
                                let approved = answer.is_empty() || answer == "y" || answer == "yes";
                                let _ = tx.send(approved);
                                continue;
                            }

                            if trimmed.is_empty() {
                                continue;
                            }
                            if matches!(trimmed.as_str(), "exit" | "/exit" | "quit" | "/quit") {
                                break;
                            }
                            if trimmed == "/cancel" {
                                let _ = message_tx.send(FrontendMessage::Cancel).await;
                                continue;
                            }

                            if message_tx.send(trimmed.as_str().into()).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) | Err(_) => break, // EOF（Ctrl+D / 管道结束）
                    }
                }
            }
        }

        agent_handle.abort();
        println!("\n再见！");
    });

    Ok(())
}

/// 脱敏展示密钥：只留前 3 后 4 位与长度，日志/终端永不出现完整值。
fn mask_secret(s: &str) -> String {
    if s.chars().count() > 10 {
        let head: String = s.chars().take(3).collect();
        let tail: String = s.chars().skip(s.chars().count() - 4).collect();
        format!("{head}***{tail}（len {}）", s.chars().count())
    } else {
        "***（长度异常，疑似未解析）".to_string()
    }
}

/// 把 Agent 事件渲染到 stdout。只在主任务里调用。
fn render_event(event: &AgentEvent) {
    match event {
        AgentEvent::TextDelta(text) => {
            print!("{}", text);
            let _ = std::io::stdout().flush();
        }
        AgentEvent::ToolCallRequested { name, arguments, .. } => {
            println!("\n┌─ 🔧 {} ─────────────────────────", name);
            if let Ok(parsed) = serde_json::from_str::<Value>(arguments) {
                if let Some(obj) = parsed.as_object() {
                    for (k, v) in obj {
                        let display_val = match v {
                            Value::String(s) if s.len() > 200 => format!("{}...", &s[..200]),
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        println!("│ {}: {}", k, display_val);
                    }
                }
            } else {
                println!("│ {}", arguments);
            }
            println!("└────────────────────────────────────");
        }
        AgentEvent::ToolCallResult { result, .. } => {
            if result.is_error {
                println!("✗ 失败:\n{}", result.content);
            } else {
                let display = if result.content.len() > 1000 {
                    format!(
                        "{}\n...（输出截断，共 {} 字节）",
                        &result.content[..1000],
                        result.content.len()
                    )
                } else {
                    result.content.clone()
                };
                println!("✓ 结果:\n{}", display);
            }
        }
        AgentEvent::TurnComplete => {
            println!();
            let _ = std::io::stdout().flush();
        }
        AgentEvent::Error(e) => {
            eprintln!("\n[错误] {}", e);
        }
        AgentEvent::SkillTriggered { name, description } => {
            println!("\n[Skill] {} — {}", name, description);
        }
        AgentEvent::AsyncToolCompleted {
            task_id,
            tool_call_id,
            result,
        } => {
            println!(
                "\n[异步任务完成] task_id={} tool_call_id={} is_error={}",
                task_id, tool_call_id, result.is_error
            );
        }
    }
}

// ============================================================================
// CLI Frontend —— 一切经通道转发，不直接写 stdout
// ============================================================================

struct CliFrontend {
    event_tx: mpsc::Sender<AgentEvent>,
    confirm_tx: mpsc::Sender<(String, oneshot::Sender<bool>)>,
}

#[async_trait]
impl Frontend for CliFrontend {
    async fn on_event(&self, event: AgentEvent) -> robit_agent::error::Result<()> {
        let _ = self.event_tx.send(event).await;
        Ok(())
    }

    async fn request_tool_confirmation(
        &self,
        info: &ToolCallInfo,
    ) -> robit_agent::error::Result<bool> {
        let prompt = format!("\n⚠️  工具 '{}' 需要确认 [Y/n]: ", info.name);
        let (tx, rx) = oneshot::channel();
        let _ = self.confirm_tx.send((prompt, tx)).await;
        match rx.await {
            Ok(approved) => Ok(approved),
            Err(_) => Ok(false),
        }
    }
}
