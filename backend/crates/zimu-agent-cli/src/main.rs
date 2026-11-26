//! 子木 · zimu-agent-cli —— Agent 调试 REPL（P2-T2 spike 起；P4-T6 换装真实领域工具面）。
//!
//! 与 zimu-server 的 Agent 完全同装配（`zimu-agent` 公共 crate，ADR-0014）：
//! - 领域工具 15 件：market_status / get_quote / get_holdings / calc_discipline /
//!   simulate_trade / record_trade / adjust_cash / get_trades / upsert_holding /
//!   remove_holding（写账本五件，ADR-0012/0013 自动落账+读回）/ refresh_quotes /
//!   calc_advice / finalize_advice / calc_diagnosis（P7-T10 诊断报告取数）
//!   + echo（链路验证）；
//! - robit 标准面：read / write / edit / ls / find / grep / bash / load_skill /
//!   search_history / query_task（+ generate_image 随 key、read_image 随 vision
//!   配置按需）——确认关闭自动执行（auto_approve=true，ADR-0014）；
//! - Skill 模板：系统技能（盘后复盘 /review /复盘、盘前关注 /watch /盘前、
//!   建议生成 /advice /建议 <代码>）应用初始化自仓库根 `skills/` 播种进
//!   `<cwd>/.robit/skills`（缺则播种永不覆盖，用户可自增技能，ADR-0021），
//!   `ZIMU_SKILLS_DIR` 可覆盖播种源。
//!
//! 用法（在 `backend/` 下）：`cargo run -p zimu-agent-cli`
//!
//! 配置读取顺序：`<cwd>/.robit/config.toml` → `~/.robit/config.toml`
//!（api_key 支持 `${ENV_VAR}` 替换，`.env` 自 `~/.robit/.env` 与 `<cwd>/.robit/.env` 加载）。
//! 会话库与记忆文件写在 `<cwd>/.robit/memory/`，已被仓库 `.gitignore` 覆盖。
//! 持仓库与行情 sidecar 与 zimu-server 同款环境变量：`ZIMU_DB_PATH`（默认 <cwd>/zimu.db）、
//! `ZIMU_MARKET_URL` / `ZIMU_MARKET_TIMEOUT_MS`（sidecar 不在也能起，行情工具如实报错）。

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use async_trait::async_trait;
use robit_agent::{Agent, AgentEvent, Frontend, FrontendMessage, ToolCallInfo};
use robit_ai::{build_log_filter, install_panic_hook, load_config, log_file_layer};
use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer as _;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use zimu_agent::assembly::AgentCore;
use zimu_market::client::{HttpMarketClient, DEFAULT_BASE_URL, DEFAULT_TIMEOUT};
use zimu_market::MarketApi as _; // base_url() 是 trait 方法

// ============================================================================
// main —— 装配 Agent 并跑 REPL
// ============================================================================

fn main() -> anyhow::Result<()> {
    // 工作目录 = 启动目录：会话库/记忆/项目级配置都以它为根
    let working_dir = std::fs::canonicalize(std::env::current_dir().context("无法读取当前工作目录")?)?;

    let config = load_config(None)
        .context("加载 robit 配置失败：需要 <cwd>/.robit/config.toml 或 ~/.robit/config.toml")?;
    // robit 0.1.27 R3 可组合原语（与 zimu-server 同款单一初始化路径）。
    // host_targets 顺带修正旧写法：原来只传 "zimu_agent"，CLI 自身 crate
    // （zimu_agent_cli）的日志反而没被放行。
    // 控制台层时间戳 = 本地时间（与 zimu-server / robit 文件层同款）：复用 robit
    // 公开 local_utc_offset() + 与 robit local_timer 同格式的 OffsetTime，
    // 替代 tracing-subscriber 缺省 UTC 计时器。
    let console_timer = tracing_subscriber::fmt::time::OffsetTime::new(
        robit_ai::logging::local_utc_offset(),
        time::format_description::parse_borrowed::<3>(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6][offset_hour sign:mandatory]:[offset_minute]",
        )
        .expect("hardcoded log timestamp format is valid"),
    );
    let filter = build_log_filter(
        config.app.as_ref(),
        &["zimu_agent_cli", "zimu_agent", "zimu_market"],
        &[],
    );
    let file_layer = log_file_layer(config.app.as_ref(), &working_dir, filter.clone());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_timer(console_timer).with_filter(filter))
        .with(file_layer)
        .init();
    install_panic_hook();

    // 持仓库与行情适配器：与 zimu-server 同款环境变量，工具数字与 HTTP API 同源
    let db_path = std::env::var("ZIMU_DB_PATH").unwrap_or_else(|_| "zimu.db".into());
    let store = Arc::new(Mutex::new(
        zimu_store::Store::open(&db_path).with_context(|| format!("打开持仓数据库失败：{db_path}"))?,
    ));
    let market_client = Arc::new(HttpMarketClient::from_env().unwrap_or_else(|e| {
        println!("[warn] 按环境装配行情适配器失败（{e}），回落默认配置");
        HttpMarketClient::new(DEFAULT_BASE_URL, DEFAULT_TIMEOUT)
            .expect("默认配置的行情客户端构建不应失败")
    }));
    let market_url = market_client.base_url().to_string();

    // 唯一装配线（T1/ADR-0011）：与 zimu-server 同一个 AgentCore，
    // REPL 的差异只在 Frontend 实现（CliFrontend vs WebFrontend）。
    let core = AgentCore::assemble(working_dir.clone(), config, store, market_client)?;
    println!(
        "子木 Agent（调试 REPL）| profile: {} | model: {}",
        core.profile, core.model
    );
    // 脱敏诊断：确认 ${ENV_VAR} 替换是否生效（401 时区分「替换失败」与「key 失效」）
    println!("api_key: {}", core.api_key_masked());
    println!("持仓库：{db_path} | 行情 sidecar：{market_url}（不在线时行情工具如实报错）");
    println!("已注册工具：{}", core.tool_names.join(", "));
    println!(
        "已加载 Skill：{}\n输入消息开始对话；/复盘 /盘前 /建议 <代码> 触发模板；exit 或 /exit 退出\n",
        if core.skill_names.is_empty() {
            "（无——检查仓库根 skills/ 目录）".to_string()
        } else {
            core.skill_names.join(", ")
        }
    );

    // 通道约定（与 robit examples/robit-agent 相同）：
    //   event   Agent → 主任务（stdout 只由主任务写，避免并发打印交错）
    //   message 主任务 → Agent（用户输入）
    //   confirm Agent → 主任务（工具确认；ADR-0014 确认关闭 auto_approve=true
    //   自动执行，本臂保留为 trait 要求与将来重开预留；写账本五件软门禁不变，ADR-0012）
    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(64);
    let (message_tx, message_rx) = mpsc::channel::<FrontendMessage>(16);
    let (confirm_tx, mut confirm_rx) = mpsc::channel::<(String, oneshot::Sender<bool>)>(4);

    let frontend = Arc::new(CliFrontend {
        event_tx,
        confirm_tx,
    });

    // Agent::new 的借用参数在 move core 字段前逐个取好（借用/部分移动互斥）
    let memory = core.memory()?;
    let context_config = core.context_config();
    let context_window = core.context_window();
    let retry = core.retry_settings(); // 0.8.0：[app.retry] LLM 超时/重试策略
    let agent = Agent::new(
        core.client,
        core.tools,
        core.skills,
        frontend,
        context_config.as_ref(),
        context_window,
        core.working_dir,
        core.auto_approve,
        HashMap::new(),
        memory,
        None, // custom_prompt（0.2.0/H6）：调试 REPL 维持 agent.md 查找路径
    )
    .with_retry_settings(retry);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let agent_handle = tokio::spawn(async move {
            agent.run(message_rx).await;
        });

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin).lines();

        let mut pending_confirm: Option<oneshot::Sender<bool>> = None;
        // 思考段进行中（R7：ThinkingDelta 渲染成暗体段，段首/段尾需要状态跟踪）
        let mut in_thinking = false;

        print!("> ");
        let _ = std::io::stdout().flush();

        loop {
            tokio::select! {
                maybe_event = event_rx.recv() => {
                    if let Some(event) = maybe_event {
                        let done = matches!(event, AgentEvent::TurnComplete);
                        render_event(&event, &mut in_thinking);
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

/// 把 Agent 事件渲染到 stdout。只在主任务里调用。
/// （脱敏诊断已收进 `AgentCore::api_key_masked`——原 CLI/ server 两处重复的一处化）
/// `in_thinking` 跟踪思考段（R7，robit 0.1.28）：段首打 💭 标记进暗体，
/// 任何非思考事件先收口——顺序契约无显式结束标记，`turn_complete` 兜底。
fn render_event(event: &AgentEvent, in_thinking: &mut bool) {
    if *in_thinking && !matches!(event, AgentEvent::ThinkingDelta(_)) {
        println!("\x1b[0m");
        *in_thinking = false;
    }
    match event {
        AgentEvent::TextDelta(text) => {
            print!("{}", text);
            let _ = std::io::stdout().flush();
        }
        AgentEvent::ThinkingDelta(text) => {
            // 调试 REPL 的用途之一就是人工核验 reasoning_content 链路，
            // 故终端以 ANSI 暗体弱样式流式渲染（不做折叠，GUI/网页才有那形态）。
            if !*in_thinking {
                print!("\n\x1b[2m💭 ");
                *in_thinking = true;
            }
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
        // 0.8.0 新变体（穷举 match 补臂，0.1.28 先例）：调试 REPL 无调度接线，
        // 正常不会出现；如实打印触发提示（robit example 同款）。
        AgentEvent::ScheduledTaskFired { task_id, title } => {
            println!("\n[计划任务触发] {title} (id={task_id})");
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
