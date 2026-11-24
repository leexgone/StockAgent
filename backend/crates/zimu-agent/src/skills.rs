//! Skill 播种与加载（P4-T6；目录策略 ADR-0021）。
//!
//! **加载目录 = `<working_dir>/.robit/skills`**（与 robit 项目级技能目录约定逐字
//! 同款，robit bootstrap.rs `working_dir.join(".robit/skills")`）：系统技能播种副本
//! 与用户自增技能并存，单源加载该目录。容器里 working_dir=/data，技能落命名卷；
//! 开发期 = `backend/.robit/skills`（`.robit/` 整目录被 gitignore，用户技能不入库）。
//!
//! **播种源 = 仓库根 `skills/`**（`ZIMU_SKILLS_DIR` 覆盖；缺省按编译期 manifest
//! 目录定位 backend/crates/zimu-agent → ../../..，与 zimu-server 定位 `frontend/dist`
//! 同款——从任何工作目录启动都能找到；容器烘焙 `/app/skills`）。系统模板是**产品
//! 文案资产**（内嵌 `03` 口径引用与免责边界），唯一真值仍在仓库、入库随代码评审
//! ——原单源政策的理由经「播种源」保留（ADR-0021 D4）。
//!
//! **播种语义（ADR-0021 D3，主人拍板）**：应用初始化（AgentCore 装配）时缺则播种
//! 永不覆盖，粒度 = 单个技能子目录（按目录名）——目标同名目录存在即跳过（用户
//! 修改保留），缺失才整目录复制；skills 根目录已存在且含用户技能时仍逐个补缺
//! 系统技能。用户自增目录（播种源无同名）永不碰，任何文件不自动删除。复制走
//! 临时同级目录 + rename（防半截播种锁死；并发双实例后到者见目标已存在视为成功）。
//! 停用技能用 frontmatter `enabled: false`（持久保留）；删除系统技能目录下次启动
//! 会重播种；上游退役的系统技能残留直至手删（已知局限）。

use std::path::{Path, PathBuf};

use robit_agent::skill::{load_skills_from_dirs, SkillLoadError};
use robit_agent::Skill;

/// 系统技能播种源（`ZIMU_SKILLS_DIR` 覆盖；默认仓库根 `skills/`；容器烘焙 /app/skills）
pub fn skills_dir() -> PathBuf {
    std::env::var("ZIMU_SKILLS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../skills"))
}

/// 技能加载目录 = `<working_dir>/.robit/skills`（robit 项目级约定，ADR-0021 D1）
pub fn runtime_skills_dir(working_dir: &Path) -> PathBuf {
    working_dir.join(".robit/skills")
}

/// 把系统技能播种进技能目录（包装：播种源 = `skills_dir()`）。
/// 错误非致命：逐目录 warn，不阻断装配。
pub fn seed_system_skills(working_dir: &Path) {
    seed_system_skills_from(&skills_dir(), &runtime_skills_dir(working_dir));
}

/// 路径参数化播种（可测形态）：枚举源子目录（排序保证确定性）→ 目标同名目录
/// **存在即跳过**（缺则播种永不覆盖，ADR-0021 D3），缺失才整目录复制。
/// 比对粒度是技能的具体子目录，不是 skills 根目录——根目录已含用户技能时
/// 仍逐个补缺系统技能；用户自增目录（源里无同名）永不碰。
pub fn seed_system_skills_from(source: &Path, runtime: &Path) {
    if !source.is_dir() {
        tracing::warn!(
            "Skill 播种源不存在，跳过播种（系统技能如实降级缺席）：{}",
            source.display()
        );
        return;
    }
    let entries = match std::fs::read_dir(source) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Skill 播种源读取失败，跳过播种：{}：{e}", source.display());
            return;
        }
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    for skill_dir in dirs {
        let name = match skill_dir.file_name() {
            Some(n) => n,
            None => continue,
        };
        let target = runtime.join(name);
        if target.exists() {
            continue;
        }
        if let Err(e) = seed_one(&skill_dir, &target) {
            tracing::warn!(
                "Skill 播种失败（跳过该目录）：{} → {}：{e}",
                skill_dir.display(),
                target.display()
            );
        }
    }
}

/// 单个技能目录播种：复制到临时同级目录（名含 pid）再 rename——半截复制不会
/// 留下「目标已存在但内容不全」的锁死态（失败清理临时目录，下次启动重试）。
/// rename 失败但目标已存在 = 另一进程已播种，视为成功（并发双实例友好）。
fn seed_one(source: &Path, target: &Path) -> std::io::Result<()> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "技能目标目录无父目录")
    })?;
    std::fs::create_dir_all(parent)?;
    let name = target
        .file_name()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "技能目标目录无名称")
        })?
        .to_string_lossy();
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    // 清理上次崩溃可能残留的临时目录（同名 pid 复用极罕见，防御性）
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    copy_dir_all(source, &tmp)?;
    match std::fs::rename(&tmp, target) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            if target.exists() {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

/// 递归整目录复制（SKILL.md + 未来附属文件；std 无递归复制）。
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 加载全部子木 Skill（播种 + 加载，ADR-0021）：先把系统技能播种进
/// `<wd>/.robit/skills`，再单源加载该目录（系统已播种 + 用户自增）。
/// robit 0.1.27 起走官方「额外技能目录」入口 `load_skills_from_dirs`（R4），
/// 不叠 `~/.robit/skills` 全局目录（个人全局模板不进服务）。
/// 错误非致命：坏的 SKILL.md 逐条上报，其余照常加载。
pub fn load_zimu_skills(working_dir: &Path) -> (Vec<Skill>, Vec<SkillLoadError>) {
    seed_system_skills(working_dir);
    load_skills_from_dirs(std::slice::from_ref(&runtime_skills_dir(working_dir)))
}