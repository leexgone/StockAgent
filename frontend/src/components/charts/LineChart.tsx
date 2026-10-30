import { useId, useLayoutEffect, useRef, useState } from "react";
import { fixed } from "../../lib/format";
import { cssVar, scale, smooth } from "./svg";

/**
 * 主折线 / 面积图（手写 SVG，镜像 design/assets/charts.js 的 ZMC.line，ADR-0008）。
 * 宽度自适应容器（ResizeObserver），十字准星 + tooltip 与原型同交互：
 * pointermove 定位最近数据点，竖线 + 浮层跟随，pointerleave 隐藏。
 * 空序列不画（返回空容器，页面另行渲染引导——原型同款空态防御）。
 */

export interface LineSeries {
  data: number[];
  /** 缺省 = color（首系列主色） */
  color?: string;
  /** false = 不填充面积（仅首系列参与填充） */
  area?: boolean;
  /** 虚线（如 "4 4"，基准线用） */
  dash?: string;
  width?: number;
  /** false = 末端不画圆点 */
  endDot?: boolean;
}

export interface LineChartProps {
  series: LineSeries[];
  height?: number;
  pad?: number;
  fill?: boolean;
  /** 主色（渐变面积与缺省线色），缺省 --accent */
  color?: string;
  /** x 轴日期标签（缺省不画底轴） */
  labels?: string[] | null;
  yFmt?: (v: number) => string;
  /** 十字准星浮层内容（i = 数据点索引） */
  tip?: (i: number) => React.ReactNode;
}

export default function LineChart({
  series,
  height = 250,
  pad = 0.08,
  fill = true,
  color,
  labels = null,
  yFmt = (v) => fixed(v, 2),
  tip,
}: LineChartProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const [hostW, setHostW] = useState(0);
  const [hover, setHover] = useState<number | null>(null);

  // 宽度跟随容器（原型是 window resize 防抖重绘；这里 ResizeObserver 直接量）
  useLayoutEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    setHostW(el.clientWidth);
    const ro = new ResizeObserver(() => setHostW(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const w = Math.max(240, hostW || 600);
  const padT = 12;
  const padB = labels ? 20 : 6;
  const iw = w;
  const ih = height - padT - padB;
  const main = series[0]?.data ?? [];
  const all = series.flatMap((s) => s.data);
  const hasData = all.length > 0 && hostW > 0;
  const sc = hasData ? scale(all, pad, iw, ih) : null;
  const baseColor = color ?? cssVar("--accent");

  // tooltip 水平夹取（原型同款：量浮层宽度后夹在容器内）
  useLayoutEffect(() => {
    const t = tipRef.current;
    const host = hostRef.current;
    if (!t || !host || hover === null || !sc) return;
    const tw = t.offsetWidth;
    const x = sc.x(hover);
    t.style.left = `${Math.max(4, Math.min(host.clientWidth - tw - 4, x - tw / 2))}px`;
    t.style.top = `${sc.y(main[hover] ?? 0) + padT - 10}px`;
  }, [hover, hostW, series]); // eslint-disable-line react-hooks/exhaustive-deps

  const onMove = (e: React.PointerEvent<SVGSVGElement>) => {
    if (!sc || main.length === 0) return;
    const r = e.currentTarget.getBoundingClientRect();
    const rel = (e.clientX - r.left) / iw;
    setHover(Math.max(0, Math.min(main.length - 1, Math.round(rel * (main.length - 1)))));
  };

  const gridYs = [0, 1, 2, 3].map((g) => (sc ? sc.min + ((sc.max - sc.min) * g) / 3 : 0));
  const labelStep = labels && sc ? Math.ceil(labels.length / Math.max(2, Math.floor(iw / 74))) : 1;

  return (
    <div className="chart-hold" ref={hostRef} style={{ height }}>
      {hasData && sc && (
        <>
          <svg
            className="chart"
            width={w}
            height={height}
            viewBox={`0 0 ${w} ${height}`}
            onPointerMove={tip ? onMove : undefined}
            onPointerLeave={tip ? () => setHover(null) : undefined}
          >
            <defs>
              <linearGradient id={`lg${uid}`} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor={baseColor} stopOpacity=".18" />
                <stop offset="1" stopColor={baseColor} stopOpacity="0" />
              </linearGradient>
            </defs>
            {gridYs.map((v, g) => {
              const y = sc.y(v) + padT;
              return (
                <g key={g}>
                  <line className="grid-line" x1={0} x2={w} y1={y.toFixed(1)} y2={y.toFixed(1)} />
                  <text x={2} y={(y - 4).toFixed(1)} fill={cssVar("--muted")}>
                    {yFmt(v)}
                  </text>
                </g>
              );
            })}
            {series.map((s, si) => {
              const pts = s.data.map((v, i) => [sc.x(i), sc.y(v) + padT] as [number, number]);
              if (pts.length === 0) return null;
              const lineColor = s.color ?? baseColor;
              const last = pts[pts.length - 1];
              return (
                <g key={si}>
                  {s.area !== false && fill && si === 0 && (
                    <path
                      d={`${smooth(pts)} L${last[0].toFixed(1)} ${ih + padT} L${pts[0][0].toFixed(1)} ${ih + padT}Z`}
                      fill={`url(#lg${uid})`}
                    />
                  )}
                  <path
                    d={smooth(pts)}
                    fill="none"
                    stroke={lineColor}
                    strokeWidth={s.width ?? 1.8}
                    strokeDasharray={s.dash}
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                  {s.endDot !== false && <circle cx={last[0].toFixed(1)} cy={last[1].toFixed(1)} r="2.8" fill={lineColor} />}
                </g>
              );
            })}
            {labels &&
              labels.map((L, i) =>
                i % labelStep === 0 ? (
                  <text key={i} x={sc.x(i).toFixed(1)} y={height - 4} textAnchor="middle" fill={cssVar("--muted")}>
                    {L}
                  </text>
                ) : null,
              )}
            {tip && (
              <line
                x1={hover !== null ? sc.x(hover) : 0}
                x2={hover !== null ? sc.x(hover) : 0}
                y1={padT}
                y2={ih + padT}
                stroke={cssVar("--border-2")}
                strokeWidth="1"
                opacity={hover !== null ? 1 : 0}
              />
            )}
          </svg>
          {tip && (
            <div
              className="crosshair-tip"
              ref={tipRef}
              style={{ opacity: hover !== null ? 1 : 0, transform: "translateY(-100%)" }}
            >
              {hover !== null ? tip(hover) : null}
            </div>
          )}
        </>
      )}
    </div>
  );
}
