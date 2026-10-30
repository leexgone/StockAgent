import { cssVar, DONUT_COLORS } from "./svg";

/**
 * 环形图（镜像 ZMC.donut）：资产拆分/行业权重用。扇区 = 圆弧 path（stroke 粗线），
 * <title> 提供原生悬停标注；配色走 DONUT_COLORS（与原型同一组 oklch）。
 * 总量为 0 时只画底环，不画扇区（空账户不伪装分布）。
 */

export interface DonutItem {
  name: string;
  v: number;
  color?: string;
}

export interface DonutProps {
  items: DonutItem[];
  size?: number;
  thick?: number;
}

export default function Donut({ items, size = 220, thick = 22 }: DonutProps) {
  const r = size / 2 - 14;
  const sw = thick;
  const cx = size / 2;
  const cy = size / 2;
  const total = items.reduce((s, x) => s + x.v, 0);
  const pt = (ang: number, rad: number): [string, string] => [
    (cx + Math.cos(ang) * rad).toFixed(2),
    (cy + Math.sin(ang) * rad).toFixed(2),
  ];

  let a0 = -Math.PI / 2;
  const arcs = total > 0 ? items.map((it, i) => {
    const frac = it.v / total;
    const a1 = a0 + frac * Math.PI * 2 * 0.998;
    const large = frac > 0.5 ? 1 : 0;
    const [x0, y0] = pt(a0, r);
    const [x1, y1] = pt(a1, r);
    const d = `M${x0} ${y0} A${r} ${r} 0 ${large} 1 ${x1} ${y1}`;
    a0 = a1;
    return { d, color: it.color ?? DONUT_COLORS[i % DONUT_COLORS.length], name: it.name, frac };
  }) : [];

  return (
    <svg viewBox={`0 0 ${size} ${size}`} width={size} height={size} className="chart" style={{ maxWidth: "100%" }}>
      <circle cx={cx} cy={cy} r={r} fill="none" stroke={cssVar("--surface-2")} strokeWidth={sw} />
      {arcs.map((a) => (
        <path key={a.name} d={a.d} fill="none" stroke={a.color} strokeWidth={sw} strokeLinecap="butt">
          <title>{`${a.name} ${(a.frac * 100).toFixed(1)}%`}</title>
        </path>
      ))}
    </svg>
  );
}
