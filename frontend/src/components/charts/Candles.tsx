import { useLayoutEffect, useRef, useState } from "react";
import { priceDecimals, type Kind } from "../../domain";
import { fmt } from "../../lib/format";
import { cssVar, smooth } from "./svg";

/**
 * K 线蜡烛图 + MA5/MA20 均线（手写 SVG，镜像 design/analysis.html 的 drawK，ADR-0008）。
 * A 股配色：阳线（收≥开）红 --up 空心、阴线绿 --down 实心；均线是好坏/趋势叠加，
 * MA5 用 --accent、MA20 用 --muted。OHLC 为真实日线（/api/market/kline），末根锚定
 * 现价；无数据不画（页面另行渲染引导）。宽度自适应容器（ResizeObserver）。
 */

export interface Ohlc {
  o: number;
  c: number;
  h: number;
  l: number;
  /** 交易日（tooltip / 末根标注用） */
  date?: string;
}

export interface CandlesProps {
  data: Ohlc[];
  kind: Kind;
  height?: number;
  /** 快线窗口（缺省 5） */
  maFast?: number;
  /** 慢线窗口（缺省 20；数据不足则不画） */
  maSlow?: number;
}

/** 简单移动平均（对收盘价）；前 k-1 根为 null（与原型 ma() 一致） */
function ma(data: Ohlc[], k: number): (number | null)[] {
  return data.map((_, i) => {
    if (i < k - 1) return null;
    let s = 0;
    for (let j = i - k + 1; j <= i; j++) s += data[j].c;
    return s / k;
  });
}

export default function Candles({ data, kind, height = 250, maFast = 5, maSlow = 20 }: CandlesProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [hostW, setHostW] = useState(0);
  useLayoutEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    setHostW(el.clientWidth);
    const ro = new ResizeObserver(() => setHostW(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const padT = 12;
  const padB = 16;
  const w = Math.max(320, hostW || 720);
  const ih = height - padT - padB;
  const dec = priceDecimals(kind);

  if (data.length === 0 || hostW === 0) {
    return <div className="chart-hold" ref={hostRef} style={{ height }} />;
  }

  const lo = Math.min(...data.map((x) => x.l));
  const hi = Math.max(...data.map((x) => x.h));
  const rg = hi - lo || hi * 0.02 || 1;
  const y = (v: number) => ih - ((v - (lo - rg * 0.06)) / (rg * 1.12)) * ih + padT;
  const step = w / data.length;
  const bw = Math.max(3, step * 0.56);

  const mFast = ma(data, Math.min(maFast, data.length));
  const mSlow = data.length >= maSlow ? ma(data, maSlow) : [];
  const lineOf = (arr: (number | null)[], col: string, wd: number) => {
    const pts = arr
      .map((v, i) => (v == null ? null : ([step * i + step / 2, y(v)] as [number, number])))
      .filter((p): p is [number, number] => p !== null);
    if (pts.length < 2) return null;
    return <path d={smooth(pts)} fill="none" stroke={col} strokeWidth={wd} opacity=".9" />;
  };

  const gridVs = [0, 1, 2, 3].map((g) => lo - rg * 0.06 + ((rg * 1.12) * g) / 3);

  return (
    <div className="chart-hold" ref={hostRef} style={{ height }}>
      <svg className="chart" width={w} height={height} viewBox={`0 0 ${w} ${height}`}>
        {gridVs.map((v, g) => {
          const yy = y(v);
          return (
            <g key={g}>
              <line className="grid-line" x1={0} x2={w} y1={yy.toFixed(1)} y2={yy.toFixed(1)} />
              <text x={3} y={(yy - 4).toFixed(1)} fill={cssVar("--muted")}>{fmt(v, dec)}</text>
            </g>
          );
        })}
        {data.map((k, i) => {
          const cx = step * i + step / 2;
          const up = k.c >= k.o;
          const col = cssVar(up ? "--up" : "--down");
          const top = y(Math.max(k.o, k.c));
          const bh = Math.max(1.4, Math.abs(y(k.o) - y(k.c)));
          return (
            <g key={i}>
              <line x1={cx.toFixed(1)} x2={cx.toFixed(1)} y1={y(k.h).toFixed(1)} y2={y(k.l).toFixed(1)} stroke={col} strokeWidth="1.2" />
              <rect
                x={(cx - bw / 2).toFixed(1)} y={top.toFixed(1)} width={bw.toFixed(1)} height={bh.toFixed(1)} rx="1"
                fill={up ? "none" : col} stroke={col} strokeWidth="1.2"
              />
            </g>
          );
        })}
        {lineOf(mFast, cssVar("--accent"), 1.4)}
        {lineOf(mSlow, cssVar("--muted"), 1.2)}
      </svg>
    </div>
  );
}
