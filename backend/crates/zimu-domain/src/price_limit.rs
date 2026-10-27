//! 价格合法性（涨跌停校验，03 §2）：比例推断、涨停/跌停价计算、委托价校验。
//! P1-T2 定稿（2026-10-28，B2 消除）：显示与拦截用同一对限价（含边界），
//! 旧原型 0.1% 容差废除；限价按整数单元四舍五入，消除浮点噪声。

use serde::{Deserialize, Serialize};

use crate::lot::price_decimals;
use crate::{Kind, THRESHOLD_EPS};

#[derive(Deserialize)]
pub struct LimitPctInput {
    pub kind: Kind,
    pub code: String,
    pub st: bool,
}

#[derive(Serialize)]
pub struct LimitPctOutput {
    pub pct: Option<i64>,
}

/// 涨跌幅比例（03 §2 表）：股票按代码前缀（沪深主板 10 / ST 5、创业板与科创板 20，
/// 二者无 ST 制度，`st` 被忽略），转债（11/12 开头）一律 20；ETF 无法由代码推断
/// （159915 创业板 ETF ±20% 与 159919 沪深300ETF ±10% 同前缀）→ None，由行情源/
/// 数据层显式配置；北交所（±30%）超出产品范围 → None。ST 识别依赖行情字段
/// （P4 填充），不做名称字符串猜测。
pub fn limit_pct(kind: Kind, code: &str, st: bool) -> Option<i64> {
    match kind {
        Kind::Stock => {
            if code.starts_with("68") {
                Some(20) // 科创板（688/689）
            } else if code.starts_with("30") {
                Some(20) // 创业板（300/301/302）
            } else if code.starts_with("60") || code.starts_with("00") {
                // 沪深主板（002/003 已并入深主板）
                if st {
                    Some(5)
                } else {
                    Some(10)
                }
            } else {
                None // 北交所（8/4/920 开头）等：超出范围
            }
        }
        // 沪 110/111/113/118，深 123/127/128
        Kind::Cb => {
            if code.starts_with("11") || code.starts_with("12") {
                Some(20)
            } else {
                None
            }
        }
        Kind::Etf => None,
    }
}

#[derive(Deserialize)]
pub struct LimitPricesInput {
    pub kind: Kind,
    pub prev: f64,
    pub pct: i64,
}

#[derive(Serialize)]
pub struct LimitPricesOutput {
    pub limit_up: Option<f64>,
    pub limit_down: Option<f64>,
}

/// 整数单元四舍五入（half-up）：floor((units × (100 ± pct) + 50) / 100)。
/// 与 TS 侧同一表达式逐位一致；全整数运算规避 double 噪声
/// （裸浮点 9.85 × 1.1 = 10.834999…，按小数 half-up 会错得 10.83）。
fn limit_units(units: f64, pct: f64, up: bool) -> f64 {
    let factor = if up { 100.0 + pct } else { 100.0 - pct };
    ((units * factor + 50.0) / 100.0).floor()
}

/// 涨停/跌停价（03 §2）：prev × (1 ± pct/100) 按 kind 报价小数位（§1）四舍五入，
/// 与交易所算法一致。prev ≤ 0（停牌/无行情，NaN 同）或 pct ≤ 0（未配置）→ 双 None，
/// 界面显示 — 并跳过区间拦截。
pub fn limit_prices(kind: Kind, prev: f64, pct: i64) -> LimitPricesOutput {
    if !(prev > 0.0) || pct <= 0 {
        return LimitPricesOutput {
            limit_up: None,
            limit_down: None,
        };
    }
    let decimals = price_decimals(kind);
    let scale = 10f64.powi(decimals as i32);
    // prev 是合法报价（≤ decimals 位小数），round 精确恢复整数单元（分/厘）
    let units = (prev * scale).round();
    let pct = pct as f64;
    LimitPricesOutput {
        limit_up: Some(limit_units(units, pct, true) / scale),
        limit_down: Some(limit_units(units, pct, false) / scale),
    }
}

/// 委托价校验失败原因（枚举码进 fixture；中文文案是界面层职责，见 05）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceReason {
    AboveLimitUp,
    BelowLimitDown,
    InvalidPrice,
}

#[derive(Deserialize)]
pub struct ValidatePriceInput {
    pub price: f64,
    pub limit_up: f64,
    pub limit_down: f64,
}

#[derive(Debug, Serialize)]
pub struct PriceVerdict {
    pub valid: bool,
    pub reason: Option<PriceReason>,
}

/// 委托价校验（03 §2）：limit_down ≤ price ≤ limit_up 合法，含边界（压线可成交
/// 是交易所语义）；显示与拦截必须用同一对限价。ε 守卫防浮点噪声翻转边界
/// （03 附录比对规则第 5 条）；price ≤ 0（或 NaN）→ InvalidPrice。
pub fn validate_price(price: f64, limit_up: f64, limit_down: f64) -> PriceVerdict {
    let invalid = |reason| PriceVerdict {
        valid: false,
        reason: Some(reason),
    };
    if !(price > 0.0) {
        return invalid(PriceReason::InvalidPrice);
    }
    if price > limit_up + THRESHOLD_EPS {
        return invalid(PriceReason::AboveLimitUp);
    }
    if price < limit_down - THRESHOLD_EPS {
        return invalid(PriceReason::BelowLimitDown);
    }
    PriceVerdict {
        valid: true,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pct_by_board() {
        assert_eq!(limit_pct(Kind::Stock, "600519", false), Some(10));
        assert_eq!(limit_pct(Kind::Stock, "600519", true), Some(5));
        assert_eq!(limit_pct(Kind::Stock, "300750", true), Some(20)); // 创业板无 ST 制度
        assert_eq!(limit_pct(Kind::Cb, "113650", false), Some(20));
        assert_eq!(limit_pct(Kind::Etf, "510300", false), None); // 无法由代码推断
        assert_eq!(limit_pct(Kind::Stock, "833171", false), None); // 北交所超范围
    }

    #[test]
    fn half_up_matches_exchange() {
        // 9.85 × 1.1 = 10.835 → 10.84；9.85 × 0.9 = 8.865 → 8.87
        //（裸浮点是 10.834999… → 会错得 10.83，整数单元运算无噪声）
        let r = limit_prices(Kind::Stock, 9.85, 10);
        assert_eq!(r.limit_up, Some(10.84));
        assert_eq!(r.limit_down, Some(8.87));
        // prev ≤ 0（停牌/无行情）→ None
        let n = limit_prices(Kind::Stock, 0.0, 10);
        assert!(n.limit_up.is_none() && n.limit_down.is_none());
    }
}
