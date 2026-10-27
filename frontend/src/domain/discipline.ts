/**
 * 仓位纪律四条与降仓卖出数量（03 §6）。
 * 四条规则分母不统一（务必照抄）：单票/转债/现金 → total_assets（含现金），行业 → total_mv（不含现金）。
 * 镜像 `backend/crates/zimu-domain/src/discipline.rs`。
 */

import { ratio, THRESHOLD_EPS } from "./common";
import type { Holding, PortfolioInput } from "./recalc";

export const LIMIT_SINGLE_STOCK = 25;
export const LIMIT_SECTOR = 35;
export const LIMIT_CB_TOTAL = 12;
export const LIMIT_CASH_RESERVE = 10;

export type Rule = { value: number; threshold: number; pass: boolean };

export type DisciplineReport = {
  single_stock: Rule;
  sector: Rule;
  cb_total: Rule;
  cash_reserve: Rule;
};

/** 上限类规则：value ≤ threshold 为 pass（压线算安全，ε 守卫防浮点噪声） */
const atMost = (value: number, threshold: number): Rule => ({
  value,
  threshold,
  pass: value <= threshold + THRESHOLD_EPS,
});

/** 下限类规则（现金保留）：value ≥ threshold 为 pass */
const atLeast = (value: number, threshold: number): Rule => ({
  value,
  threshold,
  pass: value >= threshold - THRESHOLD_EPS,
});

/** 纪律四条体检（原型 `holdings.html:419` RULES）。空持仓时前三条 value = 0。 */
export function check(inp: PortfolioInput): DisciplineReport {
  const mvOf = (h: Holding) => h.price * h.shares;
  const totalMv = inp.holdings.reduce((s, h) => s + mvOf(h), 0);
  const totalAssets = totalMv + inp.cash;

  const maxSingle = inp.holdings.reduce((m, h) => Math.max(m, mvOf(h)), 0);
  const singleStock = atMost(ratio(maxSingle, totalAssets) * 100, LIMIT_SINGLE_STOCK);

  // 单一行业占市值（分母 total_mv，不含现金）
  const grouped: { name: string; v: number }[] = [];
  for (const h of inp.holdings) {
    const mv = mvOf(h);
    const found = grouped.find((g) => g.name === h.sector);
    if (found) found.v += mv;
    else grouped.push({ name: h.sector, v: mv });
  }
  const maxSector = grouped.reduce((m, g) => Math.max(m, g.v), 0);
  const sector = atMost(ratio(maxSector, totalMv) * 100, LIMIT_SECTOR);

  const cbMv = inp.holdings.filter((h) => h.kind === "cb").reduce((s, h) => s + mvOf(h), 0);
  const cbTotal = atMost(ratio(cbMv, totalAssets) * 100, LIMIT_CB_TOTAL);

  const cashReserve = atLeast(ratio(inp.cash, totalAssets) * 100, LIMIT_CASH_RESERVE);

  return { single_stock: singleStock, sector, cb_total: cbTotal, cash_reserve: cashReserve };
}

export type SellToInput = {
  mv: number;
  price: number;
  total_assets: number;
  /** 目标仓位 % */
  target: number;
  /** 手数（03 §1；公式已由写死 100 泛化，见 03 附录约定 1） */
  lot: number;
};

export type SellToOutput = { qty: number };

/**
 * 降到目标仓位的卖出数量：`max(lot, ceil((mv − TA×target/100) / price / lot) × lot)`。
 * 已在目标线下时仍返回最小一手（max 语义：卖就至少一手，是否真要卖由调用方判断）。
 * 退化输入（price ≤ 0 或 lot ≤ 0）返回最小一手并由此兜底，不产生除零。
 */
export function sellTo(inp: SellToInput): SellToOutput {
  if (inp.price <= 0 || inp.lot <= 0) return { qty: Math.max(inp.lot, 1) };
  const excess = inp.mv - (inp.total_assets * inp.target) / 100;
  const shares = excess / inp.price;
  const lots = Math.ceil(shares / inp.lot);
  return { qty: Math.max(lots * inp.lot, inp.lot) };
}
