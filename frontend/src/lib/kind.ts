/**
 * 标的类型/市场映射工具（P5-T2 dashboard 屏起共用）。
 * symbol 拼装规则与后端 scheduler `symbol_of` 一致：显式 mkt 小写前缀 + 代码，
 * 不做号段猜测（B7）；mkt 不在 SH/SZ/BJ 内返回 null（调用方显示 "—" 并跳过行情）。
 */

import type { components } from "../api/schema";

export type Kind = components["schemas"]["Kind"];

/** 原型 D.kindLabel 同款展示映射 */
export const KIND_LABEL: Record<Kind, string> = {
  stock: "股票",
  etf: "ETF / 基金",
  cb: "可转债",
};

/** mkt（SH/SZ/BJ，大小写不敏感）+ code → sidecar symbol（如 sh600519）；拼不出 → null */
export function symbolOf(mkt: string, code: string): string | null {
  const m = mkt.toLowerCase();
  if (m !== "sh" && m !== "sz" && m !== "bj") return null;
  if (!/^[0-9]{6}$/.test(code)) return null;
  return m + code;
}
