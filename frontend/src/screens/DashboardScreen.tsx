import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import Donut from "../components/charts/Donut";
import LineChart from "../components/charts/LineChart";
import Spark from "../components/charts/Spark";
import { cssVar } from "../components/charts/svg";
import EmptyHero from "../components/EmptyHero";
import { IconBell, IconSpark } from "../components/Icons";
import { perf, priceDecimals } from "../domain";
import { alignBench } from "../lib/bench";
import { clockView, marketClock } from "../lib/clock";
import { dirClass, fixed, fmt, money, pct, wan } from "../lib/format";
import { KIND_LABEL, symbolOf } from "../lib/kind";
import { useAdvice } from "../store/advice";
import { useAlerts } from "../store/alerts";
import { useEquity } from "../store/equity";
import { klineKey, useKline } from "../store/kline";
import { useMarket } from "../store/market";
import { usePortfolio, type PortfolioView } from "../store/portfolio";
import { usePushLog } from "../store/push_log";
import { useUi } from "../store/ui";
import { useWatchlist, type NewWatchlist } from "../store/watchlist";

/**
 * 总览仪表盘（dashboard 屏，P5-T2 真实数据移植）—— 对照基准 design/dashboard.html。
 * 数字纪律（ADR-0008）：总资产/当日盈亏/归因/仓位全部是后端 recalc 原值；
 * 净值与收盘摘要读 equity_snapshots（15:05 归档原值）；区间收益/回撤/跑赢跑输走
 * src/domain 的 perf 纯函数（03 §7，黄金向量同款）；沪深 300 基准 = 指数日线点位
 * 归一到区间净值起点（价格指数、未含分红——B11 基准口径待定稿，图上如实标注）。
 * 自选卡 = B13 前端半边：CRUD 走 /api/watchlist，展示行情单独 quote 合并且永不回写。
 * 图表手写 SVG（components/charts/*，镜像 ZMC.line/spark/donut）。
 */

type HRow = PortfolioView["holdings"][number];
type PRow = PortfolioView["per"][number];

/** 沪深 300（基准曲线）；spark 走各持仓自身 symbol */
const BENCH_SYMBOL = "sh000300";
/** 迷你走势线窗口（原型：30 日走势） */
const SPARK_DAYS = 30;
/** spark/基准 kline 查询：盘中合成当日 bar（04 附录 B），与缓存键一致 */
const SPARK_Q = { withToday: true } as const;

const PERIODS: [number, string][] = [
  [5, "近一周"],
  [20, "近一月"],
  [90, "近一季"],
];

/** 空持仓态收盘摘要引导（原型 renderBrief emptyState 分支原文） */
const EMPTY_STEPS: [string, string][] = [
  ["录入第一笔持仓", "入口在持仓管理页：手工逐笔加仓，数量按整手校验、价格受当日涨跌停约束，费用与摊薄成本在预览里即时试算。"],
  ["录入后即被监控", "助理基于同一份快照计算派生数字与净值序列；价格条件、公告事件触发时生成预警，按你的设置推送微信。"],
  ["收盘自动生成摘要", "15:05 归档当日盈亏、仓位提醒与下日关注点——摘要里的每个数字都可回溯到口径手册。"],
];

const ALERT_DOT: Record<string, string> = { up: "var(--up)", down: "var(--down)", warn: "var(--warn)" };
const ALERT_PILL: Record<string, string> = { up: "up", down: "down", warn: "warn" };

const EMPTY_FORM = { code: "", name: "", mkt: "SH", kind: "stock", sector: "", note: "", limitPct: "" };

export default function DashboardScreen({ now }: { now: Date }) {
  const navigate = useNavigate();
  const view = usePortfolio((s) => s.view);
  const pLoading = usePortfolio((s) => s.loading);
  const pError = usePortfolio((s) => s.error);
  const status = useMarket((s) => s.status);
  const equityRows = useEquity((s) => s.rows);
  const alertRows = useAlerts((s) => s.rows);
  const alertsLoaded = useAlerts((s) => s.loaded);
  const alertsError = useAlerts((s) => s.error);
  const pushRows = usePushLog((s) => s.rows);
  const adviceRows = useAdvice((s) => s.rows);
  const watchRows = useWatchlist((s) => s.rows);
  const watchQuotes = useWatchlist((s) => s.quotes);
  const watchError = useWatchlist((s) => s.error);
  const refreshTick = useUi((s) => s.refreshTick);

  const [n, setN] = useState(90);
  const [alertBusy, setAlertBusy] = useState(false);
  const [adding, setAdding] = useState(false);
  const [form, setForm] = useState({ ...EMPTY_FORM });

  const toast = (msg: string) => useUi.getState().pushToast(msg);
  const agg = view?.agg;
  const holdings = view?.holdings ?? [];
  const isEmpty = view !== null && holdings.length === 0;
  const clock = status ? clockView(status.clock, now) : marketClock(now);

  /* ---------- 数据接线 ---------- */

  // 自选：进屏拉列表 + 行情；盘中 60s 重拉行情（与 App 全局轮询同节奏，休市不打 sidecar）
  useEffect(() => {
    const st = useWatchlist.getState();
    void st.load().then(() => useWatchlist.getState().loadQuotes());
    const t = window.setInterval(() => {
      if (useMarket.getState().status?.clock.live) void useWatchlist.getState().loadQuotes();
    }, 60_000);
    return () => window.clearInterval(t);
  }, []);
  // 顶栏手动刷新：自选行情重拉
  useEffect(() => {
    if (refreshTick > 0) void useWatchlist.getState().loadQuotes();
  }, [refreshTick]);

  // 基准日线：窗口起点 = 最早快照日（覆盖净值序列全range）；无快照不拉
  const benchStart = equityRows.length > 0 ? equityRows[0].date : null;
  const benchQ = useMemo(
    () => (benchStart ? { start: benchStart, withToday: true } : null),
    [benchStart],
  );
  useEffect(() => {
    if (!benchQ) return;
    if (refreshTick === 0) void useKline.getState().load(BENCH_SYMBOL, benchQ);
    else void useKline.getState().reload(BENCH_SYMBOL, benchQ);
  }, [benchQ, refreshTick]);

  // 持仓 spark 日线：缓存命中不重复请求（60s 轮询换 view 引用也不放大调用）
  const holdSymsKey = holdings
    .map((h) => symbolOf(h.mkt, h.code))
    .filter((s): s is string => s !== null)
    .join(",");
  useEffect(() => {
    for (const sym of holdSymsKey.split(",").filter(Boolean)) {
      if (refreshTick === 0) void useKline.getState().load(sym, SPARK_Q);
      else void useKline.getState().reload(sym, SPARK_Q);
    }
  }, [holdSymsKey, refreshTick]);

  const benchKey = benchQ ? klineKey(BENCH_SYMBOL, benchQ) : null;
  const benchBars = useKline((s) => (benchKey ? s.series[benchKey] : undefined));
  const klineSeries = useKline((s) => s.series);

  /* ---------- 净值曲线（切片 + 基准对齐 + perf） ---------- */

  const navRows = useMemo(() => equityRows.slice(-n), [equityRows, n]);
  const navSeries = useMemo(() => navRows.map((r) => r.total_assets), [navRows]);
  const navDates = useMemo(() => navRows.map((r) => r.date), [navRows]);

  // 基准对齐（carry-forward + 归一到区间起点）抽到 lib/bench，dashboard/diagnosis 共用
  const benchSeries = useMemo(
    (): number[] | null => alignBench(navDates, benchBars, navSeries[0] ?? 0),
    [benchBars, navRows], // eslint-disable-line react-hooks/exhaustive-deps
  );

  const perfM = useMemo(
    () => (navSeries.length >= 2 ? perf({ nav: navSeries, bench: benchSeries ?? [] }) : null),
    [navSeries, benchSeries],
  );

  /* ---------- 持仓派生视图（后端 per 原值，仅排序/配对） ---------- */

  const paired = useMemo(() => {
    if (!view) return [] as { h: HRow; p: PRow }[];
    return view.holdings
      .map((h, i) => ({ h, p: view.per[i] }))
      .filter((x): x is { h: HRow; p: PRow } => x.p !== undefined);
  }, [view]);
  const holdRows = useMemo(() => [...paired].sort((a, b) => b.p.mv - a.p.mv), [paired]);

  const bestP = paired.reduce<{ h: HRow; p: PRow } | null>(
    (m, x) => (!m || x.p.day_val > m.p.day_val ? x : m),
    null,
  );
  const worstP = paired.reduce<{ h: HRow; p: PRow } | null>(
    (m, x) => (!m || x.p.day_val < m.p.day_val ? x : m),
    null,
  );

  /* 归因：|day_val| 降序（原型同款）；数字全是 per 原值 */
  const attr = useMemo(
    () => paired.map(({ h, p }) => ({ k: h.name, v: p.day_val })).sort((a, b) => Math.abs(b.v) - Math.abs(a.v)),
    [paired],
  );
  const attrMax = Math.max(1, ...attr.map((a) => Math.abs(a.v)));
  const attrUp = attr.filter((a) => a.v > 0).length;
  const attrDown = attr.filter((a) => a.v < 0).length;
  const attrNote =
    attr.length >= 2 && agg
      ? `${attrUp} 涨 / ${attrDown} 跌，前两只贡献了今日 ${fixed(((attr[0].v + attr[1].v) / (agg.day_pnl || 1)) * 100, 0)}% 的变动。`
      : attr.length === 1
        ? `${attrUp} 涨 / ${attrDown} 跌，当前只有一只标的贡献当日变动。`
        : "当日无标的贡献 —— 账户目前全部是现金。";

  /* ---------- 文案 ---------- */

  const dateLine =
    now.toLocaleDateString("zh-CN", { month: "long", day: "numeric", weekday: "long" }) + " · " + clock.state;
  // 原型口径：live 或已收盘 → 「今日」；盘前/非交易日 → 数字实为上一交易日收盘态
  const dayWord = status
    ? status.clock.state === "non_trading_day" || status.clock.state === "pre_open"
      ? "上一交易日"
      : "今日"
    : now.getDay() === 0 || now.getDay() === 6 || now.getHours() * 60 + now.getMinutes() < 9 * 60 + 15
      ? "上一交易日"
      : "今日";

  const desc = !view
    ? pError
      ? `后端不可用：${pError}。启动 zimu-server（cargo run -p zimu-server）后点顶栏刷新重试。`
      : pLoading
        ? "正在读取组合与行情…"
        : "尚未连接后端。"
    : isEmpty
      ? "账户还没有持仓：总资产目前全部是现金。录入第一笔后，这里开始总结当日涨跌、归因与仓位提醒。"
      : agg && bestP && worstP
        ? `${dayWord}组合 ${agg.day_pnl >= 0 ? "盈利" : "亏损"} ${fmt(Math.abs(agg.day_pnl), 0)} 元：` +
          `主要是 ${bestP.h.name} ${pct(bestP.p.day_chg)}，${worstP.h.name} ${pct(worstP.p.day_chg)} 拖累最多。` +
          `仓位 ${agg.exposure.toFixed(1)}%，累计浮盈 ${pct(agg.total_pnl_pct)}。`
        : "正在读取组合快照…";

  const equitySub = isEmpty
    ? "净值序列从录入第一笔后开始逐交易日记录（含现金）"
    : navRows.length >= 2 && perfM
      ? `区间 ${navDates[0]} — ${navDates[navDates.length - 1]} · 本区间最大回撤 ${fixed(perfM.mdd, 1)}%` +
        (benchSeries ? "" : " · 沪深 300 日线暂不可用（基准未画，不伪装）")
      : "净值快照每交易日 15:05 归档（recalc 原值）；基准为沪深 300 价格指数点位归一，未含分红（B11 口径待定稿）";

  const badge =
    !isEmpty && perfM && benchSeries
      ? {
          cls: `pill ${perfM.cum - perfM.bench_cum >= 0 ? "up" : "down"}`,
          text: `近 ${navRows.length} 交易日 ${perfM.cum - perfM.bench_cum >= 0 ? "跑赢" : "跑输"} 沪深300 ${Math.abs(perfM.cum - perfM.bench_cum).toFixed(2)} pct`,
        }
      : { cls: "pill", text: isEmpty ? "待首笔持仓" : navRows.length < 2 ? "待净值积累" : "基准不可用" };

  /* ---------- 预警 ---------- */

  const pendCount = alertRows.filter((a) => !a.done).length;
  const markOne = async (id: number) => {
    setAlertBusy(true);
    await useAlerts.getState().markDone(id);
    setAlertBusy(false);
    // 以 store 实际行态为准报结果（后端失败时 error 有文案，不谎报成功）
    const row = useAlerts.getState().rows.find((r) => r.id === id);
    toast(row?.done ? "已标记处理，该预警不再出现在首屏" : (useAlerts.getState().error ?? "归档失败"));
  };
  const markAll = async () => {
    const rows = useAlerts.getState().rows.filter((r) => !r.done);
    if (rows.length === 0) {
      toast("暂无预警可归档 · 录入第一笔后开始监控");
      return;
    }
    setAlertBusy(true);
    for (const r of rows) await useAlerts.getState().markDone(r.id);
    setAlertBusy(false);
    const left = useAlerts.getState().rows.filter((r) => !r.done).length;
    toast(left === 0 ? "全部预警已归档" : `${left} 条归档失败（后端不可用？）`);
  };

  /* ---------- 自选（B13 前端半边） ---------- */

  const heldSectors = useMemo(
    () => new Set(holdings.map((h) => h.sector).filter((s) => s !== "")),
    [holdings],
  );
  const submitAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    const code = form.code.trim();
    const name = form.name.trim();
    if (!/^[0-9]{6}$/.test(code) || !name) {
      toast("代码需 6 位数字，名称必填");
      return;
    }
    // NaN 会被 JSON 序列化成 null 静默吞掉——显式拦
    if (form.limitPct.trim() !== "" && !Number.isFinite(Number(form.limitPct))) {
      toast("涨跌停 % 需是数字（如 10 或 20），留空 = 按板块推断");
      return;
    }
    const body: NewWatchlist = {
      code,
      name,
      mkt: form.mkt,
      kind: form.kind as NewWatchlist["kind"],
      sector: form.sector.trim(),
      note: form.note.trim(),
      ...(form.limitPct.trim() !== "" ? { limit_pct: Number(form.limitPct) } : {}),
    };
    const ok = await useWatchlist.getState().add(body);
    toast(ok ? `已加入自选：${name}` : (useWatchlist.getState().error ?? "添加自选失败"));
    if (ok) {
      setAdding(false);
      setForm({ ...EMPTY_FORM });
    }
  };
  const removeWatch = async (code: string, name: string) => {
    const ok = await useWatchlist.getState().remove(code);
    toast(ok ? `已移除自选 ${name}（判定范围同步缩回，重新添加即恢复）` : (useWatchlist.getState().error ?? "移除失败"));
  };

  /* ---------- 收盘摘要（全部 15:05 归档原值 + push_log 留痕） ---------- */

  const lastSnap = equityRows.length > 0 ? equityRows[equityRows.length - 1] : null;
  const lastReview = pushRows.find((r) => r.kind === "post_market_review") ?? null;
  const adviceOnSnap = lastSnap ? adviceRows.filter((a) => a.generated_at.startsWith(lastSnap.date)).length : 0;
  const brief: [string, string][] = lastSnap
    ? [
        [
          "组合今天怎么了",
          `${lastSnap.date} 收盘归档：${lastSnap.day_pnl >= 0 ? "盈利" : "亏损"} ${fmt(Math.abs(lastSnap.day_pnl), 0)} 元（${pct(lastSnap.day_pnl_pct)}），总资产 ${money(lastSnap.total_assets)}，仓位 ${lastSnap.exposure.toFixed(1)}%。`,
        ],
        [
          "仓位提醒",
          `HHI ${lastSnap.hhi.toFixed(3)}，前三行业合计占持仓 ${lastSnap.top3_sector.toFixed(1)}%（行业口径）；与 25%/35% 上限的对照在「组合诊断」页逐条走。`,
        ],
        [
          "复盘留痕",
          lastReview
            ? `${lastReview.ts} 盘后复盘${lastReview.ok ? "已受理" : `受理失败：${lastReview.detail}`}；快照当日建议留档 ${adviceOnSnap} 条（数字归规则、文字归模型，ADR-0007）。`
            : "尚无盘后复盘留痕：交易日 15:05 自动触发，agent 离线也写快照与留痕（ok=0 如实标注）。",
        ],
      ]
    : [];

  const field = (label: string, node: React.ReactNode) => (
    <div className="field">
      <label>{label}</label>
      {node}
    </div>
  );

  /* ---------- 渲染 ---------- */

  return (
    <main className="page" data-od-id="dashboard-page">
      <div className="page-head">
        <div>
          <div className="eyebrow" id="d-eyebrow">{dateLine}</div>
          <h1 className="page-title">今天我的钱在哪里</h1>
          <p className="page-desc" id="d-desc">{desc}</p>
        </div>
        <div className="page-actions">
          <div className="seg" role="group" aria-label="时间区间" id="period">
            {PERIODS.map(([v, label]) => (
              <button type="button" key={v} data-n={v} aria-pressed={n === v} onClick={() => setN(v)}>
                {label}
              </button>
            ))}
          </div>
          <Link className="btn" to="/holdings">管理持仓</Link>
        </div>
      </div>

      {/* 后端不可用：错误横幅 + 重试（旧数字不伪装） */}
      {!view && (
        <section className="card" data-od-id="dash-error" style={{ marginBottom: 16 }}>
          <div className="card-body">
            <div className="metric-label">后端连接</div>
            <p style={{ margin: "6px 0 0", fontSize: 13, color: "var(--muted)" }}>
              {pError ?? (pLoading ? "正在连接 zimu-server…" : "尚未加载。")}
            </p>
            {pError && (
              <button className="btn btn-sm" style={{ marginTop: 10 }} onClick={() => void usePortfolio.getState().load()}>
                重试
              </button>
            )}
          </div>
        </section>
      )}

      {/* 净值 + 今日概览 */}
      <div className="grid g-main">
        <section className="card" data-od-id="equity-card">
          <div className="card-head">
            <div>
              <div className="card-title">资产净值 vs 沪深 300</div>
              <div className="card-sub" id="equity-sub">{equitySub}</div>
            </div>
            <div className="head-actions">
              <span className={badge.cls} id="equity-badge">{badge.text}</span>
            </div>
          </div>
          <div className="card-body">
            <div style={{ display: "flex", alignItems: "flex-end", gap: 26, flexWrap: "wrap", marginBottom: 14 }}>
              <div>
                <div className="metric-label">总资产</div>
                <div className="metric-value lg num" id="d-assets">{agg ? money(agg.total_assets) : "—"}</div>
              </div>
              <div>
                <div className="metric-label">当日盈亏</div>
                <div className={`metric-value num ${agg ? dirClass(agg.day_pnl) : ""}`} id="d-day" style={{ marginTop: 6 }}>
                  {agg ? (agg.day_pnl > 0 ? "+" : "") + fmt(agg.day_pnl, 0) : "—"}
                </div>
              </div>
              <div>
                <div className="metric-label">区间收益</div>
                <div className={`metric-value num ${perfM ? dirClass(perfM.cum) : ""}`} id="d-range" style={{ marginTop: 6 }}>
                  {perfM ? pct(perfM.cum) : "—"}
                </div>
              </div>
            </div>
            {isEmpty ? (
              <EmptyHero
                Icon={IconSpark}
                od="dash-equity-empty"
                title="录入第一笔后，这里开始画组合净值 vs 沪深 300"
                desc="曲线按真实交易日推进；归因、预警与收盘摘要读的是同一份快照。"
              />
            ) : navRows.length === 0 ? (
              <EmptyHero
                Icon={IconSpark}
                od="dash-equity-empty"
                title="净值快照待归档"
                desc="录入持仓后的首个交易日 15:05 自动归档第一份快照（recalc 原值，date 主键幂等），曲线从那里开始推进。"
                actions={null}
              />
            ) : (
              <>
                <LineChart
                  height={250}
                  series={
                    benchSeries
                      ? [
                          { data: navSeries },
                          { data: benchSeries, color: cssVar("--muted"), area: false, dash: "4 4", width: 1.3, endDot: false },
                        ]
                      : [{ data: navSeries }]
                  }
                  labels={navDates.map((d) => d.slice(5))}
                  yFmt={(v) => fixed(v / 10000, 1) + " 万"}
                  tip={(i) => (
                    <>
                      <span className="k">{navDates[i]}</span>
                      <br />
                      组合 <b>{money(navSeries[i])}</b>
                      {benchSeries && (
                        <>
                          {" "}· 基准 <b>{wan(benchSeries[i])}</b>
                        </>
                      )}
                      <br />
                      <span className="k">区间</span>{" "}
                      <b>{pct(navSeries[0] > 0 ? (navSeries[i] / navSeries[0] - 1) * 100 : 0)}</b>
                    </>
                  )}
                />
                <div style={{ height: 12 }} />
              </>
            )}
            <div className="legend" style={{ marginTop: 10 }}>
              <span><i style={{ background: "var(--accent)" }}></i>我的组合</span>
              {benchSeries ? (
                <span><i style={{ background: "var(--muted)", opacity: 0.7 }}></i>沪深 300（点位归一到区间起点）</span>
              ) : (
                <span>沪深 300 基准{isEmpty || navRows.length === 0 ? "待净值积累" : "暂不可用（未画假线）"}</span>
              )}
              <span style={{ marginLeft: "auto" }} id="d-hover">把指针放到图上查看某一天的净值</span>
            </div>
          </div>
        </section>

        <div className="grid" style={{ gap: 16 }}>
          {/* 钱在哪里（recalc agg 原值拆分） */}
          <section className="card" data-od-id="allocation-card">
            <div className="card-head">
              <div>
                <div className="card-title">钱在哪里</div>
                <div className="card-sub">按市值拆分</div>
              </div>
            </div>
            <div className="card-body">
              {agg ? (
                <>
                  <div style={{ display: "flex", gap: 18, alignItems: "center" }}>
                    <div style={{ flex: "none" }}>
                      <Donut
                        size={132}
                        thick={16}
                        items={[
                          { name: "股票", v: agg.mv_stocks },
                          { name: "ETF / 基金", v: agg.mv_etf },
                          { name: "可转债", v: agg.mv_cb },
                          { name: "现金", v: agg.cash },
                        ]}
                      />
                    </div>
                    <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 9 }} id="alloc-list">
                      {[
                        { k: "股票", v: agg.mv_stocks },
                        { k: "ETF / 基金", v: agg.mv_etf },
                        { k: "可转债", v: agg.mv_cb },
                        { k: "现金", v: agg.cash },
                      ].map((a, i) => (
                        <div
                          key={a.k}
                          style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5 }}
                          data-od-id={`alloc-row-${i}`}
                        >
                          <i
                            style={{
                              width: 9, height: 9, borderRadius: 3, flex: "none",
                              background: ["oklch(58% 0.18 255)", "oklch(55% 0.19 22)", "oklch(62% 0.15 78)", "oklch(56% 0.12 170)"][i],
                            }}
                          />
                          <span>{a.k}</span>
                          <span className="num" style={{ marginLeft: "auto", color: "var(--fg-2)" }}>{fmt(a.v, 0)}</span>
                          <span className="num" style={{ width: 46, textAlign: "right", color: "var(--muted)" }}>
                            {agg.total_assets > 0 ? ((a.v / agg.total_assets) * 100).toFixed(1) : "0.0"}%
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                  <div style={{ marginTop: 16, paddingTop: 14, borderTop: "1px solid var(--border)", display: "flex", gap: 8, alignItems: "baseline" }}>
                    <span className="metric-label">可用现金</span>
                    <span className="num" id="d-cash" style={{ marginLeft: "auto", fontSize: 15, fontWeight: 550 }}>{money(agg.cash)}</span>
                  </div>
                </>
              ) : (
                <div className="empty">待加载组合快照…</div>
              )}
            </div>
          </section>

          {/* 当日盈亏归因（per[].day_val 原值，仅排序与画条） */}
          <section className="card" data-od-id="attribution-card">
            <div className="card-head">
              <div>
                <div className="card-title">当日盈亏归因</div>
                <div className="card-sub">谁在拖、谁在拉</div>
              </div>
            </div>
            <div className="card-body">
              <div className="bar-row" id="attribution">
                {attr.length === 0 ? (
                  <div className="empty" style={{ padding: "26px 12px" }}>
                    {view ? "还没有持仓可归因 · 录入第一笔后自动统计" : "待加载组合快照…"}
                  </div>
                ) : (
                  attr.map((a) => (
                    <div className="bar-line" data-od-id={`attr-${a.k}`} key={a.k}>
                      <span className="lb">{a.k}</span>
                      <span className="tr" style={{ display: "flex", alignItems: "center", gap: 0 }}>
                        <i
                          style={{
                            width: `${((Math.abs(a.v) / attrMax) * 100).toFixed(1)}%`,
                            background: a.v >= 0 ? "var(--up)" : "var(--down)",
                            float: "left",
                          }}
                        />
                      </span>
                      <span className={`vl ${dirClass(a.v)}`}>{(a.v > 0 ? "+" : "") + fmt(a.v, 0)}</span>
                    </div>
                  ))
                )}
              </div>
              <p className="hint" id="attr-note" style={{ marginTop: 12 }}>{view ? attrNote : ""}</p>
            </div>
          </section>
        </div>
      </div>

      {/* 预警（判定全在后端调度器；前端只读取与归档） */}
      <section className="card" id="alerts" data-od-id="alerts-card" style={{ marginTop: 16 }}>
        <div className="card-head">
          <div>
            <div className="card-title">待处理预警</div>
            <div className="card-sub">价格条件、公告与止盈止损（持仓 + 自选，03 §12），处理后不再出现在首屏</div>
          </div>
          <div className="head-actions">
            <span className={`pill ${pendCount ? "warn" : ""}`} id="alert-count">
              {pendCount ? `${pendCount} 条待处理` : isEmpty && alertRows.length === 0 ? "未启动" : "已全部处理"}
            </span>
            <button className="btn btn-sm" id="alert-all" disabled={alertBusy} onClick={() => void markAll()}>
              全部标记已处理
            </button>
          </div>
        </div>
        {!alertsLoaded && alertsError ? (
          <div className="empty">预警读取失败：{alertsError}</div>
        ) : alertRows.length === 0 ? (
          <EmptyHero
            Icon={IconBell}
            od="dash-alerts-empty"
            title="监控还没有启动"
            desc="录入第一笔持仓（或添加自选）后，助理按 03 §12 判定序盯涨跌停、放量、强赎与止盈止损，触发即出现在这里；微信推送随渠道接入（C4 拍板后）。"
          />
        ) : (
          <div className="list" id="alert-list">
            {alertRows.map((a, i) => (
              <div className="list-item" data-od-id={`alert-item-${i}`} key={a.id} style={a.done ? { opacity: 0.55 } : undefined}>
                <span
                  className={`dot${a.level === "up" ? " live" : ""}`}
                  style={{ background: ALERT_DOT[a.level] ?? "var(--muted)", width: 8, height: 8, borderRadius: "50%", flex: "none" }}
                />
                <span className="li-main">
                  <span className="li-title">
                    {a.name} <span className="code num">{a.code}</span>
                    <span className={`pill ${ALERT_PILL[a.level] ?? ""}`}>{a.type}</span>
                  </span>
                  <span className="li-desc">{a.text}</span>
                </span>
                <span className="li-right" style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 7 }}>
                  <span className="num" style={{ fontSize: 11, color: "var(--muted)" }}>{a.ts}</span>
                  <span style={{ display: "flex", gap: 6 }}>
                    <Link className="btn btn-sm" to={`/analysis?code=${a.code}`}>看建议</Link>
                    <button className="btn btn-sm" disabled={a.done || alertBusy} onClick={() => void markOne(a.id)}>
                      {a.done ? "已处理" : "处理"}
                    </button>
                  </span>
                </span>
              </div>
            ))}
          </div>
        )}
        <div className="card-foot" style={{ justifyContent: "space-between" }}>
          <span className="hint" style={{ margin: 0 }}>预警规则在「微信助理接入」里配置推送时段与方式。</span>
          <Link className="btn btn-sm" to="/wechat">推送设置</Link>
        </div>
      </section>

      {/* 持仓 + 自选 */}
      <div className="grid g-main" style={{ marginTop: 16 }}>
        <section className="card" data-od-id="dash-holdings-card">
          <div className="card-head">
            <div>
              <div className="card-title">我的持仓</div>
              <div className="card-sub">点击任意一行查看该标的的分析建议</div>
            </div>
            <div className="head-actions">
              <Link className="btn btn-sm" to="/holdings">全部明细</Link>
            </div>
          </div>
          <div className="card-body tight">
            <div className="tbl-wrap">
              <table className="tbl" id="dash-holdings">
                <thead>
                  <tr>
                    <th className="txt">标的</th>
                    <th className="txt">30 日走势</th>
                    <th>现价</th>
                    <th>今日</th>
                    <th>持仓盈亏</th>
                    <th>占组合</th>
                  </tr>
                </thead>
                <tbody>
                  {holdRows.length === 0 ? (
                    <tr>
                      <td colSpan={6}>
                        <div className="empty">
                          {view ? "还没有持仓 · 在「持仓管理」页可以录入第一笔" : "待加载组合快照…"}
                        </div>
                      </td>
                    </tr>
                  ) : (
                    holdRows.map(({ h, p }) => {
                      const sym = symbolOf(h.mkt, h.code);
                      const bars = sym ? klineSeries[klineKey(sym, SPARK_Q)] : undefined;
                      const sparkData = (bars ?? [])
                        .map((b) => b.close)
                        .filter((v): v is number => v !== null && v !== undefined && v > 0)
                        .slice(-SPARK_DAYS);
                      return (
                        <tr
                          key={h.code}
                          data-clickable
                          data-href={`/analysis?code=${h.code}`}
                          data-od-id={`dash-hrow-${h.code}`}
                          onClick={() => navigate(`/analysis?code=${h.code}`)}
                        >
                          <td className="txt">
                            <div className="name">{h.name}</div>
                            <div className="sub code">{h.code} · {h.mkt} · {KIND_LABEL[h.kind]}</div>
                          </td>
                          <td className="txt" style={{ width: 110 }}>
                            {sparkData.length > 0 ? <Spark data={sparkData} dir={p.day_chg} width={100} height={30} /> : <span className="sub">—</span>}
                          </td>
                          <td className="num strong">
                            <span className="tick">
                              {h.price !== null && h.price !== undefined && h.price > 0 ? fmt(h.price, priceDecimals(h.kind)) : "—"}
                            </span>
                          </td>
                          <td className={`num ${dirClass(p.day_chg)}`}>{pct(p.day_chg)}</td>
                          <td className={`num ${dirClass(p.pnl)}`}>{(p.pnl > 0 ? "+" : "") + fmt(p.pnl, 0)}</td>
                          <td className="num" style={{ color: "var(--muted)" }}>{p.weight.toFixed(1)}%</td>
                        </tr>
                      );
                    })
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </section>

        {/* 自选观察（B13：watchlist 表 + CRUD API 的前端消费入口） */}
        <section className="card" data-od-id="watchlist-card">
          <div className="card-head">
            <div>
              <div className="card-title">自选观察</div>
              <div className="card-sub">
                未买入，仅在条件触发时提醒（按无持仓形态判定）
                {watchError ? ` · ${watchError}` : ""}
              </div>
            </div>
            <div className="head-actions">
              <button className="btn btn-sm" id="watch-add" onClick={() => setAdding((v) => !v)}>
                {adding ? "收起" : "＋ 添加"}
              </button>
            </div>
          </div>
          {adding && (
            <form className="card-body" onSubmit={(e) => void submitAdd(e)} style={{ borderBottom: "1px solid var(--border)" }}>
              <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit,minmax(110px,1fr))", gap: "0 12px" }}>
                {field("代码 *（6 位）", (
                  <input
                    className="input num" inputMode="numeric" maxLength={6} placeholder="002415" autoComplete="off"
                    value={form.code} onChange={(e) => setForm((f) => ({ ...f, code: e.target.value }))}
                  />
                ))}
                {field("名称 *", (
                  <input
                    className="input" placeholder="海康威视" autoComplete="off"
                    value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
                  />
                ))}
                {field("市场", (
                  <select className="select" value={form.mkt} onChange={(e) => setForm((f) => ({ ...f, mkt: e.target.value }))}>
                    <option value="SH">SH · 沪</option>
                    <option value="SZ">SZ · 深</option>
                  </select>
                ))}
                {field("类型", (
                  <select className="select" value={form.kind} onChange={(e) => setForm((f) => ({ ...f, kind: e.target.value }))}>
                    <option value="stock">股票</option>
                    <option value="etf">ETF / 基金</option>
                    <option value="cb">可转债</option>
                  </select>
                ))}
                {field("涨跌停 %（可空）", (
                  <input
                    className="input num" inputMode="numeric" placeholder="按板块推断"
                    title="ETF 无法由代码推断（03 §2），建议显式填写；留空 = 后端按板块推断"
                    value={form.limitPct} onChange={(e) => setForm((f) => ({ ...f, limitPct: e.target.value }))}
                  />
                ))}
                {field("行业（可空）", (
                  <input
                    className="input" placeholder="电子" autoComplete="off"
                    value={form.sector} onChange={(e) => setForm((f) => ({ ...f, sector: e.target.value }))}
                  />
                ))}
              </div>
              {field("备注（可空）", (
                <input
                  className="input" placeholder="观察：AI 视觉商业化" autoComplete="off"
                  value={form.note} onChange={(e) => setForm((f) => ({ ...f, note: e.target.value }))}
                />
              ))}
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <button className="btn btn-primary btn-sm" type="submit">保存自选</button>
                <span className="hint" style={{ margin: 0 }}>重复添加同一代码 = 更新名称与备注（后端 upsert 幂等）</span>
              </div>
            </form>
          )}
          <div className="list" id="watch-list">
            {watchRows.length === 0 ? (
              <div className="empty">
                自选还是空的 —— 添加标的后，助理在持仓之外同步盯它的涨跌停 / 放量 / 强赎 / 临近到期（03 §12 自选按无持仓形态判定）。
              </div>
            ) : (
              watchRows.map((w, i) => {
                const sym = symbolOf(w.mkt, w.code);
                const q = sym ? watchQuotes[sym] : undefined;
                const usable = q !== undefined && q.status === "ok" && (q.price ?? 0) > 0;
                const overlap = w.sector !== "" && heldSectors.has(w.sector);
                return (
                  <div
                    className="list-item" data-od-id={`watch-item-${i}`} key={w.code}
                    style={{ cursor: "pointer" }} onClick={() => navigate(`/analysis?code=${w.code}`)}
                  >
                    <span className="li-main">
                      <span className="li-title">
                        {w.name} <span className="code num">{w.code}</span>
                        {overlap && <span className="pill warn">行业重叠</span>}
                      </span>
                      <span className="li-desc">
                        {[`${w.mkt} · ${KIND_LABEL[w.kind]}`, w.sector, w.note].filter(Boolean).join(" · ")}
                      </span>
                    </span>
                    <span className="li-right" style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      <span>
                        <span className="num" style={{ fontSize: 13 }}>
                          {usable ? fmt(q.price ?? 0, priceDecimals(w.kind)) : "—"}
                        </span>
                        {usable && q.chg_pct !== null && q.chg_pct !== undefined && (
                          <span className={`num ${dirClass(q.chg_pct)}`} style={{ fontSize: 11.5, display: "block" }}>
                            {pct(q.chg_pct)}
                          </span>
                        )}
                      </span>
                      <button
                        className="btn btn-sm" title={`移除 ${w.name}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          void removeWatch(w.code, w.name);
                        }}
                      >
                        ×
                      </button>
                    </span>
                  </div>
                );
              })
            )}
          </div>
          <div className="card-foot">
            <span className="hint" style={{ margin: 0 }}>提示：与现有行业重叠的标的会标注，避免「换了个名字加仓同一个风险」。</span>
          </div>
        </section>
      </div>

      {/* 收盘摘要（15:05 归档快照原值 + push_log 留痕，不现编文案） */}
      <section className="card" data-od-id="daily-brief" style={{ marginTop: 16 }}>
        <div className="card-head">
          <div>
            <div className="card-title">助理收盘摘要</div>
            <div className="card-sub">15:05 自动生成，微信推送随渠道接入（C4 拍板后）</div>
          </div>
          <div className="head-actions">
            <span className="pill">{lastSnap ? `${lastSnap.ts} 归档` : "待首个快照"}</span>
          </div>
        </div>
        <div className="card-body">
          {isEmpty ? (
            <div className="grid g-3" id="brief">
              {EMPTY_STEPS.map(([t, b], i) => (
                <div data-od-id={`brief-empty-${i}`} key={t}>
                  <div className="eyebrow">{String(i + 1).padStart(2, "0")} · {t}</div>
                  <p style={{ marginTop: 9, fontSize: 13, lineHeight: 1.7, color: "var(--fg-2)", maxWidth: "60ch" }}>{b}</p>
                </div>
              ))}
            </div>
          ) : brief.length > 0 ? (
            <div className="grid g-3" id="brief">
              {brief.map(([t, b], i) => (
                <div data-od-id={`brief-item-${i}`} key={t}>
                  <div className="eyebrow">{String(i + 1).padStart(2, "0")} · {t}</div>
                  <p style={{ marginTop: 9, fontSize: 13, lineHeight: 1.7, color: "var(--fg-2)", maxWidth: "60ch" }}>{b}</p>
                </div>
              ))}
            </div>
          ) : (
            <p className="hint" style={{ margin: 0 }}>
              收盘摘要读的是 15:05 归档的净值快照（recalc 原值）——录入持仓后的首个交易日收盘起，这里开始出现当日归档数字与复盘留痕。
            </p>
          )}
        </div>
      </section>

      <div className="footnote" data-od-id="dash-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          行情来自公开数据源（可能延迟或错漏）；净值与摘要数字为后端 15:05 归档的 recalc 快照原值；
          沪深 300 基准由价格指数日线点位归一到区间净值起点后对比（未含分红，非严格收益对齐，B11 口径待定稿）。
          任何结论均不构成投资建议，程序不代下单。
        </span>
      </div>
    </main>
  );
}
