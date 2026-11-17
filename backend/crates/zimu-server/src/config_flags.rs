//! 通道启用开关（T5/ADR-0011 拍板⑥）：raw 解析工程本地 config.toml 的
//! `[zimu.channels]` 节——**缺省 = 停用（fail-closed）**，未启用的通道不构造
//! adapter / ChatbotManager / 任何该通道 Agent。
//!
//! 为什么读原始 TOML：robit 的类型化 `RobitConfig` 不认识子木自有节（serde
//! 忽略未知字段），同一份文件两处消费互不冲突——robit 吃它的
//! `[providers]/[channels]`，子木吃自己的 `[zimu]`。文件搜索顺序复刻 robit
//! `find_config_path`（robit-ai config.rs:472-502，首个命中即整体接管）：
//! `{workdir}/.robit/config.toml` → `{cwd}/.robit/config.toml` → `~/.robit/config.toml`。

use std::path::{Path, PathBuf};

/// 通道开关位。新通道到货时在 `ChannelFlags` 加字段（`wechat` 已随
/// ADR-0017 接线：true 且 Web Agent 在位 → wechat_channel supervisor 启动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelFlags {
    pub qq: bool,
    pub wechat: bool,
}

impl Default for ChannelFlags {
    fn default() -> Self {
        // 拍板⑥：缺省停用——没有 [zimu.channels] 就没有任何 IM 通道
        Self { qq: false, wechat: false }
    }
}

/// 定位子木实际消费的 config.toml（与 robit load_config 同序同语义）。
pub fn config_path(working_dir: &Path) -> Option<PathBuf> {
    let local = working_dir.join(".robit").join("config.toml");
    if local.is_file() {
        return Some(local);
    }
    if let Ok(cwd) = std::env::current_dir() {
        let by_cwd = cwd.join(".robit").join("config.toml");
        if by_cwd.is_file() {
            return Some(by_cwd);
        }
    }
    if let Some(home) = dirs_home() {
        let global = home.join(".robit").join("config.toml");
        if global.is_file() {
            return Some(global);
        }
    }
    None
}

/// 纯函数：从 raw TOML 文本提取开关（单测面——不碰文件系统）。
/// 坏 TOML / 缺节 / 非 bool 值一律回落到缺省停用；`Err` 只在文件本身读不到时由调用方处理。
pub fn parse_flags(raw: &str) -> ChannelFlags {
    // toml 1.2 的 `Value::from_str`（`.parse()`）走 ValueDeserializer 单值路径，
    // 解析整份文档（表头 + 键值）会报「unexpected content」——必须用 `toml::from_str`
    // 走文档反序列化（f2ea4d5 升 toml 1.2 后的适配，否则通道开关恒回落停用）。
    let Ok(doc) = toml::from_str::<toml::Value>(raw) else {
        tracing::warn!("config.toml 无法按 TOML 解析，通道开关按缺省（全停用）处理");
        return ChannelFlags::default();
    };
    read_flags(&doc)
}

fn read_flags(doc: &toml::Value) -> ChannelFlags {
    let mut f = ChannelFlags::default();
    let Some(channels) = doc.get("zimu").and_then(|z| z.get("channels")) else {
        return f; // 缺节 = 全停用（fail-closed）
    };
    // 只认 bool；字符串 "true"/数字 1 都不算启用（词汇严格，避免误开通道）
    if let Some(b) = channels.get("qq").and_then(|v| v.as_bool()) {
        f.qq = b;
    }
    if let Some(b) = channels.get("wechat").and_then(|v| v.as_bool()) {
        f.wechat = b;
    }
    f
}

/// 读取当前部署的通道开关；文件不存在/读失败 = 全停用（与「无 robit 配置 →
/// chat 503 照常起」同构的降级）。
pub fn load_flags(working_dir: &Path) -> ChannelFlags {
    match config_path(working_dir).and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(raw) => parse_flags(&raw),
        None => ChannelFlags::default(),
    }
}

/// 用 dirs 找 HOME（不新增 dirs 依赖：与 zimu 现有依赖面一致走环境变量族）。
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_section_means_all_disabled() {
        let f = parse_flags("[providers.qwen]\napi_key = \"${X}\"\n");
        assert_eq!(f, ChannelFlags::default());
        assert!(!f.qq && !f.wechat, "fail-closed：没有 [zimu.channels] 就没有通道");
    }

    #[test]
    fn explicit_booleans_are_honored() {
        let f = parse_flags("[zimu.channels]\nqq = true\nwechat = false\n");
        assert!(f.qq && !f.wechat);
        let f = parse_flags("[zimu.channels]\nqq = true\nwechat = true\n");
        assert!(f.qq && f.wechat);
    }

    #[test]
    fn non_bool_values_do_not_enable() {
        // 词汇严格："true"/1 都不算开（防手滑把通道点亮）
        let f = parse_flags("[zimu.channels]\nqq = \"true\"\n");
        assert!(!f.qq);
        let f = parse_flags("[zimu.channels]\nqq = 1\n");
        assert!(!f.qq);
    }

    #[test]
    fn garbage_toml_falls_back_to_disabled() {
        let f = parse_flags("这不是 ][ TOML {{{");
        assert_eq!(f, ChannelFlags::default());
    }

    #[test]
    fn coexists_with_robit_sections_untouched() {
        // 同一份真实形态文件：robit 的节不影响开关提取
        let f = parse_flags(
            r#"
default_model = "qwen/qwen3.7-plus"
[providers.qwen]
api_key = "${DASHSCOPE_API_KEY}"
[channels.qq_bot]
app_id = "${ZIMU_QQ_APP_ID}"
[zimu.channels]
qq = true
"#,
        );
        assert!(f.qq);
    }
}
