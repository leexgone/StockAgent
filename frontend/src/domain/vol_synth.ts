/**
 * 波动合成假设（03 §8）：ρ 为写死假设值（同标的 1 / 同行业 0.62 / 跨行业 0.35），
 * 只用于「调整前后谁更大」的相对比较，不是绝对风险预测。
 * C7 拍板改估计值后，本模块与 `fixtures/golden/vol_synth.json` 及 Rust 侧要一起改。
 * 镜像 `backend/crates/zimu-domain/src/vol_synth.rs`（累加顺序一致）。
 */

import { TRADING_DAYS } from "./common";

export const RHO_SAME = 0.62;
export const RHO_CROSS = 0.35;

export type VolItem = {
  code: string;
  sector: string;
  /** 占市值权重（分数）；Σw 不强制为 1——再平衡「调整后」场景权重和小于 1 */
  w: number;
  /** 日收益标准差（分数，近 30 日） */
  sigma: number;
};

export type VolSynthInput = { items: VolItem[] };

export type VolSynthOutput = {
  /** 组合年化波动 %（与原型 diagnosis.html portVol 一致，×100 输出百分数） */
  ann_vol: number;
};

/** √(ΣᵢΣⱼ wᵢwⱼσᵢσⱼρᵢⱼ) × √244 × 100（原型 `diagnosis.html:137` portVol）。 */
export function volSynth(inp: VolSynthInput): VolSynthOutput {
  let v = 0;
  for (const a of inp.items) {
    for (const b of inp.items) {
      const rho = a.code === b.code ? 1 : a.sector === b.sector ? RHO_SAME : RHO_CROSS;
      v += a.w * b.w * a.sigma * b.sigma * rho;
    }
  }
  return { ann_vol: Math.sqrt(Math.max(v, 0)) * Math.sqrt(TRADING_DAYS) * 100 };
}
