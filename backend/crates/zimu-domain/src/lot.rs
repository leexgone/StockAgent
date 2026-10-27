//! 标的属性与交易单位（03 §1）：手数、报价小数位、数量校验、快捷档位。

use serde::{Deserialize, Serialize};

use crate::{Kind, Side};

#[derive(Deserialize)]
pub struct KindInput {
    pub kind: Kind,
}

#[derive(Serialize)]
pub struct LotOutput {
    pub lot: i64,
}

#[derive(Serialize)]
pub struct DecimalsOutput {
    pub decimals: u32,
}

/// 1 手 = 多少股/份/张（03 §1 表格）
pub fn lot_size(kind: Kind) -> i64 {
    match kind {
        Kind::Stock => 100,
        Kind::Etf => 100,
        Kind::Cb => 10,
    }
}

/// 报价小数位（03 §1 表格；B4 已随 P1-T3 修复——原型统一走 data.js `priceDec(kind)`）
pub fn price_decimals(kind: Kind) -> u32 {
    match kind {
        Kind::Stock => 2,
        Kind::Etf => 3,
        Kind::Cb => 3,
    }
}

/// 数量校验失败原因（枚举码进 fixture；中文文案是界面层职责，见 05）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QtyReason {
    NotLotMultiple,
    ExceedsHeld,
}

#[derive(Debug, Serialize)]
pub struct QtyVerdict {
    pub valid: bool,
    pub reason: Option<QtyReason>,
}

#[derive(Deserialize)]
pub struct ValidateQtyInput {
    pub kind: Kind,
    pub side: Side,
    pub qty: i64,
    pub held: i64,
}

/// 数量校验（原型 `holdings.html:342`）：先整手倍数、后卖出不得超持仓。
/// `qty == 0` 视为「未填写」返回 valid（提交闸门另行要求 qty > 0，与原型 `qty &&` 短路一致）；
/// 负数按非法输入拒绝。奇零股卖出与 §1「可卖整数股」的冲突待 B12 定稿。
pub fn validate_qty(kind: Kind, side: Side, qty: i64, held: i64) -> QtyVerdict {
    let invalid = |reason| QtyVerdict {
        valid: false,
        reason: Some(reason),
    };
    if qty < 0 {
        return invalid(QtyReason::NotLotMultiple);
    }
    if qty == 0 {
        return QtyVerdict {
            valid: true,
            reason: None,
        };
    }
    let lot = lot_size(kind);
    if qty % lot != 0 {
        return invalid(QtyReason::NotLotMultiple);
    }
    if side == Side::Sell && qty > held {
        return invalid(QtyReason::ExceedsHeld);
    }
    QtyVerdict {
        valid: true,
        reason: None,
    }
}

#[derive(Deserialize)]
pub struct QuickTiersInput {
    pub kind: Kind,
    pub side: Side,
    pub held: i64,
}

#[derive(Serialize)]
pub struct TiersOutput {
    pub tiers: Vec<i64>,
}

/// 快捷档位（原型 `holdings.html:334`）：基准 = max(1 手, 持仓；无持仓按 10 手)，
/// 10% / 25% / 50% 向下取整到手、每档至少 1 手；卖出且有持仓时追加「全部清仓」。
/// 用整数除法实现向下取整（floor(floor(x/100)/lot) ≡ floor(x/(100·lot))，与原型浮点 floor 等价且无噪声）。
pub fn quick_tiers(kind: Kind, side: Side, held: i64) -> Vec<i64> {
    let lot = lot_size(kind);
    let base = lot.max(if held > 0 { held } else { lot * 10 });
    let mut tiers: Vec<i64> = [10, 25, 50]
        .iter()
        .map(|pct| lot.max(base * pct / 100 / lot * lot))
        .collect();
    if side == Side::Sell && held > 0 {
        tiers.push(held);
    }
    tiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lots_and_decimals() {
        assert_eq!(lot_size(Kind::Cb), 10);
        assert_eq!(price_decimals(Kind::Etf), 3);
    }

    #[test]
    fn zero_qty_is_unfilled_not_invalid() {
        let v = validate_qty(Kind::Stock, Side::Buy, 0, 0);
        assert!(v.valid && v.reason.is_none());
    }
}
