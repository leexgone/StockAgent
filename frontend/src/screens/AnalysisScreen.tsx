import { useEffect, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import Candles, { type Ohlc } from "../components/charts/Candles";
import Gauge from "../components/charts/Gauge";
import EmptyHero from "../components/EmptyHero";
import { IconSpark } from "../components/Icons";
import { lotSize, priceDecimals, type Kind } from "../domain";
import { dirClass, fixed, fmt, money, pct } from "../lib/format";
import { KIND_LABEL, symbolOf } from "../lib/kind";
import { latestByCode, useAdvice, type AdviceRow } from "../store/advice";
import { klineKey, useKline } from "../store/kline";
import { usePortfolio, type PortfolioView } from "../store/portfolio";
import { useUi } from "../store/ui";
import { useWatchlist } from "../store/watchlist";

/**
 * AI 分析建议（analysis 屏，P5-T2 真实数据移植）—— 对照基准 design/analysis.html。
 * 铁律：绝不伪造建议（原型的 adviceFor 观察态兜底、RNG 蜡烛、样本历史全部删除）。
 * 结论/四维/得分/目标止损/风险 = `GET /api/advice?code=` 的 AdviceRow 留档原值
 * （ADR-0007：数字归规则、文字归模型；reason 空串如实标注「文案未回填」）；
 * K 线 = 真实 `/api/market/kline`；历史 = 同一 code 的多条留档（generated_at DESC，
 * 带 data_version 可回溯 B9，无「当时价格」列——留档不存该值，不臆造）。
 * 无留档 → 诚实空态 + 生成路径（盘后复盘 / 问答 /建议），不画假得分。
 * 执行计划 = 客户端模板骨架（分笔 + 失效条件引真实 stop/target），数字取后端原值 +
 * domain lotSize；「写入持仓试算」深链 /holdings 抽屉，「就这只问我」深链 /chat。
 */

type HRow = PortfolioView["holdings"][number];
type PRow = PortfolioView["per"][number];

interface Item {
  code: string;
  name: string;
  kind: Kind;
  sector: string;
  mkt: string;
  held: boolean;
  shares: number;
  cost: number;
  weight: number;
  mv: number;
  /** 浮动盈亏（per.pnl 原值；未持有为 0） */
  pnl: number;
  price: number | null;
  prev: number | null;
  dayChg: number | null;
}

/** 结论标签 → verdict-tag 配色类（交易方向轴 buy/sell + accent，03 §11） */
const TAG: Record<string, string> = {
  买入: "vt-buy", 加仓: "vt-add", 持有: "vt-hold", 减仓: "vt-reduce", 卖出: "vt-sell", 定投: "vt-dca",
};
const UNIT: Record<Kind, string> = { stock: "股", etf: "份", cb: "张" };
/** K 线取的最近日线根数（对照原型「30 日 K 线」） */
const KLINE_BARS = 30;

interface Dim { k: string; s: number; t: string }
function parseDims(raw: unknown): Dim[] {
  if (!Array.isArray(raw)) return [];
  return raw.filter((d): d is Dim => !!d && typeof (d as Dim).s === "number" && typeof (d as Dim).k === "string");
}

export default function AnalysisScreen() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const view = usePortfolio((s) => s.view);
  const watchRows = useWatchlist((s) => s.rows);
  const watchQuotes = useWatchlist((s) => s.quotes);
  const adviceRows = useAdvice((s) => s.rows);
  const byCode = useAdvice((s) => s.byCode);
  const deepCode = params.get("code");

  const [scope, setScope] = useState<"hold" | "watch">("hold");
  const [code, setCode] = useState<string>("");
  const [period, setPeriod] = useState(1);
  const [bootstrapped, setBootstrapped] = useState(false);

  const toast = (m: string) => useUi.getState().pushToast(m);
  const agg = view?.agg;

  /* ---------- 进屏拉自选 + 全量建议（rank 用） ---------- */
  useEffect(() => {
    void useWatchlist.getState().load().then(() => useWatchlist.getState().loadQuotes());
    void useAdvice.getState().load();
  }, []);

  /* ---------- 候选池（hold = 持仓；watch = 自选） ---------- */
  const holdItems = useMemo<Item[]>(() => {
    if (!view) return [];
    return view.holdings.map((h: HRow, i: number) => {
      const p: PRow | undefined = view.per[i];
      return {
        code: h.code, name: h.name, kind: h.kind, sector: h.sector, mkt: h.mkt,
        held: true, shares: h.shares, cost: h.cost, weight: p?.weight ?? 0, mv: p?.mv ?? 0, pnl: p?.pnl ?? 0,
        price: h.price ?? null, prev: h.prev ?? null, dayChg: p?.day_chg ?? null,
      };
    });
  }, [view]);

  const watchItems = useMemo<Item[]>(() => {
    const heldCodes = new Set(holdItems.map((x) => x.code));
    return watchRows
      .filter((w) => !heldCodes.has(w.code))
      .map((w) => {
        const sym = symbolOf(w.mkt, w.code);
        const q = sym ? watchQuotes[sym] : undefined;
        const usable = q && q.status === "ok" && (q.price ?? 0) > 0;
        return {
          code: w.code, name: w.name, kind: w.kind, sector: w.sector, mkt: w.mkt,
          held: false, shares: 0, cost: 0, weight: 0, mv: 0, pnl: 0,
          price: usable ? (q.price ?? null) : null, prev: usable ? (q.prev ?? null) : null,
          dayChg: usable ? (q.chg_pct ?? null) : null,
        };
      });
  }, [watchRows, watchQuotes, holdItems]);

  const items = scope === "hold" ? holdItems : watchItems;

  /* ---------- 初始 code / scope：深链优先，否则范围内第一个 ---------- */
  useEffect(() => {
    if (bootstrapped) return;
    if (deepCode) {
      const inHold = holdItems.some((x) => x.code === deepCode);
      const inWatch = watchItems.some((x) => x.code === deepCode);
      if (inHold || inWatch) {
        setScope(inHold ? "hold" : "watch");
        setCode(deepCode);
        setBootstrapped(true);
        return;
      }
    }
    if (holdItems.length > 0) {
      setScope("hold"); setCode(holdItems[0].code); setBootstrapped(true);
    } else if (watchItems.length > 0) {
      setScope("watch"); setCode(watchItems[0].code); setBootstrapped(true);
    }
  }, [bootstrapped, deepCode, holdItems, watchItems]);

  // 切换范围时把 code 落到新范围首个
  const switchScope = (s: "hold" | "watch") => {
    setScope(s);
    const list = s === "hold" ? holdItems : watchItems;
    setCode(list.length ? list[0].code : "");
  };

  const item = items.find((x) => x.code === code) ?? null;

  /* ---------- 选中 code 的建议历史 + kline ---------- */
  useEffect(() => {
    if (code) void useAdvice.getState().loadForCode(code, 50);
  }, [code]);

  const sym = item ? symbolOf(item.mkt, item.code) : null;
  const sparkQ = { withToday: true } as const;
  useEffect(() => {
    if (sym) void useKline.getState().load(sym, sparkQ);
  }, [sym]);
  const bars = useKline((s) => (sym ? s.series[klineKey(sym, sparkQ)] : undefined));

  const adviceHistory: AdviceRow[] = code ? (byCode[code] ?? []) : [];
  const A = adviceHistory[0] ?? null;

  /* ---------- K 线蜡烛（真实日线；period 5 = 5 交易日聚合近似周 K） ---------- */
  const candles = useMemo<Ohlc[]>(() => {
    if (!bars) return [];
    const valid = bars
      .filter((b) => b.open != null && b.high != null && b.low != null && b.close != null && (b.close ?? 0) > 0)
      .slice(-KLINE_BARS)
      .map((b) => ({ o: b.open as number, h: b.high as number, l: b.low as number, c: b.close as number, date: b.date }));
    if (period !== 5) return valid;
    const out: Ohlc[] = [];
    for (let i = 0; i < valid.length; i += 5) {
      const g = valid.slice(i, i + 5);
      if (!g.length) continue;
      out.push({
        o: g[0].o, c: g[g.length - 1].c,
        h: Math.max(...g.map((x) => x.h)), l: Math.min(...g.map((x) => x.l)),
        date: g[g.length - 1].date,
      });
    }
    return out;
  }, [bars, period]);

  /* ---------- rank（范围内每标的最新留档，按得分降序） ---------- */
  const latest = useMemo(() => latestByCode(adviceRows), [adviceRows]);
  const ranked = useMemo(
    () =>
      items
        .map((x) => ({ ...x, a: latest[x.code] ?? null }))
        .sort((p, q) => (q.a?.score ?? -1) - (p.a?.score ?? -1)),
    [items, latest],
  );

  /* ---------- 派生展示值 ---------- */
  const dec = item ? priceDecimals(item.kind) : 2;
  const price = item?.price ?? null;
  const dims = A ? parseDims(A.dims) : [];
  const confPct = A ? Math.round(A.conf * 100) : 0;
  const targetText = A && A.target_lo != null && A.target_hi != null ? `${fmt(A.target_lo, dec)} — ${fmt(A.target_hi, dec)}` : "未设";
  const stopText = A && A.stop != null ? fmt(A.stop, dec) : "未设";
  const suggestedWeight = A ? (A.position.split("→")[1]?.trim() ?? "—") : "—";

  /* ---------- 执行计划（客户端骨架，数字取后端原值 + domain lotSize） ---------- */
  const plan = useMemo<{ t: string; d: string }[]>(() => {
    if (!item || !A || price == null) return [];
    const lot = lotSize(item.kind);
    const unit = UNIT[item.kind];
    if (!item.held) {
      return [
        { t: "先不动，设一个触发条件", d: `突破 ${fmt(price * 1.03, dec)} 且放量确认时提醒你；在此之前不建仓。` },
        { t: "若要试仓，上限 3%", d: `${money(price * lot)} 起（1 手），试错仓位不影响主组合判断。` },
      ];
    }
    if (A.action === "减仓" || A.action === "卖出") {
      const q1 = Math.max(lot, Math.floor((item.shares * 0.4) / lot) * lot);
      const q2 = Math.max(lot, Math.floor((item.shares * 0.2) / lot) * lot);
      return [
        { t: `今日 ${fmt(price, dec)} 附近减 ${fmt(q1, 0)} ${unit}`, d: `先处理最急的部分：把单票占比从 ${fixed(item.weight, 1)}% 压下来。` },
        { t: `若明日反弹不过 60 日线，再减 ${fmt(q2, 0)} ${unit}`, d: "用「反弹不过均线」作条件，比直接追跌出更好的价格；挂条件单后微信会推给你。" },
        { t: "失效条件 / 资金去向", d: A.stop != null ? `跌破 ${fmt(A.stop, dec)} 说明判断错了，无条件回到原仓位。` : (A.action === "减仓" ? "先回现金，等回踩机会再补，别当场换股。" : "清仓资金进现金池，本周内不重复建仓同一行业。") },
      ];
    }
    if (A.action === "加仓" || A.action === "买入") {
      const cash = agg?.cash ?? 0;
      const budget = Math.min(cash * 0.4, item.held ? item.mv * 0.35 : cash * 0.3);
      const qty = Math.max(lot, Math.floor(budget / price / lot) * lot);
      return [
        { t: `首笔 ${fmt(qty, 0)} ${unit} @ ${fmt(price, dec)} 附近`, d: `约 ${money(qty * price)}，留足第二笔弹药（占可用现金 ${cash > 0 ? ((qty * price) / cash * 100).toFixed(0) : "—"}%）。` },
        { t: "回踩不破 5 日线补第二笔", d: "两笔之间至少隔 2 个交易日，避免在同一个价格区间重复下注。" },
        { t: "写下失效条件", d: A.stop != null ? `跌破 ${fmt(A.stop, dec)} 说明判断错了，无条件回到原仓位。` : "本次不设施损，以时间换空间。" },
      ];
    }
    return [
      { t: "维持现有仓位，不动", d: `当前 ${fixed(item.weight, 1)}% 在纪律范围内，没有需要处理的风险点。` },
      { t: "下一次重算时间", d: "每日 15:05 盘后复盘 + 财报/公告事件触发时即时重算，结果会推到微信。" },
    ];
  }, [item, A, price, dec, agg]);

  /* ---------- 动作 ---------- */
  const applyPlan = () => {
    if (!item || !A) return;
    if (!item.held) {
      toast("自选标的需先建仓，已在持仓页准备好新增表单");
      navigate(`/holdings?code=${item.code}&side=buy`);
      return;
    }
    const sell = A.action === "减仓" || A.action === "卖出";
    const lot = lotSize(item.kind);
    const qty = Math.max(lot, Math.floor((item.shares * (sell ? 0.4 : 0.25)) / lot) * lot);
    toast(`已把「${A.action} ${item.name}」第一笔 ${qty} 送到持仓页试算`);
    navigate(`/holdings?code=${item.code}&side=${sell ? "sell" : "buy"}&qty=${qty}`);
  };
  const askMore = () => {
    if (!item || !A) return;
    const q = `${item.name}建议「${A.action}」的理由再具体一点，我该在什么价格动手？`;
    navigate(`/chat?q=${encodeURIComponent(q)}`);
  };
  const rerun = () => {
    void useAdvice.getState().load();
    if (code) void useAdvice.getState().loadForCode(code, 50);
    if (sym) void useKline.getState().reload(sym, sparkQ);
    void useWatchlist.getState().loadQuotes();
    toast("已重读最新留档 · 建议由盘后复盘 / 问答「/建议」生成（数字归规则、文字归模型）");
  };

  const emptyScope = items.length === 0;
  const nameMeta = item
    ? `${KIND_LABEL[item.kind]} · ${item.sector || "未分类"}${item.held ? ` · 持有 ${fmt(item.shares, 0)} ${UNIT[item.kind]} · 占组合 ${fixed(item.weight, 1)}%` : " · 未持有（自选观察）"}`
    : "—";

  const th = (label: string, txt = false) => <th className={txt ? "txt" : undefined}>{label}</th>;

  return (
    <main className="page" data-od-id="analysis-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">AI 分析建议</div>
          <h1 className="page-title">结论、依据、以及怎么执行</h1>
          <p className="page-desc">
            每只标的给出一个动作（买入 / 加仓 / 持有 / 减仓 / 卖出 / 定投），并把结论拆成四个维度：你能看到它是根据什么得出的，也能看到它可能错在哪。数字归规则、文字归模型（ADR-0007）。
          </p>
        </div>
        <div className="page-actions">
          <div className="seg" role="group" aria-label="分析范围" id="scope">
            <button type="button" data-s="hold" aria-pressed={scope === "hold"} onClick={() => switchScope("hold")}>我的持仓</button>
            <button type="button" data-s="watch" aria-pressed={scope === "watch"} onClick={() => switchScope("watch")}>自选</button>
          </div>
          <button className="btn" id="rerun" onClick={rerun}>重新分析</button>
        </div>
      </div>

      {emptyScope ? (
        <section className="card" data-od-id="analysis-empty-card">
          <div className="card-head">
            <div>
              <div className="card-title">分析建议以你的持仓为上下文</div>
              <div className="card-sub">当前范围内没有标的</div>
            </div>
          </div>
          <div className="card-body">
            <EmptyHero
              Icon={IconSpark}
              od="analysis-empty"
              title={scope === "hold" ? "还没有持仓可分析" : "自选里还没有标的"}
              desc="录入第一笔后，助理按你的成本与仓位现算得分、目标价与止损，给出「结论 / 为什么 / 怎么做」三段式建议；也可以切到另一范围，或在总览仪表盘添加自选。"
              actions={scope === "hold" ? undefined : [{ label: "去总览加自选", to: "/dashboard" }]}
            />
          </div>
        </section>
      ) : (
        <div className="a-layout">
          {/* 左：标的选择 + 建议排序 */}
          <div>
            <section className="card" data-od-id="analysis-picker">
              <div className="card-head"><div className="card-title" style={{ fontSize: 13 }}>选择标的</div></div>
              <div className="card-body tight" style={{ padding: 8 }}>
                <div className="pick" id="pick">
                  {items.map((x) => (
                    <button type="button" key={x.code} data-code={x.code} aria-pressed={x.code === code} onClick={() => setCode(x.code)}>
                      <span>{x.name}</span><span className="c">{x.code}</span>
                    </button>
                  ))}
                </div>
              </div>
            </section>
            <section className="card" data-od-id="analysis-rank" style={{ marginTop: 16 }}>
              <div className="card-head">
                <div><div className="card-title" style={{ fontSize: 13 }}>建议排序</div><div className="card-sub">按综合得分</div></div>
              </div>
              <div className="card-body tight" style={{ padding: "0 0 6px" }}>
                <div className="tbl-wrap">
                  <table className="tbl" id="rank">
                    <thead><tr>{th("标的", true)}{th("得分")}{th("动作")}</tr></thead>
                    <tbody>
                      {ranked.map((r) => (
                        <tr
                          key={r.code} data-clickable data-od-id={`rank-row-${r.code}`}
                          style={r.code === code ? { background: "var(--accent-soft)", cursor: "pointer" } : { cursor: "pointer" }}
                          onClick={() => setCode(r.code)}
                        >
                          <td className="txt"><div className="name">{r.name}</div><div className="sub code">{r.code}</div></td>
                          <td className="num strong">{r.a ? Math.round(r.a.score) : "—"}</td>
                          <td style={{ textAlign: "right" }}>
                            {r.a
                              ? <span className={`pill ${r.a.score >= 70 ? "ok" : r.a.score >= 55 ? "" : "danger"}`}>{r.a.action}</span>
                              : <span className="pill">未留档</span>}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            </section>
          </div>

          {/* 右：分析结果 */}
          <div className="grid" id="a-result" style={{ gap: 16 }}>
            {/* 结论卡 */}
            <section className="card" data-od-id="verdict-card">
              <div className="card-head">
                <div>
                  <div className="card-title" id="a-name" style={{ fontSize: 17 }}>{item?.name ?? "—"}</div>
                  <div className="card-sub">
                    <span className="num" id="a-code">{item ? `${item.code} · ${item.mkt}` : "—"}</span>
                    <span id="a-meta"> · {nameMeta}</span>
                  </div>
                </div>
                <div className="head-actions">
                  {A && <span className={`pill ${A.conf >= 0.68 ? "accent" : ""}`} id="a-conf">模型置信度 {confPct}%</span>}
                  <button className="btn btn-sm" id="ask-more" disabled={!A} onClick={askMore}>就这只问我</button>
                </div>
              </div>
              <div className="card-body">
                {A ? (
                  <>
                    <div className="verdict">
                      <div className={`verdict-tag ${TAG[A.action] ?? "vt-hold"}`} id="a-tag">{A.action}</div>
                      <div>
                        <div className="metric-label">建议动作</div>
                        <div style={{ fontSize: 13.5, fontWeight: 550, marginTop: 6 }} id="a-action-line">
                          现价 <b className="num">{price != null ? fmt(price, dec) : "—"}</b>{" "}
                          {item?.dayChg != null && <span className={`${dirClass(item.dayChg)} num`}>{pct(item.dayChg)}</span>}
                          {" · "}{A.position}
                        </div>
                        <div className="hint" id="a-reason" style={{ maxWidth: "52ch", marginTop: 6 }}>
                          {A.reason || "数字已留档、模型文案未回填（ADR-0007：数字归规则、文字归模型）。"}
                        </div>
                      </div>
                      <div style={{ marginLeft: "auto", textAlign: "center" }} id="a-gauge">
                        <Gauge score={A.score} />
                      </div>
                    </div>
                    <div
                      style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit,minmax(120px,1fr))", gap: 1, background: "var(--border)", border: "1px solid var(--border)", borderRadius: "var(--r-md)", overflow: "hidden", marginTop: 20 }}
                      id="a-facts"
                    >
                      {[
                        ["现价 / 昨收", `${price != null ? fmt(price, dec) : "—"} / ${item?.prev != null ? fmt(item.prev, dec) : "—"}`],
                        ["成本 / 盈亏", item?.held ? `${fmt(item.cost, dec)} / ${money(item.pnl)}` : "— / —"],
                        ["目标区间", targetText],
                        ["止损 / 支撑", stopText],
                        ["建议仓位", suggestedWeight],
                        ["信号一致度", `${confPct} / 100`],
                      ].map((f, i) => (
                        <div style={{ background: "var(--surface)", padding: "12px 14px" }} data-od-id={`fact-${i}`} key={f[0] as string}>
                          <div style={{ fontSize: 11.5, color: "var(--muted)" }}>{f[0]}</div>
                          <div className="num" style={{ fontSize: 14, fontWeight: 550, marginTop: 5 }}>{f[1]}</div>
                        </div>
                      ))}
                    </div>
                  </>
                ) : (
                  <EmptyHero
                    Icon={IconSpark}
                    od="verdict-none"
                    title="该标的暂无建议留档"
                    desc="建议由交易日 15:05 盘后复盘自动生成，或在「智能问答」用 /建议 触发；数字归规则、文字归模型（ADR-0007），生成后这里显示结论、得分、四维与目标止损。K 线为真实行情，可先查看。"
                    actions={[{ label: "去问答触发 /建议", to: "/chat" }]}
                  />
                )}
              </div>
            </section>

            {/* K 线（真实行情，恒显） */}
            <section className="card" data-od-id="kline-card">
              <div className="card-head">
                <div>
                  <div className="card-title">{period === 1 ? "30 日" : "周"}K 线与均线</div>
                  <div className="card-sub" id="k-sub">
                    {candles.length
                      ? `${period === 1 ? "每日" : "每 5 交易日"} OHLC 为真实日线，末根锚定现价 ${price != null ? fmt(price, dec) : "—"}`
                      : "日线加载中或该标的暂无日线数据"}
                  </div>
                </div>
                <div className="head-actions">
                  <div className="seg" id="k-period">
                    <button type="button" data-p="1" aria-pressed={period === 1} onClick={() => setPeriod(1)}>日 K</button>
                    <button type="button" data-p="5" aria-pressed={period === 5} onClick={() => setPeriod(5)}>周 K</button>
                  </div>
                </div>
              </div>
              <div className="card-body">
                {item && <Candles data={candles} kind={item.kind} height={250} />}
                <div className="legend" style={{ marginTop: 8 }}>
                  <span><i style={{ background: "var(--up)" }}></i>阳线（涨）</span>
                  <span><i style={{ background: "var(--down)" }}></i>阴线（跌）</span>
                  <span><i style={{ background: "var(--accent)" }}></i>MA5</span>
                  <span><i style={{ background: "var(--muted)" }}></i>MA20</span>
                  <span style={{ marginLeft: "auto" }} className="num" id="k-read">
                    {candles.length ? `${period === 1 ? "日" : "周"}线 ${candles.length} 根` : "—"}
                  </span>
                </div>
              </div>
            </section>

            {/* 四维 + 执行计划 + 风险 */}
            <div className="grid g-2">
              <section className="card" data-od-id="dims-card">
                <div className="card-head">
                  <div><div className="card-title">依据拆解</div><div className="card-sub">四个维度各自的分数与观察</div></div>
                </div>
                <div className="card-body" id="dims">
                  {dims.length ? (
                    dims.map((d, i) => (
                      <div className="dim" data-od-id={`dim-${i}`} key={d.k}>
                        <div>
                          <div className="dim-k">{d.k}</div>
                          <div className="dim-s">{Math.round(d.s)} / 100</div>
                          <div className="progress" style={{ marginTop: 8 }}>
                            <i style={{ width: `${Math.max(0, Math.min(100, d.s))}%`, background: d.s >= 65 ? "var(--ok)" : d.s >= 45 ? "var(--warn)" : "var(--danger)" }} />
                          </div>
                        </div>
                        <div className="dim-t">{d.t}</div>
                      </div>
                    ))
                  ) : (
                    <div className="empty">{A ? "该留档无四维拆解（dims 为空）" : "建议留档后显示技术 / 基本面 / 资金 / 消息四维分数与观察"}</div>
                  )}
                </div>
              </section>
              <div className="grid" style={{ gap: 16 }}>
                <section className="card" data-od-id="plan-card">
                  <div className="card-head">
                    <div><div className="card-title">执行计划</div><div className="card-sub">分笔，不是一次性梭哈</div></div>
                    <div className="head-actions">
                      <button className="btn btn-sm btn-primary" id="apply-plan" disabled={!A} onClick={applyPlan}>写入持仓试算</button>
                    </div>
                  </div>
                  <div className="card-body" id="plan">
                    {plan.length ? (
                      plan.map((p, i) => (
                        <div className="plan-step" data-od-id={`plan-step-${i}`} key={i}>
                          <div className="plan-no">{i + 1}</div>
                          <div>
                            <div style={{ fontSize: 13, fontWeight: 550 }}>{p.t}</div>
                            <div className="hint" style={{ marginTop: 4 }}>{p.d}</div>
                          </div>
                        </div>
                      ))
                    ) : (
                      <div className="empty">{A ? "—" : "建议留档后生成分笔执行骨架（数字取后端原值）"}</div>
                    )}
                  </div>
                </section>
                <section className="card" data-od-id="risk-card">
                  <div className="card-head">
                    <div><div className="card-title">这个结论可能错在哪</div><div className="card-sub">助理自己列的风险</div></div>
                  </div>
                  <div className="card-body">
                    {A && A.risks.length ? (
                      <ul id="risks" style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, lineHeight: 1.75, color: "var(--fg-2)" }}>
                        {A.risks.map((r, i) => <li data-od-id={`risk-${i}`} key={i} style={{ marginBottom: 8 }}>{r}</li>)}
                      </ul>
                    ) : (
                      <div className="empty">{A ? "该留档未附风险自陈" : "建议留档后显示模型的风险自陈"}</div>
                    )}
                  </div>
                </section>
              </div>
            </div>

            {/* 历史建议（真实 advice_history，同 code 多条） */}
            <section className="card" data-od-id="history-card">
              <div className="card-head">
                <div>
                  <div className="card-title">该标的的历史建议</div>
                  <div className="card-sub">真实留档（advice_history）· 数字列一经写入不经模型转写（ADR-0007 / B9）</div>
                </div>
                <div className="head-actions">
                  <span className="pill">{adviceHistory.length} 条</span>
                </div>
              </div>
              <div className="card-body tight">
                <div className="tbl-wrap">
                  <table className="tbl" id="hist">
                    <thead>
                      <tr>{th("生成时间", true)}{th("动作", true)}{th("得分")}{th("目标 / 止损")}{th("数据版本", true)}{th("理由摘要", true)}</tr>
                    </thead>
                    <tbody>
                      {adviceHistory.length ? (
                        adviceHistory.map((r, i) => (
                          <tr data-od-id={`hist-row-${i}`} key={r.id}>
                            <td className="txt num" style={{ color: "var(--muted)", fontSize: 11.5 }}>{r.generated_at}</td>
                            <td className="txt">
                              <span className={`pill ${r.action === "减仓" || r.action === "卖出" ? "sell" : r.action === "持有" || r.action === "定投" ? "" : "buy"}`}>{r.action}</span>
                            </td>
                            <td className="num">{Math.round(r.score)}</td>
                            <td className="num" style={{ fontSize: 11.5 }}>
                              {r.target_lo != null && r.target_hi != null ? `${fmt(r.target_lo, dec)}–${fmt(r.target_hi, dec)}` : "—"}
                              {r.stop != null ? <div className="sub">止损 {fmt(r.stop, dec)}</div> : null}
                            </td>
                            <td className="txt" style={{ fontFamily: "var(--font-mono)", fontSize: 10.5, color: "var(--muted)", whiteSpace: "normal", maxWidth: "22ch" }}>{r.data_version}</td>
                            <td className="txt" style={{ whiteSpace: "normal", maxWidth: "34ch", fontSize: 12, color: "var(--muted)" }}>{r.reason || "（文案未回填）"}</td>
                          </tr>
                        ))
                      ) : (
                        <tr><td colSpan={6}><div className="empty">该标的暂无建议留档 · 盘后复盘或 /建议 触发后自动记录</div></td></tr>
                      )}
                    </tbody>
                  </table>
                </div>
              </div>
            </section>
          </div>
        </div>
      )}

      <div className="footnote" data-od-id="a-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          得分与动作由规则函数产出、模型只写文案（ADR-0007），置信度反映<b>样本内信号一致性</b>、不代表未来胜率；行情来自公开数据源（可能延迟或错漏）。
          本页面<b>不构成投资建议</b>，程序不连接券商、不代为执行任何真实委托，每笔委托仍需你本人确认。
        </span>
      </div>
    </main>
  );
}
