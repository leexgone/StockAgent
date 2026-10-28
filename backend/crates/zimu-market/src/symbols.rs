//! symbol 拼装（B7，docs/04 附录 B）：前缀只能来自**显式 `mkt` 字段**，
//! 不做号段猜测——sidecar 对裸代码一律 400 `BAD_SYMBOL`，拼错前缀的责任在调用方，
//! 所以这里宁缺毋滥：拼不出合法前缀就返回 None，让上层拒绝而不是发出去挨 400。

/// `mkt`（SH/SZ，大小写不敏感）+ 6 位代码 → sidecar symbol（`sh600519`）。
/// 北交所（BJ）超出产品范围（03 §2），与空值/异形一律 None。
pub fn symbol_of(mkt: &str, code: &str) -> Option<String> {
    let prefix = match mkt.trim().to_ascii_uppercase().as_str() {
        "SH" => "sh",
        "SZ" => "sz",
        _ => return None,
    };
    let c = code.trim();
    if c.len() != 6 || !c.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("{prefix}{c}"))
}

#[cfg(test)]
mod tests {
    use super::symbol_of;

    #[test]
    fn explicit_prefix_only() {
        assert_eq!(symbol_of("SH", "600519").as_deref(), Some("sh600519"));
        assert_eq!(symbol_of("SZ", "128136").as_deref(), Some("sz128136"));
        assert_eq!(symbol_of("sh", "510300").as_deref(), Some("sh510300")); // 大小写不敏感
        assert_eq!(symbol_of(" SH ", " 000001 ").as_deref(), Some("sh000001")); // 容忍空白
    }

    #[test]
    fn rejects_out_of_scope_and_malformed() {
        assert!(symbol_of("BJ", "920001").is_none()); // 北交所超出产品范围
        assert!(symbol_of("", "600519").is_none());
        assert!(symbol_of("SH", "60051").is_none()); // 5 位
        assert!(symbol_of("SH", "6005199").is_none()); // 7 位
        assert!(symbol_of("SH", "60051a").is_none()); // 非数字
    }
}
