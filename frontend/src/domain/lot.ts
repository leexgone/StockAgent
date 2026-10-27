/**
 * 标的属性与交易单位（03 §1）：手数、报价小数位、数量校验、快捷档位。
 * 镜像 `backend/crates/zimu-domain/src/lot.rs`。
 */

import type { Kind, Side } from "./common";

export type KindInput = { kind: Kind };
export type LotOutput = { lot: number };
export type DecimalsOutput = { decimals: number };

/** 1 手 = 多少股/份/张（03 §1 表格） */
export function lotSize(kind: Kind): number {
  switch (kind) {
    case "stock":
      return 100;
    case "etf":
      return 100;
    case "cb":
      return 10;
  }
}

/** 报价小数位（03 §1 表格；转债 3 位为定稿口径，抽屉显示 bug 由 B4/P1-T3 修） */
export function priceDecimals(kind: Kind): number {
  switch (kind) {
    case "stock":
      return 2;
    case "etf":
      return 3;
    case "cb":
      return 3;
  }
}

/** 数量校验失败原因（枚举码进 fixture；中文文案是界面层职责，见 05） */
export type QtyReason = "not_lot_multiple" | "exceeds_held";

export type QtyVerdict = { valid: boolean; reason: QtyReason | null };

export type ValidateQtyInput = { kind: Kind; side: Side; qty: number; held: number };

/**
 * 数量校验（原型 `holdings.html:342`）：先整手倍数、后卖出不得超持仓。
 * `qty == 0` 视为「未填写」返回 valid（提交闸门另行要求 qty > 0，与原型 `qty &&` 短路一致）；
 * 负数按非法输入拒绝。奇零股卖出与 §1「可卖整数股」的冲突待 B12 定稿。
 * qty / held 为整数语义（Rust 侧 i64）。
 */
export function validateQty(kind: Kind, side: Side, qty: number, held: number): QtyVerdict {
  if (qty < 0) return { valid: false, reason: "not_lot_multiple" };
  if (qty === 0) return { valid: true, reason: null };
  const lot = lotSize(kind);
  if (qty % lot !== 0) return { valid: false, reason: "not_lot_multiple" };
  if (side === "sell" && qty > held) return { valid: false, reason: "exceeds_held" };
  return { valid: true, reason: null };
}

export type QuickTiersInput = { kind: Kind; side: Side; held: number };
export type TiersOutput = { tiers: number[] };

/**
 * 快捷档位（原型 `holdings.html:334`）：基准 = max(1 手, 持仓；无持仓按 10 手)，
 * 10% / 25% / 50% 向下取整到手、每档至少 1 手；卖出且有持仓时追加「全部清仓」。
 * 取整用 floor(floor(x/100)/lot)（与 Rust 侧整数除法逐位一致；输入范围内 double
 * 舍入不会跨整数边界，等价于原型浮点 floor 且无噪声）。
 */
export function quickTiers(kind: Kind, side: Side, held: number): number[] {
  const lot = lotSize(kind);
  const base = Math.max(lot, held > 0 ? held : lot * 10);
  const tiers = [10, 25, 50].map((pct) =>
    Math.max(lot, Math.floor(Math.floor((base * pct) / 100) / lot) * lot),
  );
  if (side === "sell" && held > 0) tiers.push(held);
  return tiers;
}
