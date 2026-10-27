//! 仓位纪律四条与降仓卖出数量（03 §6）。
//! 四条规则分母不统一（务必照抄）：单票/转债/现金 → total_assets（含现金），行业 → total_mv（不含现金）。

use serde::{Deserialize, Serialize};

use crate::recalc::{Holding, PortfolioInput};
use crate::{ratio, Kind, THRESHOLD_EPS};

pub const LIMIT_SINGLE_STOCK: f64 = 25.0;
pub const LIMIT_SECTOR: f64 = 35.0;
pub const LIMIT_CB_TOTAL: f64 = 12.0;
pub const LIMIT_CASH_RESERVE: f64 = 10.0;

#[derive(Debug, Serialize)]
pub struct Rule {
    pub value: f64,
    pub threshold: f64,
    pub pass: bool,
}

#[derive(Debug, Serialize)]
pub struct DisciplineReport {
    pub single_stock: Rule,
    pub sector: Rule,
    pub cb_total: Rule,
    pub cash_reserve: Rule,
}

/// 上限类规则：value ≤ threshold 为 pass（压线算安全，ε 守卫防浮点噪声）
fn at_most(value: f64, threshold: f64) -> Rule {
    Rule {
        value,
        threshold,
        pass: value <= threshold + THRESHOLD_EPS,
    }
}

/// 下限类规则（现金保留）：value ≥ threshold 为 pass
fn at_least(value: f64, threshold: f64) -> Rule {
    Rule {
        value,
        threshold,
        pass: value >= threshold - THRESHOLD_EPS,
    }
}

/// 纪律四条体检（原型 `holdings.html:419` RULES）。空持仓时前三条 value = 0。
pub fn check(inp: &PortfolioInput) -> DisciplineReport {
    let mv_of = |h: &Holding| h.price * h.shares;
    let total_mv: f64 = inp.holdings.iter().map(mv_of).sum();
    let total_assets = total_mv + inp.cash;

    let max_single = inp
        .holdings
        .iter()
        .map(mv_of)
        .fold(0.0, f64::max);
    let single_stock = at_most(ratio(max_single, total_assets) * 100.0, LIMIT_SINGLE_STOCK);

    // 单一行业占市值（分母 total_mv，不含现金）
    let mut grouped: Vec<(String, f64)> = Vec::new();
    for h in &inp.holdings {
        let mv = mv_of(h);
        match grouped.iter_mut().find(|(name, _)| name == &h.sector) {
            Some((_, v)) => *v += mv,
            None => grouped.push((h.sector.clone(), mv)),
        }
    }
    let max_sector = grouped.into_iter().map(|(_, v)| v).fold(0.0, f64::max);
    let sector = at_most(ratio(max_sector, total_mv) * 100.0, LIMIT_SECTOR);

    let cb_mv: f64 = inp
        .holdings
        .iter()
        .filter(|h| h.kind == Kind::Cb)
        .map(mv_of)
        .sum();
    let cb_total = at_most(ratio(cb_mv, total_assets) * 100.0, LIMIT_CB_TOTAL);

    let cash_reserve = at_least(ratio(inp.cash, total_assets) * 100.0, LIMIT_CASH_RESERVE);

    DisciplineReport {
        single_stock,
        sector,
        cb_total,
        cash_reserve,
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct SellToInput {
    pub mv: f64,
    pub price: f64,
    pub total_assets: f64,
    /// 目标仓位 %
    pub target: f64,
    /// 手数（03 §1；公式已由写死 100 泛化，见 03 附录约定 1）
    pub lot: i64,
}

#[derive(Debug, Serialize)]
pub struct SellToOutput {
    pub qty: i64,
}

/// 降到目标仓位的卖出数量：`max(lot, ceil((mv − TA×target/100) / price / lot) × lot)`。
/// 已在目标线下时仍返回最小一手（max 语义：卖就至少一手，是否真要卖由调用方判断）。
/// 退化输入（price ≤ 0 或 lot ≤ 0）返回最小一手并由此兜底，不产生除零。
pub fn sell_to(inp: &SellToInput) -> SellToOutput {
    if inp.price <= 0.0 || inp.lot <= 0 {
        return SellToOutput {
            qty: inp.lot.max(1),
        };
    }
    let excess = inp.mv - inp.total_assets * inp.target / 100.0;
    let shares = excess / inp.price;
    let lots = (shares / inp.lot as f64).ceil() as i64;
    SellToOutput {
        qty: (lots * inp.lot).max(inp.lot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_portfolio_passes_upper_rules() {
        let r = check(&PortfolioInput {
            cash: 50000.0,
            holdings: vec![],
        });
        assert!(r.single_stock.pass && r.sector.pass && r.cb_total.pass);
        assert_eq!(r.cash_reserve.value, 100.0);
        assert!(r.cash_reserve.pass);
    }
}
