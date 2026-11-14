//! 子木 · 唯一装配线（T1/ADR-0011）：`LlmClient` / `ToolRegistry` / `SkillRegistry`
//! 三个 Arc 与元数据一次装配，供多通道宿主共同消费——
//! zimu-server Web 桥（agent_bridge）、IM 通道（ChatbotManager 每会话一 Agent，T5）、
//! zimu-agent-cli（调试 REPL）。共享 Arc 多 Agent 是 robit 自家姿势
//! （robit-chatbot manager.rs 同款），不是子木发明。
//!
//! 边界（与 lib.rs 头部纪律一致）：
//! - core **不建 Frontend/桥**：一桥一会话、宿主用 key 管理是 robit-web 成文契约
//!   （bridge.rs "One bridge per session…key them in the host"），Frontend 属通道私有物；
//! - **工具面 = robit 标准面 + 领域 14 件（ADR-0014，推翻 R2）**：标准面走公开内核
//!   `create_tools_from_config_with`（与 bootstrap 同实现：截断缺省/provider 解析/
//!   memory 模式判定内置），**不用 bootstrap_with**——它强引全局 skill 目录，违背
//!   子木 skill 单源政策（仓库 skills/ 为准）；内核路径 skill 加载主权留子木。
//!   read_image 按需补注册（内核因主模型支持图像强制跳过，独立 vision 描述非冗余）；
//!   确认关闭自动执行（config auto_approve=true，ADR-0014 拍板；信任边界扩张与
//!   防线收缩明示于 ADR-0014）；
//! - 数字口径只在 zimu-domain：本 crate 工具只取数→调领域函数→组装 JSON。
//!
//! `RobitConfig` 不可 Clone 且 `Agent::new` 需要 `app.context` 的引用——core 持一份
//! 供 Web 通道；需要按值再喂一口的宿主（ChatbotManager::new）自行 `load_config` 取
//! 第二份（配置加载可重复，robit-ai config.rs:388）。

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use robit_agent::storage::{init_db, resolve_db_path};
use robit_agent::tool::read_image::ReadImageTool;
use robit_agent::{
    create_tools_from_config_with, resolve_memory_settings, MemorySettings, SkillRegistry,
    ToolRegistry,
};
use robit_ai::{resolve_vision_model, ContextConfig, LlmClient, RobitConfig};
use zimu_market::MarketApi;

use crate::skills::load_zimu_skills;
use crate::tools::{register_domain_tools, DOMAIN_TOOL_NAMES};
use crate::{AgentDeps, SharedStore};

/// robit 标准面可能注册的工具名（tools_required 校验名单用，warn-only）。
/// 条件件（generate_image 随 key、read_image 随 vision 配置）缺席时仅 warn 不阻断；
/// 记忆四件不在此列——file 模式维持（ADR-0014），memory_mode="tools" 另案。
const STANDARD_TOOL_NAMES: [&str; 12] = [
    "read", "write", "edit", "ls", "find", "grep", "bash", "generate_image", "read_image",
    "load_skill", "search_history", "query_task",
];

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
    /// 装配（ADR-0014 重排：skills 先行 → robit 标准面内核 → read_image 按需 →
    /// init_db → 领域 14 件叠加）。store/market 与 HTTP API 同一份句柄——
    /// 工具数字与端点数字同源（P4-T6 纪律）。
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
        if auto_approve {
            // 诊断而非警告：用户拍板（ADR-0014）。重开确认 = config 翻 false + 先补 C13。
            tracing::info!("工具确认已关闭，自动执行（ADR-0014 拍板；重开须先补 docs/07 C13）");
        }

        // Skill 模板先行（LoadSkillTool 构造需要 Arc<SkillRegistry>）：
        // 仓库根 skills/，ZIMU_SKILLS_DIR 可覆盖；坏文件逐条上报不致命。
        let (loaded, errors) = load_zimu_skills();
        for e in &errors {
            tracing::warn!("Skill 加载失败（跳过该文件）：{e}");
        }
        let skill_names: Vec<String> =
            loaded.iter().map(|s| s.frontmatter.name.clone()).collect();
        // tools_required 校验名单 = 领域 14 + 标准面可能名（warn-only，容条件注册缺席）
        let mut available: Vec<&str> = DOMAIN_TOOL_NAMES.to_vec();
        available.extend(STANDARD_TOOL_NAMES);
        let skills = Arc::new(SkillRegistry::new(loaded, &available));

        // robit 标准工具面（ADR-0014）：与 bootstrap 同内核，skill 主权留子木。
        // standard_tools=None → 回退 app.enabled_tools（未设 = 全注，robit 官方语义）。
        let mut registry = create_tools_from_config_with(&config, skills.clone(), None);

        // read_image 按需补注册：内核因主模型 supports_images=true 强制跳过
        // （bootstrap.rs:361-371），独立 vision 描述与 read 图像注入互补而非冗余。
        let ctx_cfg = config.app.as_ref().and_then(|a| a.context.as_ref());
        let max_bytes = ctx_cfg.and_then(|c| c.max_output_bytes).unwrap_or(51200);
        let max_dim = ctx_cfg.and_then(|c| c.max_image_dimension).unwrap_or(1024);
        match resolve_vision_model(&config) {
            Ok(resolved) => {
                registry.register(ReadImageTool::new(
                    LlmClient::from_resolved(resolved),
                    max_dim,
                    max_bytes,
                ));
                tracing::info!("read_image 按需注册（default_vision_model 已解析）");
            }
            Err(e) => tracing::debug!("read_image 未注册（vision 模型未配置或解析失败）：{e}"),
        }

        // robit 自持会话库初始化（幂等，ADR-0014）：Web 通道从不自建库，不补则
        // search_history 报 no such table（QQ 由 ChatbotManager 自建）。库语义：
        // robit.db 存 IM 会话档案，Web 会话档案在 zimu.db（ADR-0011 通道独立）。
        let global_storage = config
            .app
            .as_ref()
            .and_then(|a| a.global_storage)
            .unwrap_or(false);
        match resolve_db_path(&working_dir, global_storage) {
            Ok(db_path) => {
                if let Some(parent) = db_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match rusqlite::Connection::open(&db_path) {
                    Ok(conn) => {
                        if let Err(e) = init_db(&conn) {
                            tracing::warn!("robit 会话库初始化失败（search_history 可能不可用）：{e}");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("robit 会话库打开失败（search_history 可能不可用）：{e}");
                    }
                }
            }
            Err(e) => tracing::warn!("robit 会话库路径解析失败：{e}"),
        }

        // 领域 14 件叠加（写账本五件自动落账纪律见 tools/mod.rs 头与 ADR-0012/0013）
        let deps = AgentDeps::new(store, market);
        register_domain_tools(&mut registry, deps);
        let mut tool_names: Vec<String> =
            registry.tool_names().iter().map(|s| s.to_string()).collect();
        tool_names.sort();
        let tools = Arc::new(registry);

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
