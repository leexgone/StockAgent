import { cssVar, scale, smooth } from "./svg";

/**
 * 迷你走势线（表格内联，镜像 ZMC.spark）：涨红跌绿只由 dir（当日涨跌幅）定色，
 * 与曲线自身形态无关（03 §11 三套语义）。无数据返回 null——空态防御，不画假线。
 */

export interface SparkProps {
  /** 收盘价序列（近 30 交易日） */
  data: number[];
  /** 当日涨跌幅（决定线色方向） */
  dir: number;
  width?: number;
  height?: number;
}

export default function Spark({ data, dir, width = 96, height = 30 }: SparkProps) {
  if (!data || data.length === 0) return null;
  const sc = scale(data, 0.12, width - 2, height - 4);
  const pts = data.map((v, i) => [sc.x(i) + 1, sc.y(v) + 2] as [number, number]);
  const col = dir >= 0 ? cssVar("--up") : cssVar("--down");
  const last = pts[pts.length - 1];
  return (
    <svg className="spark" viewBox={`0 0 ${width} ${height}`} width={width} height={height} aria-hidden="true">
      <path d={smooth(pts)} fill="none" stroke={col} strokeWidth="1.5" strokeLinecap="round" />
      <circle cx={last[0].toFixed(1)} cy={last[1].toFixed(1)} r="1.9" fill={col} />
    </svg>
  );
}
