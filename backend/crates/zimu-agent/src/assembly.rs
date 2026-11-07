//! 子木 · 唯一装配线（T1/ADR-0011）：`LlmClient` / `ToolRegistry` / `SkillRegistry`
//! 三个 Arc 与元数据一次装配，供多通道宿主共同消费——
//! zimu-server Web 桥（agent_bridge）、IM 通道（ChatbotManager 每会话一 Agent，T5）、
//! zimu-agent-cli（调试 REPL）。共享 Arc 多 Agent 是 robit 自家姿势
//! （robit-chatbot manager.rs 同款），不是子木发明。
//!
//! 边界（与 lib.rs 头部纪律一致）：
//! - core **不建 Frontend/桥**：一桥一会话、宿主用 key 管理是 robit-web 成文契约
//!   （bridge.rs "One bridge per session…key them in the host"），Frontend 属通道私有物；
//! - **R2 决策（docs/07）随迁**：工具面维持纯领域手搓（`build_registry`），不走
//!   robit `bootstrap_with`——其系统工具（read/load_skill/search_history/query_task）
//!   始终注册、不受白名单控制；server 工作目录可能存在 .env（密钥），把 read 暴露给
//!   LLM 是安全面。Skill 触发不依赖 load_skill 工具（触发词命中由 robit-agent 内部
//!   注入正文）；
//! - 数字口径只在 zimu-domain：本 crate 工具只取数→调领域函数→组装 JSON。
//!
//! `RobitConfig` 不可 Clone 且 `Agent::new` 需要 `app.context` 的引用——core 持一份
//! 供 Web 通道；需要按值再喂一口的宿主（ChatbotManager::new）自行 `load_config` 取
//! 第二份（配置加载可重复，robit-ai config.rs:388）。

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use robit_agent::{resolve_memory_settings, MemorySettings, SkillRegistry, ToolRegistry};
use robit_ai::{ContextConfig, LlmClient, RobitConfig};
use zimu_market::MarketApi;

use crate::skills::load_zimu_skills;
use crate::tools::build_registry;
use crate::{AgentDeps, SharedStore};

/// 装配产物：三 Arc + 通道无关元数据。日志/展示各宿主自便（server tracing、CLI println）。
pub struct AgentCore {
    /// Agent::new 的 working_dir（= 启动 cwd，canonicalize 后）；
    /// robit 的提示词/记忆搜索都以它为根（backend/ 同源是人格命中的结构前提，T8）
    pub working_dir: PathBuf,
    config: RobitConfig,
    pub client: Arc<LlmClient>,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
    pub profile: String,
    pub model: String,
    /// 已排序：SSE connected 首帧的 tools 列表顺序稳定（HashMap 遍历序不定）
    pub tool_names: Vec<String>,
    pub skill_names: Vec<String>,
    pub auto_approve: bool,
}

impl AgentCore {
    /// 装配（与 P2-T3 以来 agent_bridge/CLI 的既有顺序逐行等价，语义零改动）。
    /// store/market 与 HTTP API 同一份句柄——工具数字与端点数字同源（P4-T6 纪律）。
    pub fn assemble(
        working_dir: PathBuf,
        config: RobitConfig,
        store: SharedStore,
        market: Arc<dyn MarketApi>,
    ) -> anyhow::Result<Self> {
        let client = Arc::new(
            LlmClient::from_config(&config, None)
                .context("创建 LLM 客户端失败：检查 providers / default_model 配置")?,
        );
        let profile = client.profile().to_string();
        let model = client.model().to_string();

        let auto_approve = config
            .app
            .as_ref()
            .and_then(|a| a.auto_approve)
            .unwrap_or(false);

        // 领域工具面（9 个，无下单类；R2 决策见模块头）
        let deps = AgentDeps::new(store, market);
        let registry = build_registry(deps);
        let mut tool_names: Vec<String> =
            registry.tool_names().iter().map(|s| s.to_string()).collect();
        tool_names.sort();
        let tools = Arc::new(registry);

        // Skill 模板（仓库根 skills/，ZIMU_SKILLS_DIR 可覆盖）：坏文件逐条上报不致命
        let (loaded, errors) = load_zimu_skills();
        for e in &errors {
            tracing::warn!("Skill 加载失败（跳过该文件）：{e}");
        }
        let skill_names: Vec<String> =
            loaded.iter().map(|s| s.frontmatter.name.clone()).collect();
        let name_refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
        let skills = Arc::new(SkillRegistry::new(loaded, &name_refs));

        Ok(Self {
            working_dir,
            config,
            client,
            tools,
            skills,
            profile,
            model,
            tool_names,
            skill_names,
            auto_approve,
        })
    }

    /// 每个 Agent 实例各取一份记忆设置（会建目录，fallible——调用时机由宿主决定）。
    pub fn memory(&self) -> anyhow::Result<MemorySettings> {
        Ok(resolve_memory_settings(&self.config, &self.working_dir)?)
    }

    /// Agent::new 的 context_config 参数源（Clone 出来再 move core，避开借用/部分移动冲突）。
    pub fn context_config(&self) -> Option<ContextConfig> {
        self.config.app.as_ref().and_then(|a| a.context.as_ref()).cloned()
    }

    pub fn context_window(&self) -> Option<u64> {
        self.client.resolved().context_window
    }

    /// 脱敏诊断（原 server/CLI 两处重复的 mask_secret 收到这里——日志纪律一处化）：
    /// 401 时区分「${ENV_VAR} 未替换」与「key 失效」，完整值永不进日志。
    pub fn api_key_masked(&self) -> String {
        let s = self.client.resolved().api_key.as_str();
        if s.chars().count() > 10 {
            let head: String = s.chars().take(3).collect();
            let tail: String = s.chars().skip(s.chars().count() - 4).collect();
            format!("{head}***{tail}（len {}）", s.chars().count())
        } else {
            "***（长度异常，疑似未解析）".to_string()
        }
    }
}
