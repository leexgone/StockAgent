/**
 * 净值与风险指标（03 §7）：日收益取相邻差比；sd 为总体标准差（除以 n）；年化常数 244。
 * B11 已定稿（2026-10-30）：bench_vol（全序列 sd，原型 RMS 变体废除）/ beta / corr
 * （配对前缀 m 项；sd ≤ THRESHOLD_EPS 守卫——恒定序列在 f64 下带 ~1e-18 噪声，
 * 纯零判定会放出天文 beta）。
 * 镜像 `backend/crates/zimu-domain/src/perf.rs`。
 */

import { SHARPE_EPS, THRESHOLD_EPS, TRADING_DAYS } from "./common";

export type PerfInput = {
  /** 组合净值序列 */
  nav: number[];
  /** 基准序列（已归一到同一起点） */
  bench: number[];
};

export type PerfMetrics = {
  /** 累计收益 % */
  cum: number;
  bench_cum: number;
  /** 年化波动 % = sd × √244 × 100 */
  ann_vol: number;
  /** 最大回撤 %（滚动前高） */
  mdd: number;
  bench_mdd: number;
  /** (mean × 244) / (sd × √244 + 1e−9)，未扣无风险利率 */
  sharpe: number;
  /** 跑赢基准的天数占比 % */
  win_rate: number;
  /** 基准年化波动 % = sd(rb) × √244 × 100（与 ann_vol 同算法，B11 定稿） */
  bench_vol: number;
  /** cov / var_rb'（配对前缀 m 项；sd_rb' ≤ 1e−9 → 0） */
  beta: number;
  /** cov / (sd_r' × sd_rb')（任一 sd ≤ 1e−9 → 0） */
  corr: number;
};

const zeroMetrics = (): PerfMetrics => ({
  cum: 0,
  bench_cum: 0,
  ann_vol: 0,
  mdd: 0,
  bench_mdd: 0,
  sharpe: 0,
  win_rate: 0,
  bench_vol: 0,
  beta: 0,
  corr: 0,
});

function returns(series: number[]): number[] {
  const out: number[] = [];
  for (let i = 0; i + 1 < series.length; i++) {
    out.push(series[i] !== 0 ? series[i + 1] / series[i] - 1 : 0);
  }
  return out;
}

function cumPct(series: number[]): number {
  if (series.length < 2 || series[0] === 0) return 0;
  return (series[series.length - 1] / series[0] - 1) * 100;
}

function maxDrawdownPct(series: number[]): number {
  let peak = -Infinity;
  let mdd = 0;
  for (const v of series) {
    if (v > peak) peak = v;
    if (peak > 0) {
      const dd = (v - peak) / peak;
      if (dd < mdd) mdd = dd;
    }
  }
  return mdd * 100;
}

/**
 * 净值指标（原型 `data.js:161` perf()）。序列长度 < 2（无相邻收益）→ 全指标返回 0，
 * 不报错（03 附录约定 2）。nav 与 bench 长度不等时 win_rate 按重叠前缀配对（zip），
 * 分母仍是 nav 收益数。
 */
export function perf(inp: PerfInput): PerfMetrics {
  if (inp.nav.length < 2) return zeroMetrics();
  const r = returns(inp.nav);
  const rb = returns(inp.bench);
  const n = r.length;

  const mean = r.reduce((s, x) => s + x, 0) / n;
  const variance = r.reduce((s, x) => s + (x - mean) * (x - mean), 0) / n;
  const sd = Math.sqrt(variance);
  const sqrtDays = Math.sqrt(TRADING_DAYS);

  let wins = 0;
  const paired = Math.min(r.length, rb.length);
  for (let i = 0; i < paired; i++) {
    if (r[i] > rb[i]) wins++;
  }

  // B11：bench_vol = 基准全序列总体 sd × √244 × 100（与 ann_vol 同算法）
  let benchVol = 0;
  if (rb.length > 0) {
    const mb = rb.reduce((s, x) => s + x, 0) / rb.length;
    const vb = rb.reduce((s, x) => s + (x - mb) * (x - mb), 0) / rb.length;
    benchVol = Math.sqrt(vb) * sqrtDays * 100;
  }

  // B11：beta / corr 按配对前缀 m；均值/标准差取前缀 m 项（等长时 = 全样本）。
  // ε 守卫是口径的一部分：恒定序列的 f64 噪声不得放大成天文 beta（03 §7）
  const m = Math.min(r.length, rb.length);
  let beta = 0;
  let corr = 0;
  if (m > 0) {
    const rp = r.slice(0, m);
    const rbp = rb.slice(0, m);
    const mr = rp.reduce((s, x) => s + x, 0) / m;
    const mrb = rbp.reduce((s, x) => s + x, 0) / m;
    let cov = 0;
    let varB = 0;
    let sumR = 0;
    for (let i = 0; i < m; i++) {
      cov += (rp[i] - mr) * (rbp[i] - mrb);
      varB += (rbp[i] - mrb) * (rbp[i] - mrb);
      sumR += (rp[i] - mr) * (rp[i] - mr);
    }
    cov /= m;
    varB /= m;
    const sdR = Math.sqrt(sumR / m);
    const sdB = Math.sqrt(varB);
    if (sdB > THRESHOLD_EPS) beta = cov / varB;
    if (sdR > THRESHOLD_EPS && sdB > THRESHOLD_EPS) corr = cov / (sdR * sdB);
  }

  return {
    cum: cumPct(inp.nav),
    bench_cum: cumPct(inp.bench),
    ann_vol: sd * sqrtDays * 100,
    mdd: maxDrawdownPct(inp.nav),
    bench_mdd: maxDrawdownPct(inp.bench),
    sharpe: (mean * TRADING_DAYS) / (sd * sqrtDays + SHARPE_EPS),
    win_rate: (wins / n) * 100,
    bench_vol: benchVol,
    beta,
    corr,
  };
}
