/**
 * 价格合法性（涨跌停校验，03 §2）：比例推断、涨停/跌停价计算、委托价校验。
 * 镜像 `backend/crates/zimu-domain/src/price_limit.rs`。
 * P1-T2 定稿（2026-10-28，B2 消除）：显示与拦截用同一对限价（含边界），
 * 旧原型 0.1% 容差废除；限价按整数单元四舍五入，消除浮点噪声。
 */

import { THRESHOLD_EPS, type Kind } from "./common";
import { priceDecimals } from "./lot";

export type LimitPctInput = { kind: Kind; code: string; st: boolean };
export type LimitPctOutput = { pct: number | null };

/**
 * 涨跌幅比例（03 §2 表）：股票按代码前缀（沪深主板 10 / ST 5、创业板与科创板 20，
 * 二者无 ST 制度，`st` 被忽略），转债（11/12 开头）一律 20；ETF 无法由代码推断
 * （159915 创业板 ETF ±20% 与 159919 沪深300ETF ±10% 同前缀）→ null，由行情源/
 * 数据层显式配置；北交所（±30%）超出产品范围 → null。ST 识别依赖行情字段
 * （P4 填充），不做名称字符串猜测。
 */
export function limitPct(kind: Kind, code: string, st: boolean): number | null {
  switch (kind) {
    case "stock":
      if (code.startsWith("68")) return 20; // 科创板（688/689）
      if (code.startsWith("30")) return 20; // 创业板（300/301/302）
      if (code.startsWith("60") || code.startsWith("00")) return st ? 5 : 10; // 沪深主板（002/003 已并入深主板）
      return null; // 北交所（8/4/920 开头）等：超出范围
    case "cb":
      // 沪 110/111/113/118，深 123/127/128
      return code.startsWith("11") || code.startsWith("12") ? 20 : null;
    case "etf":
      return null;
  }
}

export type LimitPricesInput = { kind: Kind; prev: number; pct: number };
export type LimitPricesOutput = { limit_up: number | null; limit_down: number | null };

/**
 * 整数单元四舍五入（half-up）：floor((units × (100 ± pct) + 50) / 100)。
 * 与 Rust 侧同一表达式逐位一致；全整数运算规避 double 噪声
 * （裸浮点 9.85 × 1.1 = 10.834999…，按小数 half-up 会错得 10.83）。
 */
function limitUnits(units: number, pct: number, up: boolean): number {
  const factor = up ? 100 + pct : 100 - pct;
  return Math.floor((units * factor + 50) / 100);
}

/**
 * 涨停/跌停价（03 §2）：prev × (1 ± pct/100) 按 kind 报价小数位（§1）四舍五入，
 * 与交易所算法一致。prev ≤ 0（停牌/无行情，NaN 同）或 pct ≤ 0（未配置）→ 双 null，
 * 界面显示 — 并跳过区间拦截。
 */
export function limitPrices(kind: Kind, prev: number, pct: number): LimitPricesOutput {
  if (!(prev > 0) || !(pct > 0)) return { limit_up: null, limit_down: null };
  const decimals = priceDecimals(kind);
  const scale = 10 ** decimals;
  // prev 是合法报价（≤ decimals 位小数），round 精确恢复整数单元（分/厘）
  const units = Math.round(prev * scale);
  return {
    limit_up: limitUnits(units, pct, true) / scale,
    limit_down: limitUnits(units, pct, false) / scale,
  };
}

/** 委托价校验失败原因（枚举码进 fixture；中文文案是界面层职责，见 05） */
export type PriceReason = "above_limit_up" | "below_limit_down" | "invalid_price";

export type PriceVerdict = { valid: boolean; reason: PriceReason | null };

export type ValidatePriceInput = { price: number; limit_up: number; limit_down: number };

/**
 * 委托价校验（03 §2）：limit_down ≤ price ≤ limit_up 合法，含边界（压线可成交
 * 是交易所语义）；显示与拦截必须用同一对限价。ε 守卫防浮点噪声翻转边界
 * （03 附录比对规则第 5 条）；price ≤ 0（或 NaN）→ invalid_price。
 */
export function validatePrice(price: number, limitUp: number, limitDown: number): PriceVerdict {
  if (!(price > 0)) return { valid: false, reason: "invalid_price" };
  if (price > limitUp + THRESHOLD_EPS) return { valid: false, reason: "above_limit_up" };
  if (price < limitDown - THRESHOLD_EPS) return { valid: false, reason: "below_limit_down" };
  return { valid: true, reason: null };
}
