/**
 * 净值与风险指标（03 §7）：日收益取相邻差比；sd 为总体标准差（除以 n）；年化常数 244。
 * bench_vol / beta / 相关系数不在本模块：未收入 03 §7 且原型两处实现不一致，
 * 口径待定稿（07-backlog B11）。
 * 镜像 `backend/crates/zimu-domain/src/perf.rs`。
 */

import { SHARPE_EPS, TRADING_DAYS } from "./common";

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
};

const zeroMetrics = (): PerfMetrics => ({
  cum: 0,
  bench_cum: 0,
  ann_vol: 0,
  mdd: 0,
  bench_mdd: 0,
  sharpe: 0,
  win_rate: 0,
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

  return {
    cum: cumPct(inp.nav),
    bench_cum: cumPct(inp.bench),
    ann_vol: sd * sqrtDays * 100,
    mdd: maxDrawdownPct(inp.nav),
    bench_mdd: maxDrawdownPct(inp.bench),
    sharpe: (mean * TRADING_DAYS) / (sd * sqrtDays + SHARPE_EPS),
    win_rate: (wins / n) * 100,
  };
}
