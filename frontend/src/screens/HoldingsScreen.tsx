import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import EmptyHero from "../components/EmptyHero";
import HoldingFormDrawer from "../components/HoldingFormDrawer";
import { IconSearch, IconShield } from "../components/Icons";
import TradeDrawer, { receiptToast, type Candidate } from "../components/TradeDrawer";
import {
  LIMIT_CASH_RESERVE,
  LIMIT_CB_TOTAL,
  LIMIT_SECTOR,
  LIMIT_SINGLE_STOCK,
  priceDecimals,
  THRESHOLD_EPS,
  type Side,
} from "../domain";
import { dirClass, fixed, fmt, money, pct, wan } from "../lib/format";
import { KIND_LABEL, symbolOf } from "../lib/kind";
import { useHoldingEdits, type HoldingEditRow, type HoldingSnapshot } from "../store/holdingEdits";
import { usePortfolio, type PortfolioView } from "../store/portfolio";
import { useTrades } from "../store/trades";
import { useUi } from "../store/ui";
import { useWatchlist } from "../store/watchlist";

/**
 * 持仓管理（holdings 屏，P5-T2 真实数据移植）—— 对照基准 design/holdings.html。
 * 汇总/派生数字全部取后端 recalc 原值（agg / per，ADR-0008 后端唯一真值）；
 * 仓位纪律四条从后端 agg/per 原值现算展示值 + domain 阈值常量判定（无纪律端点，
 * check() 需非空 price，改由服务端 agg/per 供给口径一致的分子分母）。
 * 加减仓抽屉 = P4-T1 前端半边：实时试算走 src/domain 纯函数，提交 POST /api/holdings/trade
 * 以后端收据覆盖。深链 ?add=1 / ?code=&side=&qty= 直接开抽屉（分析页闭环出口）。
 */

type HRow = PortfolioView["holdings"][number];
type PRow = PortfolioView["per"][number];

const UNIT: Record<string, string> = { stock: "股", etf: "份", cb: "张" };
type SortKey = "shares" | "cost" | "price" | "dayChg" | "mv" | "pnl" | "weight";
const SORT_LABEL: Record<SortKey, string> = {
  shares: "持仓数量", cost: "成本价", price: "现价", dayChg: "今日涨跌", mv: "市值", pnl: "浮动盈亏", weight: "占组合",
};

const moneyShort = (v: number): string => (Math.abs(v) >= 10000 ? wan(v) : fmt(v, 0));

/* ---------- 维护记录渲染助手（ADR-0009「维护记录」卡） ---------- */
const OP_LABEL: Record<string, string> = { create: "新增", update: "调整", delete: "删除" };
const OP_PILL: Record<string, string> = { create: "ok", update: "", delete: "danger" };

const snapSummary = (s: HoldingSnapshot): string =>
  `份额 ${fmt(s.shares, 0)} · 成本 ${fmt(s.cost, priceDecimals(s.kind))}`;

/** 留痕行变化摘要：create/delete 显示快照值，update 逐差异字段列 before→after */
const editSummary = (e: HoldingEditRow): string => {
  if (e.op === "create") return e.after ? `${snapSummary(e.after)}（新建持仓）` : "—";
  if (e.op === "delete") return e.before ? `${snapSummary(e.before)}（移除行，流水保留）` : "—";
  const b = e.before, a = e.after;
  if (!b || !a) return "—";
  const parts: string[] = [];
  if (b.shares !== a.shares) parts.push(`份额 ${fmt(b.shares, 0)}→${fmt(a.shares, 0)}`);
  if (b.cost !== a.cost) parts.push(`成本 ${fmt(b.cost, priceDecimals(b.kind))}→${fmt(a.cost, priceDecimals(a.kind))}`);
  if (b.name !== a.name) parts.push(`名称 ${b.name}→${a.name}`);
  if (b.sector !== a.sector) parts.push(`行业 ${b.sector || "—"}→${a.sector || "—"}`);
  if (b.kind !== a.kind) parts.push(`类型 ${KIND_LABEL[b.kind]}→${KIND_LABEL[a.kind]}`);
  if ((b.limit_pct ?? null) !== (a.limit_pct ?? null)) parts.push(`涨跌停 % ${b.limit_pct ?? "—"}→${a.limit_pct ?? "—"}`);
  return parts.length ? parts.join(" · ") : "元信息调整";
};

export default function HoldingsScreen() {
  const [params, setParams] = useSearchParams();
  const view = usePortfolio((s) => s.view);
  const pLoading = usePortfolio((s) => s.loading);
  const pError = usePortfolio((s) => s.error);
  const trades = useTrades((s) => s.rows);
  const tradesError = useTrades((s) => s.error);
  const watchRows = useWatchlist((s) => s.rows);
  const watchQuotes = useWatchlist((s) => s.quotes);
  const edits = useHoldingEdits((s) => s.rows);

  const [sortKey, setSortKey] = useState<SortKey>("mv");
  const [sortDir, setSortDir] = useState(-1);
  const [filterKind, setFilterKind] = useState("all");
  const [query, setQuery] = useState("");
  const [showWeight, setShowWeight] = useState(true);
  const [editingCash, setEditingCash] = useState(false);
  const [cashDraft, setCashDraft] = useState("");
  const [drawer, setDrawer] = useState<{ open: boolean; code: string | null; side: Side; qty: number | null }>({
    open: false, code: null, side: "buy", qty: null,
  });
  // 直接维护（ADR-0009）：表单抽屉（null = 新增）+ 行内两步删除确认
  const [form, setForm] = useState<{ open: boolean; initial: HRow | null }>({ open: false, initial: null });
  const [deletingCode, setDeletingCode] = useState<string | null>(null);

  const toast = (m: string) => useUi.getState().pushToast(m);
  const agg = view?.agg;
  const holdings = view?.holdings ?? [];
  const per = view?.per ?? [];
  const isEmpty = view !== null && holdings.length === 0;

  /* ---------- 进屏拉流水 + 自选（候选池 & 新建持仓来源）+ 维护留痕 ---------- */
  useEffect(() => {
    void useTrades.getState().load();
    void useHoldingEdits.getState().load();
    const st = useWatchlist.getState();
    void st.load().then(() => useWatchlist.getState().loadQuotes());
  }, []);

  /* ---------- 直接维护动作（ADR-0009：保存/删除后重拉留痕，全景由 store 覆盖） ---------- */
  const doDelete = async (code: string, name: string) => {
    setDeletingCode(null);
    const ok = await usePortfolio.getState().removeHolding(code);
    toast(
      ok
        ? `已删除 ${name} ${code} · 流水保留、现金不动（快照校正）`
        : (usePortfolio.getState().error ?? "删除失败"),
    );
    if (ok) void useHoldingEdits.getState().load();
  };

  /* ---------- 候选池：持仓（held）+ 自选（未持有，买入即新建持仓） ---------- */
  const candidates = useMemo<Candidate[]>(() => {
    if (!view) return [];
    const held: Candidate[] = view.holdings.map((h, i) => {
      const p = view.per[i];
      return {
        code: h.code, name: h.name, kind: h.kind, mkt: h.mkt, sector: h.sector,
        shares: h.shares, cost: h.cost, mv: p?.mv ?? 0, weight: p?.weight ?? 0,
        limit_pct: h.limit_pct ?? null, price: h.price ?? null, prev: h.prev ?? null, held: true,
      };
    });
    const heldCodes = new Set(view.holdings.map((h) => h.code));
    const watch: Candidate[] = watchRows
      .filter((w) => !heldCodes.has(w.code))
      .map((w) => {
        const sym = symbolOf(w.mkt, w.code);
        const q = sym ? watchQuotes[sym] : undefined;
        const usable = q && q.status === "ok" && (q.price ?? 0) > 0;
        return {
          code: w.code, name: w.name, kind: w.kind, mkt: w.mkt, sector: w.sector,
          shares: 0, cost: 0, mv: 0, weight: 0,
          limit_pct: w.limit_pct ?? null,
          price: usable ? (q.price ?? null) : null,
          prev: usable ? (q.prev ?? null) : null,
          held: false,
        };
      });
    return [...held, ...watch];
  }, [view, watchRows, watchQuotes]);

  /* ---------- 深链开抽屉（?add=1 / ?code=&side=&qty=），组合快照就绪后消费一次 ---------- */
  const deepLinked = useRef(false);
  useEffect(() => {
    // B15：闸门从「候选池非空」改为「组合快照已加载」——空白账户（持仓+自选皆空）
    // 此前会静默不开抽屉；现在池空也开，code=null 落到抽屉的新标的手工输入兜底。
    if (deepLinked.current || !view) return;
    const code = params.get("code");
    const add = params.get("add");
    if (code && candidates.some((c) => c.code === code)) {
      deepLinked.current = true;
      const q = params.get("qty");
      setDrawer({ open: true, code, side: params.get("side") === "sell" ? "sell" : "buy", qty: q ? parseInt(q, 10) : null });
    } else if (add) {
      deepLinked.current = true;
      setDrawer({ open: true, code: candidates[0]?.code ?? null, side: "buy", qty: null });
    }
  }, [view, candidates, params]);

  const closeDrawer = () => {
    setDrawer((d) => ({ ...d, open: false }));
    deepLinked.current = false; // 允许空态引导 CTA 再次点开（否则第二次 ?add=1 被 ref 挡掉）
    // 清掉深链参数，避免刷新重开
    if (params.get("add") || params.get("code")) {
      const next = new URLSearchParams(params);
      next.delete("add"); next.delete("code"); next.delete("side"); next.delete("qty");
      setParams(next, { replace: true });
    }
  };
  const openTrade = (code: string | null, side: Side) => setDrawer({ open: true, code, side, qty: null });

  /* ---------- 表格排序 / 筛选 ---------- */
  const rows = useMemo(() => {
    if (!view) return [] as { h: HRow; p: PRow }[];
    const paired = view.holdings
      .map((h, i) => ({ h, p: view.per[i] }))
      .filter((x): x is { h: HRow; p: PRow } => x.p !== undefined);
    const get = (x: { h: HRow; p: PRow }): number | string => {
      switch (sortKey) {
        case "shares": return x.h.shares;
        case "cost": return x.h.cost;
        case "price": return x.h.price ?? -1;
        case "dayChg": return x.p.day_chg;
        case "mv": return x.p.mv;
        case "pnl": return x.p.pnl;
        case "weight": return x.p.weight;
      }
    };
    let list = paired.filter((x) => filterKind === "all" || x.h.kind === filterKind);
    if (query.trim()) {
      const q = query.trim().toLowerCase();
      list = list.filter((x) => (x.h.code + x.h.name + x.h.sector).toLowerCase().includes(q));
    }
    return list.sort((a, b) => {
      const av = get(a), bv = get(b);
      return (av > bv ? 1 : av < bv ? -1 : 0) * sortDir;
    });
  }, [view, sortKey, sortDir, filterKind, query]);

  const onSort = (k: SortKey) => {
    if (k === sortKey) setSortDir((d) => -d);
    else { setSortKey(k); setSortDir(-1); }
  };

  /* ---------- 汇总条（后端 agg/per 原值） ---------- */
  const win = per.filter((p) => p.pnl > 0).length;
  const cashShare = agg && agg.total_assets > 0 ? (agg.cash / agg.total_assets) * 100 : 0;
  const sumRows: [string, string, string, string][] = isEmpty
    ? [
        ["总资产", money(agg?.total_assets ?? 0), "当前全部为现金", ""],
        ["持仓市值", money(agg?.total_mv ?? 0), "仓位 0.0%", ""],
        ["当日浮动盈亏", "—", "录入第一笔后开始统计", ""],
        ["累计盈亏", "—", "录入第一笔后开始统计", ""],
        ["可用现金", money(agg?.cash ?? 0), `占总资产 ${fixed(cashShare, 1)}%`, ""],
        ["盈利标的", "0 / 0", "—", ""],
      ]
    : [
        ["总资产", money(agg?.total_assets ?? 0), "", ""],
        ["持仓市值", money(agg?.total_mv ?? 0), `仓位 ${fixed(agg?.exposure ?? 0, 1)}%`, ""],
        ["当日浮动盈亏", (agg && agg.day_pnl > 0 ? "+" : "") + fmt(agg?.day_pnl ?? 0, 2), pct(agg?.day_pnl_pct ?? 0), dirClass(agg?.day_pnl ?? 0)],
        ["累计盈亏", (agg && agg.total_pnl > 0 ? "+" : "") + fmt(agg?.total_pnl ?? 0, 0), pct(agg?.total_pnl_pct ?? 0), dirClass(agg?.total_pnl ?? 0)],
        ["可用现金", money(agg?.cash ?? 0), `占总资产 ${fixed(cashShare, 1)}%`, ""],
        ["盈利标的", `${win} / ${holdings.length}`, holdings.length ? `胜率 ${((win / holdings.length) * 100).toFixed(0)}%` : "—", ""],
      ];

  /* ---------- 仓位纪律四条（后端原值 + domain 阈值常量） ---------- */
  const maxWeight = per.length ? Math.max(...per.map((p) => p.weight)) : 0;
  const maxSectorW = agg && agg.sectors.length ? Math.max(...agg.sectors.map((s) => s.w)) : 0;
  const cbShare = agg && agg.total_assets > 0 ? (agg.mv_cb / agg.total_assets) * 100 : 0;
  const rules = [
    { k: "单票占组合上限", thr: LIMIT_SINGLE_STOCK, cur: maxWeight, inverse: false },
    { k: "单一行业占市值上限", thr: LIMIT_SECTOR, cur: maxSectorW, inverse: false },
    { k: "可转债合计占组合", thr: LIMIT_CB_TOTAL, cur: cbShare, inverse: false },
    { k: "现金最低保留", thr: LIMIT_CASH_RESERVE, cur: cashShare, inverse: true },
  ];

  /* ---------- CSV 导出（真实数据，客户端生成，不落后端） ---------- */
  const exportCsv = () => {
    if (!view || holdings.length === 0) {
      toast("暂无持仓可导出 · 先录入第一笔");
      return;
    }
    const head = ["代码", "名称", "市场", "类型", "行业", "持仓数量", "成本价", "现价", "市值", "今日涨跌%", "浮动盈亏", "浮动盈亏%", "占组合%"];
    const lines = view.holdings.map((h, i) => {
      const p = view.per[i];
      return [h.code, h.name, h.mkt, KIND_LABEL[h.kind], h.sector, h.shares, h.cost, h.price ?? "", p ? fixed(p.mv, 2) : "", p ? fixed(p.day_chg, 2) : "", p ? fixed(p.pnl, 2) : "", p ? fixed(p.pnl_pct, 2) : "", p ? fixed(p.weight, 2) : ""].join(",");
    });
    const csv = "﻿" + [head.join(","), ...lines].join("\r\n");
    const url = URL.createObjectURL(new Blob([csv], { type: "text/csv;charset=utf-8" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = `holdings-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    toast("已导出持仓明细 CSV（本地生成，不含真实券商数据）");
  };

  const saveCash = async () => {
    const v = Number(cashDraft);
    if (!Number.isFinite(v) || v < 0) {
      toast("现金需为非负数字");
      return;
    }
    const ok = await usePortfolio.getState().setCash(v);
    toast(ok ? `可用现金已设为 ${money(v)}` : (usePortfolio.getState().error ?? "更新现金失败"));
    if (ok) setEditingCash(false);
  };

  const desc = !view
    ? pError
      ? `后端不可用：${pError}。启动 zimu-server 后点顶栏刷新重试。`
      : pLoading
        ? "正在读取组合与行情…"
        : "尚未连接后端。"
    : isEmpty
      ? "账户里还没有持仓。先在汇总条「可用现金 · 调整」设入实际资金，再点右上角「录入第一笔」：标的可从自选挑，也可在抽屉里手工输入新标的；整手、涨跌停与摊薄成本即时校验和试算。"
      : agg && per.length
        ? `组合里最大的一只占总资产 ${fixed(maxWeight, 1)}%，前三大合计 ${fixed(agg.top3_sector, 1)}%（行业口径）。点「加减仓」试算一笔委托，摊薄成本与占比会即时重算。`
        : "正在读取组合快照…";

  const th = (label: string, key?: SortKey, txt = false) => (
    <th className={txt ? "txt" : undefined} onClick={key ? () => onSort(key) : undefined} style={key ? { cursor: "pointer" } : undefined}
      aria-sort={key && sortKey === key ? (sortDir < 0 ? "descending" : "ascending") : undefined}>
      {key ? (
        <span className="tbl-sort">
          {label}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" style={{ width: 10, height: 10 }}>
            <path d={sortKey === key && sortDir > 0 ? "M6 15l6-6 6 6" : "M6 9l6 6 6-6"} />
          </svg>
        </span>
      ) : (
        label
      )}
    </th>
  );

  return (
    <main className="page" data-od-id="holdings-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">持仓管理</div>
          <h1 className="page-title">个股明细与加减仓</h1>
          <p className="page-desc" id="h-desc">{desc}</p>
        </div>
        <div className="page-actions">
          <button className="btn" id="export-btn" onClick={exportCsv}>导出明细</button>
          {/* 直接维护入口（ADR-0009）：快照校正——初始化映射 / 纠错 / 场外变动 */}
          <button className="btn" id="direct-entry-btn" onClick={() => setForm({ open: true, initial: null })}>直接录入</button>
          <button
            className="btn btn-primary" id="trade-btn"
            // B15：候选池为空不再是死路——code=null 时抽屉落到「新标的（手工输入）」
            onClick={() => openTrade(holdings[0]?.code ?? candidates[0]?.code ?? null, "buy")}
          >
            {isEmpty ? "＋ 录入第一笔" : "＋ 记一笔加减仓"}
          </button>
        </div>
      </div>

      {!view && (
        <section className="card" data-od-id="holdings-error" style={{ marginBottom: 16 }}>
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

      {/* 汇总条 */}
      <div className="sumbar" id="sumbar" data-od-id="holdings-summary">
        {sumRows.map((r, i) => (
          <div data-od-id={`sum-${i}`} key={r[0]}>
            <div className="k">
              {r[0]}
              {i === 4 && !editingCash && (
                <button className="chip" style={{ marginLeft: 8, padding: "1px 7px" }} onClick={() => { setCashDraft(String(Math.round(agg?.cash ?? 0))); setEditingCash(true); }}>
                  调整
                </button>
              )}
            </div>
            {i === 4 && editingCash ? (
              <div style={{ display: "flex", gap: 6, marginTop: 5, alignItems: "center" }}>
                <input className="input num" style={{ height: 30, padding: "0 8px", width: "100%" }} inputMode="decimal" value={cashDraft} autoFocus
                  onChange={(e) => setCashDraft(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") void saveCash(); }} />
                <button className="btn btn-sm" onClick={() => void saveCash()}>存</button>
                <button className="btn btn-sm" onClick={() => setEditingCash(false)}>×</button>
              </div>
            ) : (
              <>
                <div className={`v ${r[3]}`}>{r[1]}</div>
                <div className="hint" style={{ margin: "5px 0 0" }}>{r[2]}</div>
              </>
            )}
          </div>
        ))}
      </div>

      {/* 持仓表 */}
      <section className="card" data-od-id="holdings-table-card" style={{ marginTop: 16 }}>
        <div className="card-head">
          <div>
            <div className="card-title">全部持仓</div>
            <div className="card-sub" id="h-count">
              {view ? `显示 ${rows.length} / ${holdings.length} 只 · 按${SORT_LABEL[sortKey]}${sortDir < 0 ? "降序" : "升序"}` : "—"}
            </div>
          </div>
          <div className="head-actions">
            <div className="seg" role="group" aria-label="按类型筛选" id="kind-filter">
              {[["all", "全部"], ["stock", "股票"], ["etf", "ETF / 基金"], ["cb", "可转债"]].map(([k, label]) => (
                <button type="button" key={k} data-k={k} aria-pressed={filterKind === k} onClick={() => setFilterKind(k)}>{label}</button>
              ))}
            </div>
          </div>
        </div>
        <div className="filter-row">
          <label className="search" style={{ marginLeft: 0, width: 220 }}>
            <IconSearch />
            <input id="h-search" placeholder="筛选代码或名称" autoComplete="off" value={query} onChange={(e) => setQuery(e.target.value)} />
          </label>
          <span className="hint" style={{ margin: 0 }}>列标题可点击排序 · 数字均为等宽对齐</span>
          <span style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
            <label style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 12, color: "var(--muted)", cursor: "pointer" }}>
              <span className="switch">
                <input type="checkbox" id="show-weight" checked={showWeight} onChange={(e) => setShowWeight(e.target.checked)} />
                <span className="track" />
              </span>
              显示仓位占比条
            </label>
          </span>
        </div>
        <div className="card-body tight">
          <div className="tbl-wrap">
            <table className="tbl" id="h-table">
              <thead>
                <tr>
                  {th("标的", undefined, true)}
                  {th("行业 / 类型", undefined, true)}
                  {th("持仓数量", "shares", true)}
                  {th("成本价", "cost")}
                  {th("现价", "price")}
                  {th("今日涨跌", "dayChg")}
                  {th("市值", "mv")}
                  {th("浮动盈亏", "pnl")}
                  {th("占组合", "weight")}
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {rows.length ? (
                  rows.map(({ h, p }) => {
                    const dec = priceDecimals(h.kind);
                    return (
                      <tr data-od-id={`hrow-${h.code}`} key={h.code}>
                        <td className="txt">
                          <div className="name">{h.name}</div>
                          <div className="sub code">{h.code} · {h.mkt}</div>
                        </td>
                        <td className="txt">
                          <span className="pill plain" style={{ borderColor: "var(--border)" }}>{h.sector || "—"}</span>
                          <div className="sub">{KIND_LABEL[h.kind]}</div>
                        </td>
                        <td className="num">{fmt(h.shares, 0)} <span style={{ color: "var(--muted)", fontSize: 11 }}>{UNIT[h.kind]}</span></td>
                        <td className="num">{fmt(h.cost, dec)}</td>
                        <td className="num strong">
                          <span className="tick">{h.price != null && h.price > 0 ? fmt(h.price, dec) : "—"}</span>
                        </td>
                        <td className={`num ${dirClass(p.day_chg)}`}>{pct(p.day_chg)}</td>
                        <td className="num">{moneyShort(p.mv)}</td>
                        <td className={`num ${dirClass(p.pnl)}`}>
                          {(p.pnl > 0 ? "+" : "") + moneyShort(p.pnl)}
                          <div className={`sub ${dirClass(p.pnl)}`}>{pct(p.pnl_pct)}</div>
                        </td>
                        <td className="num">
                          {fixed(p.weight, 1)}%
                          {showWeight && (
                            <div className="tr" style={{ height: 4, marginTop: 5, width: 70, marginLeft: "auto", background: "var(--surface-2)", borderRadius: 2, overflow: "hidden" }}>
                              <i style={{ display: "block", height: "100%", width: `${Math.min(100, (p.weight / 30) * 100)}%`, background: p.weight > 25 ? "var(--warn)" : "var(--fg-2)" }} />
                            </div>
                          )}
                        </td>
                        <td>
                          {deletingCode === h.code ? (
                            // 两步确认（行内，不引入弹窗组件）：删除是快照校正，流水保留
                            <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                              <button className="btn btn-sm" style={{ color: "var(--danger)", borderColor: "var(--danger)" }}
                                onClick={() => void doDelete(h.code, h.name)}>确认删除</button>
                              <button className="btn btn-sm" onClick={() => setDeletingCode(null)}>取消</button>
                            </div>
                          ) : (
                            <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                              <button className="btn btn-sm" onClick={() => openTrade(h.code, "buy")}>加仓</button>
                              <button className="btn btn-sm" onClick={() => openTrade(h.code, "sell")}>减仓</button>
                              <button className="btn btn-sm" onClick={() => setForm({ open: true, initial: h })}>编辑</button>
                              <button className="btn btn-sm" onClick={() => setDeletingCode(h.code)}>删除</button>
                              <Link className="btn btn-sm" to={`/analysis?code=${h.code}`}>分析</Link>
                            </div>
                          )}
                        </td>
                      </tr>
                    );
                  })
                ) : holdings.length ? (
                  <tr><td colSpan={10}><div className="empty">没有匹配的持仓 · 换个筛选条件或清空搜索</div></td></tr>
                ) : (
                  <tr>
                    <td colSpan={10}>
                      <EmptyHero
                        od="h-table-empty"
                        title="还没有持仓"
                        desc="录入第一笔后，市值、成本、涨跌与仓位占比立刻现算。标的可从自选挑（总览仪表盘的自选卡添加），也可直接在录入抽屉里手工输入新标的。"
                      />
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
        <div className="card-foot" style={{ justifyContent: "space-between" }}>
          <span className="hint" style={{ margin: 0 }} id="h-foot-sum">
            {agg ? `共 ${holdings.length} 只标的 · 投入成本 ${money(agg.cost_total)} · 市值 ${money(agg.total_mv)} · 现金 ${money(agg.cash)}` : "—"}
          </span>
          <span className="hint" style={{ margin: 0 }}>加减仓会即时重算摊薄成本与仓位占比</span>
        </div>
      </section>

      {/* 流水 + 纪律 */}
      <div className="grid g-2" style={{ marginTop: 16 }}>
        <section className="card" data-od-id="order-log-card">
          <div className="card-head">
            <div>
              <div className="card-title">委托 / 成交流水</div>
              <div className="card-sub">本账户记录，不含真实券商成交回报</div>
            </div>
          </div>
          <div className="card-body tight">
            <div className="tbl-wrap">
              <table className="tbl" id="log-table">
                <thead>
                  <tr>
                    <th className="txt">时间</th><th className="txt">标的</th><th className="txt">方向</th>
                    <th>数量</th><th>价格</th><th>费用</th><th>已实现盈亏</th>
                  </tr>
                </thead>
                <tbody>
                  {tradesError && !trades.length ? (
                    <tr><td colSpan={7}><div className="empty">流水读取失败：{tradesError}</div></td></tr>
                  ) : trades.length ? (
                    trades.map((t, i) => (
                      <tr data-od-id={`log-row-${i}`} key={t.id}>
                        <td className="txt" style={{ fontFamily: "var(--font-mono)", fontSize: 11.5, color: "var(--muted)" }}>{t.ts}</td>
                        <td className="txt"><span className="name">{t.name}</span> <span className="code">{t.code}</span></td>
                        <td className="txt"><span className={`pill ${t.side === "buy" ? "buy" : "sell"}`}>{t.side === "buy" ? "买入" : "卖出"}</span></td>
                        <td className="num">{fmt(t.qty, 0)}</td>
                        <td className="num">{fmt(t.price, priceDecimals(t.kind))}</td>
                        <td className="num" style={{ color: "var(--muted)" }}>{fmt(t.fee, 2)}</td>
                        <td className={`num ${t.realized == null ? "" : dirClass(t.realized)}`}>
                          {t.realized == null ? "—" : (t.realized > 0 ? "+" : "") + fmt(t.realized, 2)}
                        </td>
                      </tr>
                    ))
                  ) : (
                    <tr><td colSpan={7}><div className="empty">还没有委托 / 成交记录 · 记入第一笔后这里自动留档</div></td></tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </section>

        <section className="card" data-od-id="rule-card">
          <div className="card-head">
            <div>
              <div className="card-title">仓位纪律</div>
              <div className="card-sub">触发即预警，并在微信推送里给出建议动作</div>
            </div>
          </div>
          <div className="card-body" id="rules">
            {isEmpty || !agg ? (
              <EmptyHero
                Icon={IconShield}
                od="h-rules-empty"
                title="仓位纪律待首笔持仓生效"
                desc="录入第一笔后，这里逐条现算单票上限、行业集中度、转债占比与现金下限，触及即在预警与微信里给出建议动作。"
                actions={null}
              />
            ) : (
              rules.map((r, i) => {
                const hit = r.inverse ? r.cur < r.thr - THRESHOLD_EPS : r.cur > r.thr + THRESHOLD_EPS;
                const bar = Math.min(100, r.inverse ? (r.cur > 0 ? (r.thr / r.cur) * 100 : 100) : (r.cur / r.thr) * 100);
                return (
                  <div style={{ marginBottom: i === rules.length - 1 ? 0 : 15 }} data-od-id={`rule-${i}`} key={r.k}>
                    <div style={{ display: "flex", alignItems: "baseline", gap: 8, fontSize: 12.5 }}>
                      <span>{r.k}</span>
                      <span className="num" style={{ marginLeft: "auto", color: "var(--muted)", fontSize: 11.5 }}>{r.inverse ? "下限" : "上限"} {r.thr}%</span>
                      <span className={`num ${hit ? "danger" : "ok"}`} style={{ fontWeight: 550 }}>{fixed(r.cur, 1)}%</span>
                      <span className={`pill ${hit ? "warn" : ""}`}>{hit ? "已触及" : "安全"}</span>
                    </div>
                    <div className="progress" style={{ marginTop: 7 }}>
                      <i style={{ width: `${fixed(bar, 0)}%`, background: hit ? "var(--warn)" : "var(--fg-2)" }} />
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </section>
      </div>

      {/* 维护记录（ADR-0009）：直接维护留痕——交易录入走上方流水，两本账各自审计 */}
      <section className="card" data-od-id="holding-edits-card" style={{ marginTop: 16 }}>
        <div className="card-head">
          <div>
            <div className="card-title">维护记录</div>
            <div className="card-sub">直接维护（快照校正）留痕：不生成流水、不动现金——初始化映射 / 纠错 / 场外变动（03 §13）</div>
          </div>
        </div>
        <div className="card-body tight">
          {edits.length ? (
            edits.map((e) => (
              <div data-od-id={`edit-row-${e.id}`} key={e.id}
                style={{ display: "flex", gap: 10, alignItems: "baseline", flexWrap: "wrap", padding: "8px 12px", borderBottom: "1px solid var(--border)", fontSize: 12.5 }}>
                <span className="num" style={{ fontFamily: "var(--font-mono)", fontSize: 11.5, color: "var(--muted)", flex: "none" }}>{e.ts}</span>
                <span className={`pill ${OP_PILL[e.op] ?? ""}`} style={{ flex: "none" }}>{OP_LABEL[e.op] ?? e.op}</span>
                <span style={{ flex: "none" }}><b>{e.name}</b> <span className="code">{e.code}</span></span>
                <span className="num" style={{ color: "var(--fg-2)" }}>{editSummary(e)}</span>
                {e.note && <span className="hint" style={{ margin: 0, marginLeft: "auto", flex: "none" }}>{e.note}</span>}
              </div>
            ))
          ) : (
            <div className="empty">暂无维护留痕 · 「直接录入」、行内「编辑 / 删除」会在这里留档；买卖录入走上方委托 / 成交流水</div>
          )}
        </div>
      </section>

      <div className="footnote" data-od-id="h-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          费用按「佣金万 2.5（单笔最低 5 元）+ 卖出印花税 0.05% + 过户费 0.001%（沪深两市均收）」估算，可转债与 ETF 免印花税，实际以券商结算为准。
          试算数字与后端 <code>zimu-domain</code> 同源（黄金向量校验），但<b>试算 ≠ 下单</b>——程序不连接券商、不代下单，每笔委托仍需你本人确认。
          直接维护（编辑 / 删除 / 直接录入）是<b>快照校正</b>：不生成流水、不动现金，删除后历史流水保留、累计已实现盈亏不回滚（ADR-0009）。
        </span>
      </div>

      <TradeDrawer
        open={drawer.open}
        candidates={candidates}
        cash={agg?.cash ?? 0}
        totalMv={agg?.total_mv ?? 0}
        initialCode={drawer.code}
        initialSide={drawer.side}
        initialQty={drawer.qty}
        onClose={closeDrawer}
        onSubmitted={(receipt) => {
          toast(receiptToast(receipt));
          void useTrades.getState().load();
        }}
      />

      {/* 直接维护表单（ADR-0009）：新增 = 页头「直接录入」，编辑 = 行内「编辑」 */}
      <HoldingFormDrawer
        open={form.open}
        initial={form.initial}
        onClose={() => setForm({ open: false, initial: null })}
        onSaved={(msg) => {
          toast(msg);
          void useHoldingEdits.getState().load();
        }}
      />
    </main>
  );
}
