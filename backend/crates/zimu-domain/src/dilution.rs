//! 摊薄成本、已实现盈亏与调整后仓位（03 §4）：抽屉试算的纯函数形态。

use serde::{Deserialize, Serialize};

use crate::fees::{fee, FeeInput};
use crate::{Kind, Side, THRESHOLD_EPS};

/// 单票占组合上限 %（03 §6 第一条；试算超限判定用它）
pub const SINGLE_STOCK_LIMIT: f64 = 25.0;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ApplyTradeInput {
    pub kind: Kind,
    pub side: Side,
    pub qty: f64,
    pub price: f64,
    /// 当前持仓数量
    pub held: f64,
    /// 当前摊薄成本（无持仓传 0）
    pub cost: f64,
    /// 当前现金
    pub cash: f64,
    /// 其他持仓当前市值合计（原型取快照，不随本笔价格变化）
    pub others_mv: f64,
}

#[derive(Debug, Serialize)]
pub struct TradeOutcome {
    pub amount: f64,
    pub fee: f64,
    pub after_shares: f64,
    /// 清仓（after_shares = 0）时持仓行移除 → null（03 附录约定 3）
    pub after_cost: Option<f64>,
    pub cash_after: f64,
    /// cash_after < 0（原型：数字标绿并在提交判定中拦截，不弹错误框）
    pub insufficient_cash: bool,
    /// 仅卖出且有成本基准时有值
    pub realized: Option<f64>,
    /// realized / (cost × qty) × 100
    pub realized_pct: Option<f64>,
    /// others_mv + after_shares×price + cash_after（分母随本笔交易变化）
    pub ta_after: f64,
    pub weight_after: f64,
    /// weight_after > 25%（压线含边界，THRESHOLD_EPS 守卫）
    pub over_limit: bool,
}

/// 试算一笔交易（原型 `holdings.html:352`）。买入费用计入摊薄成本；卖出不改成本。
pub fn apply_trade(inp: &ApplyTradeInput) -> TradeOutcome {
    let breakdown = fee(FeeInput {
        kind: inp.kind,
        side: inp.side,
        qty: inp.qty,
        price: inp.price,
    });
    let amount = breakdown.amount;
    let f = breakdown.total;

    let after_shares = match inp.side {
        Side::Buy => inp.held + inp.qty,
        Side::Sell => (inp.held - inp.qty).max(0.0),
    };
    let cash_after = match inp.side {
        Side::Buy => inp.cash - amount - f,
        Side::Sell => inp.cash + amount - f,
    };
    let insufficient_cash = cash_after < 0.0;

    let after_cost = match inp.side {
        // 费用计入成本；held=0 新建仓即 (amount+f)/qty
        Side::Buy if after_shares > 0.0 => Some((inp.cost * inp.held + amount + f) / after_shares),
        Side::Buy => None,
        Side::Sell if after_shares > 0.0 => Some(inp.cost),
        Side::Sell => None,
    };

    let (realized, realized_pct) = if inp.side == Side::Sell && inp.cost > 0.0 {
        let r = (inp.price - inp.cost) * inp.qty - f;
        (Some(r), Some(r / (inp.cost * inp.qty) * 100.0))
    } else {
        (None, None)
    };

    let ta_after = inp.others_mv + after_shares * inp.price + cash_after;
    let weight_after = if ta_after != 0.0 {
        after_shares * inp.price / ta_after * 100.0
    } else {
        0.0
    };
    let over_limit = weight_after > SINGLE_STOCK_LIMIT + THRESHOLD_EPS;

    TradeOutcome {
        amount,
        fee: f,
        after_shares,
        after_cost,
        cash_after,
        insufficient_cash,
        realized,
        realized_pct,
        ta_after,
        weight_after,
        over_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sell_keeps_cost_and_realizes() {
        let out = apply_trade(&ApplyTradeInput {
            kind: Kind::Stock,
            side: Side::Sell,
            qty: 100.0,
            price: 25.0,
            held: 200.0,
            cost: 20.0,
            cash: 6.275,
            others_mv: 0.0,
        });
        assert_eq!(out.after_cost, Some(20.0));
        assert!((out.realized.unwrap() - 493.725).abs() < 1e-9);
    }
}
