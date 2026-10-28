//! Skill 模板加载（P4-T6）。
//!
//! 模板放**仓库根 `skills/`**（每目录一个 SKILL.md，robit 约定：YAML frontmatter
//! + Markdown 正文；正文在用户 `/触发词` 命中时以临时 system message 注入本轮，
//! 见 robit-agent skill 模块）。不放 `<workdir>/.robit/skills/` 的原因：`.robit/`
//! 整目录被 .gitignore（robit 运行时会话库），而 Skill 模板是**产品文案资产**，
//! 必须入库、随代码评审——内嵌 `03` 口径引用与免责边界（roadmap P4-T6 验收项）。
//!
//! 目录解析：`ZIMU_SKILLS_DIR` 覆盖；默认按编译期 manifest 目录定位到
//! `仓库根/skills`（backend/crates/zimu-agent → ../../..），与 zimu-server 定位
//! `frontend/dist` 同款——从任何工作目录启动都能找到。

use std::path::PathBuf;

use robit_agent::skill::{load_skills, SkillLoadError};
use robit_agent::Skill;

/// Skill 模板目录（`ZIMU_SKILLS_DIR` 覆盖；默认仓库根 `skills/`）
pub fn skills_dir() -> PathBuf {
    std::env::var("ZIMU_SKILLS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../skills"))
}

/// 加载全部子木 Skill（项目目录单源，不叠 `~/.robit/skills` 全局目录——
/// 产品模板以仓库为准，个人全局模板不进服务）。错误非致命：坏的 SKILL.md
/// 逐条上报，其余照常加载。
pub fn load_zimu_skills() -> (Vec<Skill>, Vec<SkillLoadError>) {
    load_skills(None, Some(skills_dir()))
}
