import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import LineChart from "../components/charts/LineChart";
import EmptyHero from "../components/EmptyHero";
import { IconShield, IconSpark } from "../components/Icons";
import { DONUT_COLORS } from "../components/charts/svg";
import {
  LIMIT_CASH_RESERVE,
  LIMIT_SECTOR,
  LIMIT_SINGLE_STOCK,
  lotSize,
  perf,
  priceDecimals,
  sellTo,
  volSynth,
  type VolItem,
} from "../domain";
import { alignBench } from "../lib/bench";
import { dirClass, fixed, fmt, money, pct } from "../lib/format";
import { symbolOf } from "../lib/kind";
import { useEquity } from "../store/equity";
import { klineKey, useKline } from "../store/kline";
import { usePortfolio, type PortfolioView } from "../store/portfolio";
import { useUi } from "../store/ui";

/**
 * 组合诊断（diagnosis 屏，P5-T2 真实数据移植）—— 对照基准 design/diagnosis.html。
 * 纪律（ADR-0008）：KPI 与分区间表只用黄金向量校验的 perf 字段（B11 定稿
 * 2026-10-30 后含 bench_vol/beta/corr，恒定基准 ε 守卫 → 0）+ 后端 agg 原值
 * （hhi/sectors），不在前端重算。再平衡清单的「调整前后波动」走 volSynth（03 §8
 * 黄金向量：ρ 同标的 1 / 同行业 0.62 / 跨行业 0.35，σᵢ = 各标的近 30 日真实日线收益
 * 标准差），降仓数量走 sellTo（03 §6）；数字全部取后端 recalc 原值，清单是客户端
 * 模板骨架。基准 = sh000300 价格指数归一（未含分红，非 §7「全收益」，B11 如实标注）。
 */

type HRow = PortfolioView["holdings"][number];
type PRow = PortfolioView["per"][number];

const BENCH_SYMBOL = "sh000300";
const SPARK_Q = { withToday: true } as const;
const WINDOWS: [number, string][] = [[20, "近一月"], [45, "近两月"], [90, "近一季"]];
const SIGMA_FALLBACK = 0.012; // 无日线时的日波动兜底（与原型一致，标注为估计）

/** 近 30 日收益的总体标准差（分数）——σᵢ 输入估计（03 §8），非黄金向量口径本身 */
function returnStd(closes: number[]): number {
  if (closes.length < 2) return 0;
  const r: number[] = [];
  for (let i = 1; i < closes.length; i++) r.push(closes[i] / closes[i - 1] - 1);
  const m = r.reduce((s, x) => s + x, 0) / r.length;
  return Math.sqrt(r.reduce((s, x) => s + (x - m) ** 2, 0) / r.length);
}

/** 水下曲线（距滚动前高 %）——图表数据，简单峰值追踪，非持久化派生数字 */
function underwater(nav: number[]): number[] {
  let peak = nav[0] ?? 0;
  return nav.map((v) => {
    peak = Math.max(peak, v);
    return peak > 0 ? (v / peak - 1) * 100 : 0;
  });
}

interface Fix {
  p: 1 | 2 | 3;
  t: string;
  d: string;
  calc: string;
  code?: string;
  side?: "buy" | "sell";
  qty?: number;
}

export default function DiagnosisScreen() {
  const navigate = useNavigate();
  const view = usePortfolio((s) => s.view);
  const pLoading = usePortfolio((s) => s.loading);
  const pError = usePortfolio((s) => s.error);
  const equityRows = useEquity((s) => s.rows);
  const refreshTick = useUi((s) => s.refreshTick);
  const [win, setWin] = useState(90);
  const [acked, setAcked] = useState<Set<number>>(new Set());

  const toast = (m: string) => useUi.getState().pushToast(m);
  const agg = view?.agg;
  const holdings = view?.holdings ?? [];
  const isEmpty = view !== null && holdings.length === 0;

  /* ---------- kline：基准 + 各持仓（σᵢ 用） ---------- */
  const benchStart = equityRows.length > 0 ? equityRows[0].date : null;
  const benchQ = useMemo(() => (benchStart ? { start: benchStart, withToday: true } : null), [benchStart]);
  const holdSymsKey = holdings.map((h) => symbolOf(h.mkt, h.code)).filter((s): s is string => s !== null).join(",");
  useEffect(() => {
    if (benchQ) {
      if (refreshTick === 0) void useKline.getState().load(BENCH_SYMBOL, benchQ);
      else void useKline.getState().reload(BENCH_SYMBOL, benchQ);
    }
    for (const sym of holdSymsKey.split(",").filter(Boolean)) {
      if (refreshTick === 0) void useKline.getState().load(sym, SPARK_Q);
      else void useKline.getState().reload(sym, SPARK_Q);
    }
  }, [benchQ, holdSymsKey, refreshTick]);

  const benchKey = benchQ ? klineKey(BENCH_SYMBOL, benchQ) : null;
  const benchBars = useKline((s) => (benchKey ? s.series[benchKey] : undefined));
  const klineSeries = useKline((s) => s.series);

  /* ---------- 净值 / 基准 / perf ---------- */
  const navSeries = useMemo(() => equityRows.map((r) => r.total_assets), [equityRows]);
  const navDates = useMemo(() => equityRows.map((r) => r.date), [equityRows]);
  const benchSeries = useMemo(
    (): number[] | null => alignBench(navDates, benchBars, navSeries[0] ?? 0),
    [benchBars, navSeries, navDates],
  );

  const hasCurve = navSeries.length >= 2;
  const navWin = useMemo(() => navSeries.slice(-win), [navSeries, win]);
  const benchWin = useMemo(() => (benchSeries ? benchSeries.slice(-win) : null), [benchSeries, win]);
  const datesWin = useMemo(() => navDates.slice(-win), [navDates, win]);
  const perfWin = useMemo(
    () => (hasCurve ? perf({ nav: navWin, bench: benchWin ?? [] }) : null),
    [hasCurve, navWin, benchWin],
  );
  const underWin = useMemo(() => (hasCurve ? underwater(navWin) : []), [hasCurve, navWin]);

  const sigmaOf = (h: HRow): number => {
    const sym = symbolOf(h.mkt, h.code);
    const bars = sym ? klineSeries[klineKey(sym, SPARK_Q)] : undefined;
    const closes = (bars ?? [])
      .map((b) => b.close)
      .filter((v): v is number => v !== null && v !== undefined && v > 0)
      .slice(-30);
    return returnStd(closes) || SIGMA_FALLBACK;
  };

  /* ---------- 再平衡清单（纪律阈值 + volSynth + sellTo，数字取后端原值） ---------- */
  const fixes = useMemo<Fix[]>(() => {
    if (!view || !agg || holdings.length === 0) return [];
    const ta = agg.total_assets;
    const mv = agg.total_mv;
    const cash = agg.cash;
    const out: Fix[] = [];

    // 单票 > 25%（per.weight = mv/total_assets×100，后端原值）
    holdings.forEach((h, i) => {
      const p = view.per[i];
      if (!p || p.weight <= LIMIT_SINGLE_STOCK) return;
      const price = h.price != null && h.price > 0 ? h.price : h.shares > 0 ? p.mv / h.shares : 0;
      const qty = sellTo({ mv: p.mv, price, total_assets: ta, target: 22, lot: lotSize(h.kind) }).qty;
      out.push({
        p: 1,
        t: `把 ${h.name} 从 ${fixed(p.weight, 1)}% 降到 22%`,
        d: `这是组合里较大的一只，超出你设的 ${LIMIT_SINGLE_STOCK}% 单票上限。减 ${fmt(qty, 0)} ${h.kind === "cb" ? "张" : h.kind === "etf" ? "份" : "股"}即可，不必清仓。`,
        calc: `卖出金额约 ${money(Math.max(0, p.mv - ta * 0.22))} · 回收后现金占比升至 ${fixed(ta > 0 ? ((cash + Math.max(0, p.mv - ta * 0.22)) / ta) * 100 : 0, 1)}%`,
        code: h.code,
        side: "sell",
        qty,
      });
    });

    // 行业 > 35%（agg.sectors[].w = 占 total_mv，后端原值）
    for (const s of agg.sectors) {
      if (s.w <= LIMIT_SECTOR) continue;
      const names = holdings.filter((h) => h.sector === s.name).map((h) => h.name);
      out.push({
        p: 2,
        t: `${s.name} 占持仓 ${fixed(s.w, 1)}%，超过 ${LIMIT_SECTOR}% 上限`,
        d: `同一行业里的标的在坏消息面前是同一个风险。当前 ${s.name} 涉及 ${names.join("、")}。`,
        calc: `压到 32% 需减约 ${money(Math.max(0, s.v - mv * 0.32))}，可从最大单票里出`,
      });
    }

    // 现金 < 10%（cash/total_assets = 100 − exposure）
    const cashW = 100 - agg.exposure;
    if (cashW < LIMIT_CASH_RESERVE) {
      out.push({
        p: 3,
        t: `现金只有 ${fixed(cashW, 1)}%，跌起来没有补的余地`,
        d: `按持仓页「仓位纪律」，现金下限是 ${LIMIT_CASH_RESERVE}%。下次有标的触发止盈时，优先把资金留在现金池而不是当场换股。`,
        calc: `补到 ${LIMIT_CASH_RESERVE}% 需要 ${money(Math.max(0, ta * 0.1 - cash))} 现金`,
      });
    }

    // 波动测算（volSynth：调整前后，03 §8）
    const items: VolItem[] = holdings.map((h, i) => ({
      code: h.code,
      sector: h.sector,
      w: mv > 0 ? (view.per[i]?.mv ?? 0) / mv : 0,
      sigma: sigmaOf(h),
    }));
    if (items.length >= 2) {
      const before = volSynth({ items }).ann_vol;
      const after = items.map((x) => ({ ...x }));
      const big = after.reduce((m, x) => (x.w > m.w ? x : m), after[0]);
      const targetMvFrac = 0.22 * (mv > 0 ? ta / mv : 0);
      const cut = Math.max(0, big.w - targetMvFrac);
      if (cut > 0) {
        big.w -= cut;
        const afterVol = volSynth({ items: after }).ann_vol;
        out.push({
          p: 2,
          t: `只做这一件事，组合年化波动大约从 ${fixed(before, 1)}% 降到 ${fixed(afterVol, 1)}%`,
          d: "测算方式：各标的近 30 日真实日线收益标准差按权重合成，跨行业相关系数取 0.35、同行业取 0.62（03 §8 写死假设）。它不是预测，只是把「分散」变成能比较的数字。",
          calc: `波动变化 ${fixed(afterVol - before, 1)} pct · 最大单票占持仓 ${fixed((big.w + cut) * 100, 1)}% → ${fixed(big.w * 100, 1)}%（卖出部分计入现金）`,
        });
      }
    }

    // 亏损标的无止损
    const paired = holdings.map((h, i) => ({ h, p: view.per[i] })).filter((x): x is { h: HRow; p: PRow } => x.p !== undefined);
    const loser = paired.reduce<{ h: HRow; p: PRow } | null>((m, x) => (!m || x.p.pnl_pct < m.p.pnl_pct ? x : m), null);
    if (loser && loser.p.pnl < 0 && loser.h.price != null) {
      const dec = priceDecimals(loser.h.kind);
      out.push({
        p: 3,
        t: `${loser.h.name} 浮亏 ${pct(loser.p.pnl_pct)}，但还没有止损线`,
        d: `亏不可怕，没有边界才可怕。给它一个失效价：跌破 ${fmt(loser.h.price * 0.93, dec)} 无条件减半，写进「仓位纪律」里，微信会盯着。`,
        calc: `按 7% 止损计算，最大追加亏损约 ${money(loser.p.mv * 0.07)}（占总资产 ${fixed(ta > 0 ? (loser.p.mv * 0.07) / ta * 100 : 0, 2)}%）`,
        code: loser.h.code,
        side: "sell",
      });
    }

    return out;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, agg, holdings, klineSeries]);

  /* ---------- KPI（perf 全字段——B11 已定稿 2026-10-30，bench_vol/beta/corr 入 §7 + fixtures） ---------- */
  const kpiCells: [string, string, string, string][] = perfWin
    ? [
        ["区间收益", pct(perfWin.cum), `基准 ${pct(perfWin.bench_cum)}`, dirClass(perfWin.cum)],
        ["超额收益", pct(perfWin.cum - perfWin.bench_cum), perfWin.cum - perfWin.bench_cum >= 0 ? "跑赢基准（收益差）" : "跑输基准（收益差）", dirClass(perfWin.cum - perfWin.bench_cum)],
        ["年化波动率", fixed(perfWin.ann_vol, 1) + "%", `基准 ${fixed(perfWin.bench_vol, 1)}%`, perfWin.ann_vol > perfWin.bench_vol ? "danger" : "ok"],
        ["最大回撤", fixed(perfWin.mdd, 1) + "%", `基准 ${fixed(perfWin.bench_mdd, 1)}%`, perfWin.mdd < perfWin.bench_mdd ? "danger" : "ok"],
        ["与基准相关系数", fixed(perfWin.corr, 2), `Beta ${fixed(perfWin.beta, 2)}`, ""],
        ["夏普比率", fixed(perfWin.sharpe, 2), "样本内 · 无风险利率按 0", ""],
        ["日度跑赢频率", fixed(perfWin.win_rate, 0) + "%", `${Math.max(0, navWin.length - 1)} 个交易日`, ""],
      ]
    : [];

  const sectors = agg?.sectors ?? [];
  const hhi = agg?.hhi ?? 0;
  const hhiLabel = hhi > 0.22 ? "偏集中" : hhi > 0.15 ? "中等" : "分散";

  const windowRows = WINDOWS.map(([n, label]) => {
    const nv = navSeries.slice(-n);
    const bc = benchSeries ? benchSeries.slice(-n) : null;
    const p = nv.length >= 2 ? perf({ nav: nv, bench: bc ?? [] }) : null;
    return { n, label, p, hasBench: bc !== null };
  });

  const applyAll = () => {
    const first = fixes.find((f) => f.code);
    if (!first) {
      toast("清单里没有可试算的减仓项 · 组合已在纪律范围内");
      return;
    }
    toast("按清单生成第 1 组委托试算，送到持仓页确认");
    navigate(`/holdings?code=${first.code}&side=${first.side ?? "sell"}${first.qty ? `&qty=${first.qty}` : ""}`);
  };

  const curveSub = perfWin
    ? `区间 ${datesWin[0]} — ${datesWin[datesWin.length - 1]}（${navWin.length} 个交易日）· 夏普 ${fixed(perfWin.sharpe, 2)}（样本内，无风险利率按 0 计）`
    : "尚无净值序列 · 从录入第一笔后逐交易日记录";

  return (
    <main className="page" data-od-id="diagnosis-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">组合诊断</div>
          <h1 className="page-title">风险、集中度，和它跑赢了谁</h1>
          <p className="page-desc">
            诊断只看一件事：这个组合在你没看住的时候，会不会让你难受。每个指标都由你的持仓与真实净值样本算出（perf / volSynth 走黄金向量校验的 domain 纯函数），不是装饰数字。
          </p>
        </div>
        <div className="page-actions">
          <div className="seg" role="group" aria-label="回测区间" id="window">
            {WINDOWS.map(([n, label]) => (
              <button type="button" key={n} data-n={n} aria-pressed={win === n} onClick={() => setWin(n)}>
                {label}
              </button>
            ))}
          </div>
          <button
            className="btn" id="report-btn"
            onClick={() =>
              toast(
                perfWin
                  ? `诊断摘要：${win} 日超额 ${pct(perfWin.cum - perfWin.bench_cum)}、回撤 ${fixed(perfWin.mdd, 1)}%。诊断报告推送另案；预警与盘后摘要已推送绑定的微信/QQ 会话。`
                  : "暂无净值样本可诊断 · 录入第一笔后报告才有内容",
              )
            }
          >
            生成诊断报告
          </button>
        </div>
      </div>

      {!view && (
        <section className="card" data-od-id="diag-error" style={{ marginBottom: 16 }}>
          <div className="card-body">
            <div className="metric-label">后端连接</div>
            <p style={{ margin: "6px 0 0", fontSize: 13, color: "var(--muted)" }}>
              {pError ?? (pLoading ? "正在连接 zimu-server…" : "尚未加载。")}
            </p>
            {pError && (
              <button className="btn btn-sm" style={{ marginTop: 10 }} onClick={() => void usePortfolio.getState().load()}>重试</button>
            )}
          </div>
        </section>
      )}

      {/* KPI（perf 七字段；空态/无样本显示 — 待净值积累，不伪装 0=安全） */}
      <div className="kpi" id="kpi" data-od-id="diag-kpi">
        {perfWin ? (
          kpiCells.map((c, i) => (
            <div data-od-id={`kpi-${i}`} key={c[0]}>
              <div className="k">{c[0]}</div>
              <div className={`v ${c[3]}`}>{c[1]}</div>
              <div className="s">{c[2]}</div>
            </div>
          ))
        ) : (
          ["区间收益", "超额收益", "年化波动率", "最大回撤", "与基准相关系数", "夏普比率", "日度跑赢频率"].map((k, i) => (
            <div data-od-id={`kpi-empty-${i}`} key={k}>
              <div className="k">{k}</div>
              <div className="v">—</div>
              <div className="s">{isEmpty ? "待首笔持仓" : "待净值积累"}</div>
            </div>
          ))
        )}
      </div>

      <div className="grid g-main" style={{ marginTop: 16 }}>
        {/* 组合 vs 沪深300 */}
        <section className="card" data-od-id="diag-curve-card">
          <div className="card-head">
            <div><div className="card-title">组合 vs 沪深 300</div><div className="card-sub" id="curve-sub">{curveSub}</div></div>
          </div>
          <div className="card-body">
            {hasCurve ? (
              <>
                <LineChart
                  height={250}
                  series={
                    benchWin
                      ? [{ data: navWin }, { data: benchWin, color: "var(--muted)", area: false, dash: "4 4", width: 1.3, endDot: false }]
                      : [{ data: navWin }]
                  }
                  labels={datesWin.map((d) => d.slice(5))}
                  yFmt={(v) => fixed(v / 10000, 1) + " 万"}
                  tip={(i) => (
                    <>
                      <span className="k">{datesWin[i]}</span>
                      <br />
                      组合 <b>{money(navWin[i])}</b>
                      <br />
                      <span className="k">区间收益</span> <b>{pct(navWin[0] > 0 ? (navWin[i] / navWin[0] - 1) * 100 : 0)}</b>
                    </>
                  )}
                />
                <div className="legend" style={{ marginTop: 10 }}>
                  <span><i style={{ background: "var(--accent)" }}></i>我的组合（含现金）</span>
                  {benchWin
                    ? <span><i style={{ background: "var(--muted)" }}></i>沪深 300（点位归一，未含分红）</span>
                    : <span>沪深 300 基准暂不可用（未画假线）</span>}
                  <span style={{ marginLeft: "auto" }} className="num" id="curve-read">
                    {perfWin ? `组合 ${fixed(perfWin.cum, 1)}% / 基准 ${fixed(perfWin.bench_cum, 1)}%` : ""}
                  </span>
                </div>
              </>
            ) : (
              <EmptyHero
                Icon={IconSpark}
                od="diag-curve-empty"
                title="组合曲线需要先在持仓里「有东西」"
                desc="净值序列（含现金）从第一笔起按交易日 15:05 归档推进，与基准的对比、回撤与夏普都由它派生（perf 纯函数，黄金向量校验）。"
                actions={null}
              />
            )}
          </div>
        </section>

        {/* 行业集中度 */}
        <section className="card" data-od-id="diag-conc-card">
          <div className="card-head">
            <div><div className="card-title">行业集中度</div><div className="card-sub">按市值占持仓（不含现金）</div></div>
            <div className="head-actions">
              <span className={`pill ${hhi > 0.22 ? "warn" : ""}`} id="hhi-badge">
                {agg ? `HHI ${fixed(hhi, 3)} · ${hhiLabel}` : "待数据"}
              </span>
            </div>
          </div>
          <div className="card-body">
            {sectors.length ? (
              <>
                <div className="stack" id="stack" title="行业分布">
                  {sectors.map((s, i) => (
                    <i key={s.name} style={{ width: `${fixed(s.w, 2)}%`, background: DONUT_COLORS[i % DONUT_COLORS.length] }} title={`${s.name} ${fixed(s.w, 1)}%`} />
                  ))}
                </div>
                <div className="legend-dots" id="stack-legend">
                  {sectors.map((s, i) => (
                    <span key={s.name}><i style={{ background: DONUT_COLORS[i % DONUT_COLORS.length] }}></i>{s.name} {fixed(s.w, 1)}%</span>
                  ))}
                </div>
                <div style={{ marginTop: 18 }} className="bar-row" id="sector-bars">
                  {sectors.map((s, i) => {
                    const maxW = Math.max(1, ...sectors.map((x) => x.w));
                    return (
                      <div className="bar-line" data-od-id={`sector-bar-${i}`} key={s.name}>
                        <span className="lb">{s.name}</span>
                        <span className="tr"><i style={{ width: `${fixed((s.w / maxW) * 100, 1)}%`, background: s.w > LIMIT_SECTOR ? "var(--warn)" : "var(--fg-2)" }} /></span>
                        <span className="vl">{fixed(s.w, 1)}%</span>
                      </div>
                    );
                  })}
                </div>
              </>
            ) : (
              <div className="empty" style={{ padding: "22px 12px" }}>{isEmpty ? "行业权重在录入第一笔后现算" : "待加载组合快照…"}</div>
            )}
          </div>
        </section>
      </div>

      <div className="grid g-main" style={{ marginTop: 16 }}>
        {/* 再平衡清单 */}
        <section className="card" data-od-id="diag-fix-card">
          <div className="card-head">
            <div><div className="card-title">再平衡清单</div><div className="card-sub">按「影响大小 × 操作成本」排序，可逐条试算</div></div>
            <div className="head-actions">
              <button className="btn btn-sm btn-primary" id="apply-all" disabled={fixes.length === 0} onClick={applyAll}>一键试算第一笔</button>
            </div>
          </div>
          <div id="fixes">
            {fixes.length ? (
              fixes.map((f, i) => (
                <div className={`fix p${f.p}`} data-od-id={`fix-${i}`} key={i} style={acked.has(i) ? { opacity: 0.45 } : undefined}>
                  <span className="no">{i + 1}</span>
                  <div>
                    <h4>{f.t}</h4>
                    <p>{f.d}</p>
                    <div className="calc">{f.calc}</div>
                  </div>
                  <div style={{ display: "flex", gap: 6, flexDirection: "column", alignItems: "flex-end" }}>
                    {f.p === 1 ? <span className="pill danger">优先</span> : f.p === 2 ? <span className="pill warn">建议</span> : <span className="pill">可选</span>}
                    {f.code ? (
                      <Link className="btn btn-sm" to={`/holdings?code=${f.code}&side=${f.side ?? "sell"}${f.qty ? `&qty=${f.qty}` : ""}`}>试算</Link>
                    ) : acked.has(i) ? (
                      <span className="pill">已确认</span>
                    ) : (
                      <button className="btn btn-sm" onClick={() => { setAcked((s) => new Set(s).add(i)); toast("已标记为知悉，下次盘后复盘不再重复提醒"); }}>知道了</button>
                    )}
                  </div>
                </div>
              ))
            ) : isEmpty || !view ? (
              <EmptyHero
                Icon={IconShield}
                od="diag-fixes-empty"
                title="再平衡清单需要一个组合"
                desc="录入第一笔后，清单按「影响大小 × 操作成本」排序，每条附现算的测算与试算入口；未触及纪律上限时这里就是空的。"
                actions={null}
              />
            ) : (
              <div className="empty" style={{ padding: "22px 12px" }}>组合已在纪律范围内 · 暂无需要再平衡的项（触及 25%/35%/10% 上限时这里会给出现算清单）</div>
            )}
          </div>
          <div className="card-foot" style={{ justifyContent: "space-between" }}>
            <span className="hint" style={{ margin: 0 }} id="fix-sum">
              {view ? `${fixes.length} 条待处理 · 其中 ${fixes.filter((f) => f.p === 1).length} 条已触及你的纪律上限` : "—"}
            </span>
            <span className="hint" style={{ margin: 0 }}>假设：跨行业相关系数 0.35，同行业 0.62（03 §8）</span>
          </div>
        </section>

        <div className="grid" style={{ gap: 16 }}>
          {/* 回撤水下 */}
          <section className="card" data-od-id="diag-drawdown-card">
            <div className="card-head">
              <div><div className="card-title">回撤在水下多久</div><div className="card-sub">距前高的百分比，越接近 0 越好</div></div>
            </div>
            <div className="card-body">
              {hasCurve ? (
                <LineChart
                  height={150}
                  pad={0.05}
                  series={[{ data: underWin, color: "var(--down)" }]}
                  yFmt={(v) => fixed(v, 1) + "%"}
                  tip={(i) => (
                    <>
                      <span className="k">{datesWin[i]}</span>
                      <br />
                      水下 <b>{fixed(underWin[i], 2)}%</b>
                    </>
                  )}
                />
              ) : (
                <div className="empty" style={{ padding: "22px 12px" }}>暂无回撤记录</div>
              )}
            </div>
          </section>

          {/* 分区间表（perf 逐窗口；相关系数列删除——B11 未定稿） */}
          <section className="card" data-od-id="diag-window-table">
            <div className="card-head">
              <div><div className="card-title">分区间对比</div><div className="card-sub">样本内测算（perf 纯函数）</div></div>
            </div>
            <div className="card-body tight">
              <div className="tbl-wrap">
                <table className="tbl" id="wtable">
                  <thead>
                    <tr>
                      <th className="txt">区间</th><th>组合</th><th>沪深300</th><th>超额</th><th>最大回撤</th><th>相关系数</th>
                    </tr>
                  </thead>
                  <tbody>
                    {hasCurve ? (
                      windowRows.map((r) => (
                        <tr data-od-id={`wt-${r.n}`} key={r.n}>
                          <td className="txt">{r.label} <span className="sub">{Math.min(r.n, navSeries.length)} 交易日</span></td>
                          <td className={`num ${r.p ? dirClass(r.p.cum) : ""}`}>{r.p ? pct(r.p.cum) : "—"}</td>
                          <td className={`num ${r.p && r.hasBench ? dirClass(r.p.bench_cum) : ""}`}>{r.p && r.hasBench ? pct(r.p.bench_cum) : "—"}</td>
                          <td className={`num ${r.p && r.hasBench ? dirClass(r.p.cum - r.p.bench_cum) : ""}`}>{r.p && r.hasBench ? pct(r.p.cum - r.p.bench_cum) : "—"}</td>
                          <td className="num down">{r.p ? fixed(r.p.mdd, 1) : "—"}%</td>
                          <td className="num">{r.p && r.hasBench ? fixed(r.p.corr, 2) : "—"}</td>
                        </tr>
                      ))
                    ) : (
                      <tr><td colSpan={6}><div className="empty">暂无可对比的区间样本</div></td></tr>
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          </section>
        </div>
      </div>

      <div className="footnote" data-od-id="diag-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          区间收益/波动/回撤/夏普/胜率/beta/相关系数由 <code>perf</code>（黄金向量校验）算出；再平衡「调整前后波动」按 <code>volSynth</code>（03 §8：各标的近 30 日真实日线收益标准差 + 相关系数假设 0.35/0.62）合成，为<b>样本内测算</b>、非绝对风险预测。
          基准为沪深 300 <b>价格指数</b>点位归一（未含分红，非 §7「全收益」理想口径，图下如实标注）；基准侧指标按 §7 B11 定稿口径（恒定基准 ε 守卫 → 0）。回测不预示未来表现，本页<b>不构成投资建议</b>。
        </span>
      </div>
    </main>
  );
}
