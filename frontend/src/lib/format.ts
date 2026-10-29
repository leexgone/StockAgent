/**
 * 格式化工具（自 data/demo.ts 抽出，P5-T2；与原型 design/assets/data.js 逐一相同）。
 * 约定：金额/百分比一律走这里，不手搓 toFixed 拼百分号（CLAUDE.md 中文文案节）。
 */

export const fmt = (v: number, d = 2): string =>
  Number(v).toLocaleString("zh-CN", { minimumFractionDigits: d, maximumFractionDigits: d });
export const money = (v: number): string => "¥" + fmt(v, 2);
export const pct = (v: number): string => (v > 0 ? "+" : "") + fmt(v, 2) + "%";
export const wan = (v: number): string => (v / 10000).toFixed(2) + " 万";
/** 涨跌方向类（只用于涨跌数字与标记；好坏轴 .ok/.warn/.danger、交易方向 .buy/.sell 不混类，03 §11） */
export const dirClass = (v: number): "up" | "down" | "flat" => (v > 0 ? "up" : v < 0 ? "down" : "flat");
