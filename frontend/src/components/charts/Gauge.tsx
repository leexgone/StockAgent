import { cssVar } from "./svg";

/**
 * 得分弧（镜像 ZMC.gauge）：综合得分 0–100 的半环仪表。
 * 颜色走好坏轴红绿灯（B5：得分不是涨跌）——≥70 ok / ≥55 warn / else danger，
 * 与涨跌方向色（--up/--down）严格分开（03 §11 三套语义不混类）。
 */

export interface GaugeProps {
  score: number;
  label?: string;
  size?: number;
}

export default function Gauge({ score, label = "综合得分", size = 132 }: GaugeProps) {
  const r = 52;
  const cx = size / 2;
  const cy = size / 2 + 6;
  const C0 = Math.PI * 1.5;
  const pt = (a: number): [string, string] => [
    (cx + Math.cos(a) * r).toFixed(2),
    (cy + Math.sin(a) * r).toFixed(2),
  ];
  const arc = (from: number, to: number): string => {
    const [x0, y0] = pt(from);
    const [x1, y1] = pt(to);
    return `M${x0} ${y0} A${r} ${r} 0 ${to - from > Math.PI ? 1 : 0} 1 ${x1} ${y1}`;
  };
  const frac = Math.max(0, Math.min(1, score / 100));
  const col = score >= 70 ? cssVar("--ok") : score >= 55 ? cssVar("--warn") : cssVar("--danger");

  return (
    <svg viewBox={`0 0 ${size} ${size}`} width={size} height={size} className="chart">
      <path d={arc(C0, C0 + Math.PI)} fill="none" stroke={cssVar("--surface-2")} strokeWidth="9" strokeLinecap="round" />
      {frac > 0 && (
        <path d={arc(C0, C0 + Math.PI * frac)} fill="none" stroke={col} strokeWidth="9" strokeLinecap="round" />
      )}
      <text
        x={cx} y={cy + 2} textAnchor="middle"
        style={{ fontFamily: "var(--font-display)", fontSize: 30, fontWeight: 590, letterSpacing: "-.03em" }}
        fill={col}
      >
        {Math.round(score)}
      </text>
      <text x={cx} y={cy + 22} textAnchor="middle" style={{ fontSize: 10.5, letterSpacing: ".06em" }} fill={cssVar("--muted")}>
        {label}
      </text>
    </svg>
  );
}
