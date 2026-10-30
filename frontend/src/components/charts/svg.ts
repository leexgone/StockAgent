/**
 * 图表数学层 —— 镜像 design/assets/charts.js 的纯计算部分（ADR-0008：手写 SVG，
 * 不引图表库）。scale/smooth/path 与原型逐行同构，保证移植屏与原型曲线形态一致；
 * 颜色经 cssVar() 读设计令牌（--up/--down/--accent/--muted/--border…），不写死色值。
 */

export interface Scale {
  min: number;
  max: number;
  /** 索引 → x（单点居中） */
  x: (i: number) => number;
  /** 值 → y（未加 padT，调用方平移） */
  y: (v: number) => number;
}

/** 原型 charts.js scale()：值域上下各留 pad 比例的余量；全平常量序列防除零 */
export function scale(series: number[], pad: number, w: number, h: number): Scale {
  let min = Math.min(...series);
  let max = Math.max(...series);
  const r = max - min || Math.abs(max) * 0.02 || 1;
  min -= r * pad;
  max += r * pad;
  return {
    min,
    max,
    x: (i) => (series.length === 1 ? w / 2 : (i / (series.length - 1)) * w),
    y: (v) => h - ((v - min) / (max - min)) * h,
  };
}

/** 折线 path（尖角；bars/调试用） */
export function path(pts: [number, number][]): string {
  return pts.map((p, i) => (i ? "L" : "M") + p[0].toFixed(1) + " " + p[1].toFixed(1)).join(" ");
}

/** 轻量 catmull-rom → 三次贝塞尔（原型 smooth()，避免折线生硬） */
export function smooth(pts: [number, number][]): string {
  if (pts.length === 0) return "";
  if (pts.length === 1) return `M${pts[0][0].toFixed(1)} ${pts[0][1].toFixed(1)}`;
  let d = `M${pts[0][0].toFixed(1)} ${pts[0][1].toFixed(1)}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] || pts[i];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2] || p2;
    const c1x = p1[0] + (p2[0] - p0[0]) / 6;
    const c1y = p1[1] + (p2[1] - p0[1]) / 6;
    const c2x = p2[0] - (p3[0] - p1[0]) / 6;
    const c2y = p2[1] - (p3[1] - p1[1]) / 6;
    d += `C${c1x.toFixed(1)} ${c1y.toFixed(1)} ${c2x.toFixed(1)} ${c2y.toFixed(1)} ${p2[0].toFixed(1)} ${p2[1].toFixed(1)}`;
  }
  return d;
}

/** 读设计令牌当前值（深浅色主题跟随；图表 stroke/fill 用） */
export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** 环形图配色（原型 DONUT_COLORS 原样搬运，oklch 与令牌同色系） */
export const DONUT_COLORS = [
  "oklch(58% 0.18 255)",
  "oklch(55% 0.19 22)",
  "oklch(62% 0.15 78)",
  "oklch(56% 0.12 170)",
  "oklch(60% 0.14 320)",
  "oklch(52% 0.13 152)",
  "oklch(58% 0.1 285)",
  "oklch(65% 0.06 250)",
];
