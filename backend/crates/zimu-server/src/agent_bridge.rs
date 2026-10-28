//! 子木 · robit Agent 装配（P2-T3 起；P2-T5 桥体下沉 robit-web；P4-T6 工具面成形）。
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
//! P4-T6 起：领域工具（get_quote / get_holdings / calc_discipline / simulate_trade /
//! calc_advice / finalize_advice / refresh_quotes / market_status + echo 链路验证）
//! 与 Skill 模板（仓库根 `skills/`：盘后复盘 / 盘前关注 / 建议生成）都在
//! `zimu-agent` crate——P2 期「echo 两处重复」的债随公共 crate 兑现清偿。
//! 工具与 HTTP API 共用同一 store/market 句柄：`refresh_quotes` 工具与
//! `POST /api/market/refresh` 是同一条链（`zimu_agent::service`）。
//!
//! 边界：进程级单会话（单 Agent、单确认槽位）；多会话与持久化留给后续。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use robit_agent::{resolve_memory_settings, Agent, SkillRegistry};
use robit_ai::{LlmClient, RobitConfig};
use robit_web::WebBridge;
use tokio::task::JoinHandle;
use zimu_agent::skills::load_zimu_skills;
use zimu_agent::tools::build_registry;
use zimu_agent::{AgentDeps, SharedStore};
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

/// 装配并启动 Agent（与 zimu-agent-cli 同一套顺序）。
/// `config` 被消耗：`Agent::new` 持有 `config.app.context` 的引用，
/// 必须在任务内构造后一起 move 进去。
/// store/market 与 HTTP API 是同一份句柄（工具数字与端点数字同源）。
pub fn spawn_agent(
    working_dir: PathBuf,
    config: RobitConfig,
    store: SharedStore,
    market: Arc<dyn MarketApi>,
) -> anyhow::Result<AgentBridge> {
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

    // 领域工具面（zimu-agent 公共 crate；不走 robit bootstrap()——通用工具
    // read/bash 等一律不注册，子木的工具面就是领域面）
    let deps = AgentDeps::new(store, market);
    let registry = build_registry(deps);
    let mut tool_names: Vec<String> = registry.tool_names().iter().map(|s| s.to_string()).collect();
    tool_names.sort(); // connected 首帧的 tools 列表顺序稳定（HashMap 遍历序不定）
    let tools = Arc::new(registry);

    // Skill 模板（仓库根 skills/，ZIMU_SKILLS_DIR 可覆盖）：坏文件逐条上报不致命
    let (loaded, errors) = load_zimu_skills();
    for e in &errors {
        tracing::warn!("Skill 加载失败（跳过该文件）：{e}");
    }
    let skill_names: Vec<String> = loaded.iter().map(|s| s.frontmatter.name.clone()).collect();
    tracing::info!(
        "领域工具 {} 个：{}；Skill {} 个：{}",
        tool_names.len(),
        tool_names.join(", "),
        skill_names.len(),
        if skill_names.is_empty() { "（无）".into() } else { skill_names.join(", ") }
    );
    let name_refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
    let skills = Arc::new(SkillRegistry::new(loaded, &name_refs));

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
        tools: tool_names,
        skills: skill_names,
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
