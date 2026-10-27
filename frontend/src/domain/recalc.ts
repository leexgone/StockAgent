/**
 * 逐笔派生与组合汇总（03 §5）：`recalc()` 的纯函数形态。
 * 所有派生值只出自这里；分母为 0 时各比率取 0，不产生 NaN。
 * 镜像 `backend/crates/zimu-domain/src/recalc.rs`（求和/排序顺序一致）。
 */

import { ratio, type Kind } from "./common";

export type Holding = {
  code: string;
  kind: Kind;
  sector: string;
  shares: number;
  cost: number;
  price: number;
  prev: number;
};

export type PortfolioInput = { cash: number; holdings: Holding[] };

export type PerHolding = {
  code: string;
  /** (price − prev) / prev × 100 */
  day_chg: number;
  /** (price − prev) × shares */
  day_val: number;
  mv: number;
  /** (price − cost) × shares */
  pnl: number;
  /** (price − cost) / cost × 100 */
  pnl_pct: number;
  /** mv / total_assets × 100（含现金分母，03 §6 口径） */
  weight: number;
};

export type SectorAgg = {
  name: string;
  /** 行业市值 */
  v: number;
  /** 占 total_mv（不含现金分母） */
  w: number;
};

export type Agg = {
  cash: number;
  mv_stocks: number;
  mv_etf: number;
  mv_cb: number;
  total_mv: number;
  total_assets: number;
  day_pnl: number;
  /** 分母 = 昨日总资产（total_assets − day_pnl），别改成今日（03 §5 注） */
  day_pnl_pct: number;
  total_pnl: number;
  cost_total: number;
  total_pnl_pct: number;
  exposure: number;
  /** 按市值降序（排序是黄金向量期望的一部分） */
  sectors: SectorAgg[];
  /** Σw² / 10000，归一 0–1 */
  hhi: number;
  top3_sector: number;
};

export type RecalcOutput = {
  /** 与输入 holdings 同序对齐 */
  per: PerHolding[];
  agg: Agg;
};

const mvOf = (h: Holding) => h.price * h.shares;

function sumByKind(inp: PortfolioInput, kind: Kind): number {
  return inp.holdings.filter((h) => h.kind === kind).reduce((s, h) => s + mvOf(h), 0);
}

/** 全量重算（原型 `data.js:82` recalc()）。 */
export function recalc(inp: PortfolioInput): RecalcOutput {
  const mvStocks = sumByKind(inp, "stock");
  const mvEtf = sumByKind(inp, "etf");
  const mvCb = sumByKind(inp, "cb");
  const totalMv = mvStocks + mvEtf + mvCb;
  const totalAssets = totalMv + inp.cash;

  const per: PerHolding[] = inp.holdings.map((h) => {
    const mv = mvOf(h);
    return {
      code: h.code,
      day_chg: ratio(h.price - h.prev, h.prev) * 100,
      day_val: (h.price - h.prev) * h.shares,
      mv,
      pnl: (h.price - h.cost) * h.shares,
      pnl_pct: ratio(h.price - h.cost, h.cost) * 100,
      weight: ratio(mv, totalAssets) * 100,
    };
  });

  const dayPnl = per.reduce((s, p) => s + p.day_val, 0);
  const totalPnl = per.reduce((s, p) => s + p.pnl, 0);
  const costTotal = inp.holdings.reduce((s, h) => s + h.cost * h.shares, 0);

  // 行业聚合：首次出现顺序收集，再按市值稳定降序（Array#sort 规范保证稳定，与原型/Rust 一致）
  const grouped: SectorAgg[] = [];
  for (const h of inp.holdings) {
    const mv = mvOf(h);
    const found = grouped.find((g) => g.name === h.sector);
    if (found) found.v += mv;
    else grouped.push({ name: h.sector, v: mv, w: 0 });
  }
  for (const g of grouped) g.w = ratio(g.v, totalMv) * 100;
  const sectors = [...grouped].sort((a, b) => b.v - a.v);

  const hhi = sectors.reduce((s, x) => s + x.w * x.w, 0) / 10000;
  const top3Sector = sectors.slice(0, 3).reduce((s, x) => s + x.w, 0);

  return {
    per,
    agg: {
      cash: inp.cash,
      mv_stocks: mvStocks,
      mv_etf: mvEtf,
      mv_cb: mvCb,
      total_mv: totalMv,
      total_assets: totalAssets,
      day_pnl: dayPnl,
      day_pnl_pct: ratio(dayPnl, totalAssets - dayPnl) * 100,
      total_pnl: totalPnl,
      cost_total: costTotal,
      total_pnl_pct: ratio(totalPnl, costTotal) * 100,
      exposure: ratio(totalMv, totalAssets) * 100,
      sectors,
      hhi,
      top3_sector: top3Sector,
    },
  };
}
