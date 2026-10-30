/**
 * 格式化工具（自 data/demo.ts 抽出，P5-T2；与原型 design/assets/data.js 逐一相同）。
 * 约定：金额/百分比一律走这里，不手搓 toFixed 拼百分号（CLAUDE.md 中文文案节）。
 *
 * 负零防御（真实数据修复，2026-10-30）：负零（-0）与舍入到显示精度后为零的
 * 微负数（如 -0.004 → "-0"、-0.001 → "-0.00"）不得带负号——展示层统一剥掉。
 * 只影响显示；domain 层黄金向量文案（alert/advice）不经这里，口径不受影响。
 */

/** "-0" / "-0.00" 等舍零负值 → 去负号；正常负数（"-1,234.50"）原样保留 */
const stripNegZero = (s: string): string => (/^-0(?:\.0+)?$/.test(s) ? s.slice(1) : s);

export const fmt = (v: number, d = 2): string =>
  stripNegZero(Number(v).toLocaleString("zh-CN", { minimumFractionDigits: d, maximumFractionDigits: d }));
export const money = (v: number): string => "¥" + fmt(v, 2);
export const pct = (v: number): string => (v > 0 ? "+" : "") + fmt(v, 2) + "%";
export const wan = (v: number): string => stripNegZero((v / 10000).toFixed(2)) + " 万";
/** toFixed 同款但剥 "-0"（裸 toFixed 的可负值显示一律走这里） */
export const fixed = (v: number, d: number): string => stripNegZero(v.toFixed(d));
/** 涨跌方向类（只用于涨跌数字与标记；好坏轴 .ok/.warn/.danger、交易方向 .buy/.sell 不混类，03 §11） */
export const dirClass = (v: number): "up" | "down" | "flat" => (v > 0 ? "up" : v < 0 ? "down" : "flat");
