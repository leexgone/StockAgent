//! 逐笔派生与组合汇总（03 §5）：`recalc()` 的纯函数形态。
//! 所有派生值只出自这里；分母为 0 时各比率取 0，不产生 NaN。

use serde::{Deserialize, Serialize};

use crate::{ratio, Kind};

#[derive(Debug, Clone, Deserialize)]
pub struct Holding {
    pub code: String,
    pub kind: Kind,
    pub sector: String,
    pub shares: f64,
    pub cost: f64,
    pub price: f64,
    pub prev: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PortfolioInput {
    pub cash: f64,
    pub holdings: Vec<Holding>,
}

#[derive(Debug, Serialize)]
pub struct PerHolding {
    pub code: String,
    /// (price − prev) / prev × 100
    pub day_chg: f64,
    /// (price − prev) × shares
    pub day_val: f64,
    pub mv: f64,
    /// (price − cost) × shares
    pub pnl: f64,
    /// (price − cost) / cost × 100
    pub pnl_pct: f64,
    /// mv / total_assets × 100（含现金分母，03 §6 口径）
    pub weight: f64,
}

#[derive(Debug, Serialize)]
pub struct SectorAgg {
    pub name: String,
    /// 行业市值
    pub v: f64,
    /// 占 total_mv（不含现金分母）
    pub w: f64,
}

#[derive(Debug, Serialize)]
pub struct Agg {
    pub cash: f64,
    pub mv_stocks: f64,
    pub mv_etf: f64,
    pub mv_cb: f64,
    pub total_mv: f64,
    pub total_assets: f64,
    pub day_pnl: f64,
    /// 分母 = 昨日总资产（total_assets − day_pnl），别改成今日（03 §5 注）
    pub day_pnl_pct: f64,
    pub total_pnl: f64,
    pub cost_total: f64,
    pub total_pnl_pct: f64,
    pub exposure: f64,
    /// 按市值降序（排序是黄金向量期望的一部分）
    pub sectors: Vec<SectorAgg>,
    /// Σw² / 10000，归一 0–1
    pub hhi: f64,
    pub top3_sector: f64,
}

#[derive(Debug, Serialize)]
pub struct RecalcOutput {
    /// 与输入 holdings 同序对齐
    pub per: Vec<PerHolding>,
    pub agg: Agg,
}

/// 全量重算（原型 `data.js:82` recalc()）。
pub fn recalc(inp: &PortfolioInput) -> RecalcOutput {
    let mv_of = |h: &Holding| h.price * h.shares;

    let mv_stocks = sum_by(inp, Kind::Stock, mv_of);
    let mv_etf = sum_by(inp, Kind::Etf, mv_of);
    let mv_cb = sum_by(inp, Kind::Cb, mv_of);
    let total_mv = mv_stocks + mv_etf + mv_cb;
    let total_assets = total_mv + inp.cash;

    let per: Vec<PerHolding> = inp
        .holdings
        .iter()
        .map(|h| {
            let mv = mv_of(h);
            PerHolding {
                code: h.code.clone(),
                day_chg: ratio(h.price - h.prev, h.prev) * 100.0,
                day_val: (h.price - h.prev) * h.shares,
                mv,
                pnl: (h.price - h.cost) * h.shares,
                pnl_pct: ratio(h.price - h.cost, h.cost) * 100.0,
                weight: ratio(mv, total_assets) * 100.0,
            }
        })
        .collect();

    let day_pnl = per.iter().map(|p| p.day_val).sum::<f64>();
    let total_pnl = per.iter().map(|p| p.pnl).sum::<f64>();
    let cost_total = inp
        .holdings
        .iter()
        .map(|h| h.cost * h.shares)
        .sum::<f64>();

    // 行业聚合：首次出现顺序收集，再按市值稳定降序（与原型 Object.entries + sort 一致）
    let mut grouped: Vec<(String, f64)> = Vec::new();
    for h in &inp.holdings {
        let mv = mv_of(h);
        match grouped.iter_mut().find(|(name, _)| name == &h.sector) {
            Some((_, v)) => *v += mv,
            None => grouped.push((h.sector.clone(), mv)),
        }
    }
    let sectors: Vec<SectorAgg> = {
        let mut list: Vec<SectorAgg> = grouped
            .into_iter()
            .map(|(name, v)| SectorAgg {
                name,
                v,
                w: ratio(v, total_mv) * 100.0,
            })
            .collect();
        list.sort_by(|a, b| b.v.partial_cmp(&a.v).unwrap_or(std::cmp::Ordering::Equal));
        list
    };
    let hhi = sectors.iter().map(|s| s.w * s.w).sum::<f64>() / 10000.0;
    let top3_sector = sectors.iter().take(3).map(|s| s.w).sum::<f64>();

    RecalcOutput {
        per,
        agg: Agg {
            cash: inp.cash,
            mv_stocks,
            mv_etf,
            mv_cb,
            total_mv,
            total_assets,
            day_pnl,
            day_pnl_pct: ratio(day_pnl, total_assets - day_pnl) * 100.0,
            total_pnl,
            cost_total,
            total_pnl_pct: ratio(total_pnl, cost_total) * 100.0,
            exposure: ratio(total_mv, total_assets) * 100.0,
            sectors,
            hhi,
            top3_sector,
        },
    }
}

fn sum_by(inp: &PortfolioInput, kind: Kind, f: impl Fn(&Holding) -> f64) -> f64 {
    inp.holdings
        .iter()
        .filter(|h| h.kind == kind)
        .map(f)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_portfolio_has_no_nan() {
        let out = recalc(&PortfolioInput {
            cash: 50000.0,
            holdings: vec![],
        });
        assert_eq!(out.agg.total_assets, 50000.0);
        assert_eq!(out.agg.day_pnl_pct, 0.0);
        assert_eq!(out.agg.hhi, 0.0);
        assert!(out.per.is_empty());
    }
}
