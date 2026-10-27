//! 净值与风险指标（03 §7）：日收益取相邻差比；sd 为总体标准差（除以 n）；年化常数 244。
//! bench_vol / beta / 相关系数不在本模块：未收入 03 §7 且原型两处实现不一致，口径待定稿（07-backlog B11）。

use serde::{Deserialize, Serialize};

use crate::{SHARPE_EPS, TRADING_DAYS};

#[derive(Debug, Clone, Deserialize)]
pub struct PerfInput {
    /// 组合净值序列
    pub nav: Vec<f64>,
    /// 基准序列（已归一到同一起点）
    pub bench: Vec<f64>,
}

#[derive(Debug, Default, Serialize)]
pub struct PerfMetrics {
    /// 累计收益 %
    pub cum: f64,
    pub bench_cum: f64,
    /// 年化波动 % = sd × √244 × 100
    pub ann_vol: f64,
    /// 最大回撤 %（滚动前高）
    pub mdd: f64,
    pub bench_mdd: f64,
    /// (mean × 244) / (sd × √244 + 1e−9)，未扣无风险利率
    pub sharpe: f64,
    /// 跑赢基准的天数占比 %
    pub win_rate: f64,
}

fn returns(series: &[f64]) -> Vec<f64> {
    series
        .windows(2)
        .map(|w| if w[0] != 0.0 { w[1] / w[0] - 1.0 } else { 0.0 })
        .collect()
}

fn cum_pct(series: &[f64]) -> f64 {
    match series.first() {
        Some(&first) if first != 0.0 && series.len() >= 2 => {
            (series[series.len() - 1] / first - 1.0) * 100.0
        }
        _ => 0.0,
    }
}

fn max_drawdown_pct(series: &[f64]) -> f64 {
    let mut peak = f64::MIN;
    let mut mdd = 0.0f64;
    for &v in series {
        if v > peak {
            peak = v;
        }
        if peak > 0.0 {
            let dd = (v - peak) / peak;
            if dd < mdd {
                mdd = dd;
            }
        }
    }
    mdd * 100.0
}

/// 净值指标（原型 `data.js:161` perf()）。序列长度 < 2（无相邻收益）→ 全指标返回 0，
/// 不报错（03 附录约定 2）。nav 与 bench 长度不等时 win_rate 按重叠前缀配对（zip），分母仍是 nav 收益数。
pub fn perf(inp: &PerfInput) -> PerfMetrics {
    if inp.nav.len() < 2 {
        return PerfMetrics::default();
    }
    let r = returns(&inp.nav);
    let rb = returns(&inp.bench);
    let n = r.len() as f64;

    let mean = r.iter().sum::<f64>() / n;
    let var = r.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    let sd = var.sqrt();
    let sqrt_days = TRADING_DAYS.sqrt();

    let wins = r
        .iter()
        .zip(rb.iter())
        .filter(|(a, b)| *a > *b)
        .count() as f64;

    PerfMetrics {
        cum: cum_pct(&inp.nav),
        bench_cum: cum_pct(&inp.bench),
        ann_vol: sd * sqrt_days * 100.0,
        mdd: max_drawdown_pct(&inp.nav),
        bench_mdd: max_drawdown_pct(&inp.bench),
        sharpe: (mean * TRADING_DAYS) / (sd * sqrt_days + SHARPE_EPS),
        win_rate: wins / n * 100.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_series_sharpe_is_zero_not_nan() {
        let m = perf(&PerfInput {
            nav: vec![100.0, 100.0, 100.0],
            bench: vec![100.0, 100.0, 100.0],
        });
        assert_eq!(m.sharpe, 0.0);
        assert!(!m.sharpe.is_nan() && !m.sharpe.is_infinite());
    }

    #[test]
    fn single_point_returns_zeros() {
        let m = perf(&PerfInput {
            nav: vec![100.0],
            bench: vec![100.0],
        });
        assert_eq!(m.cum, 0.0);
        assert_eq!(m.win_rate, 0.0);
    }
}
