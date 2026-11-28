//! T8 人格资产守护（ADR-0011）：子木主人格提示词在库、可被 robit 命中、忽略面正确。
//!
//! robit 的提示词搜索是**文件层**（PromptBuilder::with_working_dir，robit-agent
//! agent.rs:159/217）：`{working_dir}/.robit/prompts/agent.md` → 全局 → 内置默认。
//! zimu-server / zimu-agent-cli 的 working_dir 都是启动 cwd（backend/），所以
//! 人格文件必须躺在 `backend/.robit/prompts/agent.md`——本测试锁死这个几何关系，
//! 挪目录/漏提交/误忽略都会在这里红。

use std::path::{Path, PathBuf};

/// 相对 crate manifest 定位（不依赖测试运行时的 cwd）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2) // crates/zimu-server → crates → backend
        .expect("backend workspace root")
        .to_path_buf()
}

#[test]
fn persona_prompt_present_and_opens_with_zimu_identity() {
    let md = repo_root().join(".robit/prompts/agent.md");
    let text = std::fs::read_to_string(&md).unwrap_or_else(|e| {
        panic!(
            "缺少子木主人格提示词 {md:?}——robit 会静默回落到内置「Robit 代码打工」人格（T8 拍板禁止）：{e}"
        )
    });
    assert!(
        text.trim_start().starts_with("你是子木"),
        "人格提示词必须以子木身份开头（当前开头：{:?}）",
        text.trim_start().chars().take(24).collect::<String>()
    );
    // 边界硬规则的最小断言：与 CLAUDE.md「安全与责任边界」同条纪律的关键句
    for must in [
        "永不代下单",
        "不是胜率",
        "失效条件",
        "只增不改",
        "record_trade",
        "整行覆盖",
        "remove_holding",
        // ADR-0014 文件与 shell 纪律（自动执行语境，软约束是唯一防线）
        "bash",
        ".env",
        "直写",
        // ADR-0022 计划任务纪律（仅 IM 生效 + 自主回合授权不扩张）
        "计划任务",
        "自主回合",
        // ADR-0024 仓位纪律不进智能体（取代 0023 的「参考线」锚——判断与表述链中立）
        "不进你的判断与表述链",
    ] {
        assert!(text.contains(must), "人格提示词缺关键边界句：{must}");
    }
}

#[test]
fn gitignore_excludes_runtime_but_tracks_persona() {
    let ignore = std::fs::read_to_string(
        repo_root()
            .parent()
            .expect("repo root（StockAgent/）")
            .join(".gitignore"),
    )
    .expect("仓库根 .gitignore 必须存在");
    assert!(
        ignore.contains("**/.robit/*"),
        ".robit 运行时产物必须是内容级排除（目录级 `.robit/` 会挡住人格豁免）"
    );
    assert!(
        ignore.contains("!**/.robit/prompts/agent.md"),
        ".gitignore 必须保留人格提示词的入库例外"
    );
    assert!(
        !ignore.lines().any(|l| l.trim() == ".robit/"),
        "禁止回退为目录级 `.robit/` 排除"
    );
}
