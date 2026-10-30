//! 净值与风险指标（03 §7）：日收益取相邻差比；sd 为总体标准差（除以 n）；年化常数 244。
//! B11 已定稿（2026-10-30）：bench_vol（全序列 sd，原型 RMS 变体废除）/ beta / corr
//! （配对前缀 m 项；sd ≤ THRESHOLD_EPS 守卫——恒定序列在 f64 下带 ~1e-18 噪声，
//! 纯零判定会放出天文 beta，见 perf.json pf-downtrend desc）。

use serde::{Deserialize, Serialize};

use crate::{SHARPE_EPS, THRESHOLD_EPS, TRADING_DAYS};

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
    /// 基准年化波动 % = sd(rb) × √244 × 100（与 ann_vol 同算法，B11 定稿）
    pub bench_vol: f64,
    /// cov / var_rb'（配对前缀 m 项；sd_rb' ≤ 1e−9 → 0）
    pub beta: f64,
    /// cov / (sd_r' × sd_rb')（任一 sd ≤ 1e−9 → 0）
    pub corr: f64,
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

    // B11：bench_vol = 基准全序列总体 sd × √244 × 100（与 ann_vol 同算法）
    let bench_vol = if rb.is_empty() {
        0.0
    } else {
        let nb = rb.len() as f64;
        let mean_b = rb.iter().sum::<f64>() / nb;
        let var_b = rb.iter().map(|x| (x - mean_b) * (x - mean_b)).sum::<f64>() / nb;
        var_b.sqrt() * sqrt_days * 100.0
    };

    // B11：beta / corr 按配对前缀 m = min(n, nb)；均值/标准差取前缀 m 项（等长时 = 全样本）。
    // ε 守卫是口径的一部分：恒定序列的 f64 噪声（~1e-18）不得放大成天文 beta（03 §7）。
    let m = r.len().min(rb.len());
    let (beta, corr) = if m == 0 {
        (0.0, 0.0)
    } else {
        let mf = m as f64;
        let (rp, rbp) = (&r[..m], &rb[..m]);
        let mean_r = rp.iter().sum::<f64>() / mf;
        let mean_rb = rbp.iter().sum::<f64>() / mf;
        let cov = rp
            .iter()
            .zip(rbp.iter())
            .map(|(a, b)| (a - mean_r) * (b - mean_rb))
            .sum::<f64>()
            / mf;
        let var_rb = rbp.iter().map(|b| (b - mean_rb) * (b - mean_rb)).sum::<f64>() / mf;
        let sd_r = (rp.iter().map(|a| (a - mean_r) * (a - mean_r)).sum::<f64>() / mf).sqrt();
        let sd_rb = var_rb.sqrt();
        let beta = if sd_rb > THRESHOLD_EPS { cov / var_rb } else { 0.0 };
        let corr = if sd_r > THRESHOLD_EPS && sd_rb > THRESHOLD_EPS {
            cov / (sd_r * sd_rb)
        } else {
            0.0
        };
        (beta, corr)
    };

    PerfMetrics {
        cum: cum_pct(&inp.nav),
        bench_cum: cum_pct(&inp.bench),
        ann_vol: sd * sqrt_days * 100.0,
        mdd: max_drawdown_pct(&inp.nav),
        bench_mdd: max_drawdown_pct(&inp.bench),
        sharpe: (mean * TRADING_DAYS) / (sd * sqrt_days + SHARPE_EPS),
        win_rate: wins / n * 100.0,
        bench_vol,
        beta,
        corr,
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

    #[test]
    fn empty_bench_new_metrics_are_zero() {
        // bench 为空（前端「基准不可用」形态）：bench_vol/beta/corr 全 0，无 NaN
        let m = perf(&PerfInput {
            nav: vec![100.0, 101.0, 102.0],
            bench: vec![],
        });
        assert_eq!((m.bench_vol, m.beta, m.corr), (0.0, 0.0, 0.0));
    }

    #[test]
    fn constant_bench_eps_guard_kills_astronomic_beta() {
        // B11 ε 守卫：rb = [−0.02, 96.04/98−1] 两值 f64 差 ~1 ulp（非纯零），
        // 纯零判定会放出 beta ≈ 4×10^14；sd ≤ 1e-9 守卫必须归 0
        let m = perf(&PerfInput {
            nav: vec![100.0, 90.0, 85.0],
            bench: vec![100.0, 98.0, 96.04],
        });
        assert_eq!(m.beta, 0.0);
        assert_eq!(m.corr, 0.0);
        assert!(m.bench_vol.abs() < 1e-5);
    }
}
