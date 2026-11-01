import { useEffect, useMemo, useState } from "react";
import {
  applyTrade,
  limitPrices,
  limitPct,
  lotSize,
  priceDecimals,
  quickTiers,
  validatePrice,
  validateQty,
  type Kind,
  type Side,
  type TradeOutcome,
} from "../domain";
import { dirClass, fixed, fmt, money, pct } from "../lib/format";
import { KIND_LABEL, symbolOf } from "../lib/kind";
import type { RecordReceipt, TradeRequest } from "../store/portfolio";
import { usePortfolio } from "../store/portfolio";

/**
 * 加减仓抽屉（P5-T2，ADR-0006 手工逐笔 + ADR-0008 唯一试算例外）。
 * 「试算 ≠ 下单」是产品核心交互：数量/价格即时校验（整手 lot、涨跌停区间、
 * 可卖上限、现金余额）与费用/摊薄成本/调整后仓位预览全部走 src/domain 纯函数
 * （黄金向量同款，与后端 zimu-domain 逐位一致）；点「记入持仓」才 POST
 * /api/holdings/trade，成功后以后端收据覆盖（recordTrade 内整表重拉）。
 * 后端仍是最终裁判：涨跌停越界/现金不足/缺 meta 会被 422/400 拒，拒绝体带
 * limit_up/cash_after 等字段就地高亮（客户端校验只为即时反馈，不替代后端）。
 */

export interface Candidate {
  code: string;
  name: string;
  kind: Kind;
  /** 显式市场码 SH/SZ（不按代码首字母猜，B7） */
  mkt: string;
  sector: string;
  /** 当前持仓数量；0 = 未持有（自选候选，买入即新建持仓） */
  shares: number;
  /** 当前摊薄成本；未持有为 0 */
  cost: number;
  /** 当前市值（per.mv 原值；未持有为 0）——算 others_mv 用 */
  mv: number;
  /** 当前占组合 %（per.weight 原值；未持有为 0）——预览「调整前」用后端值不重算 */
  weight: number;
  /** 显式涨跌停比例（配置携带；null → 按 §2 推断） */
  limit_pct: number | null;
  /** 参考价（持仓回写价 / 自选 quote 价）；null = 无行情，需手填 */
  price: number | null;
  /** 昨收（涨跌停基准）；null = 无行情，跳过区间拦截 */
  prev: number | null;
  /** 是否当前持有（决定标题「加减仓 / 录入第一笔」与是否携带 meta） */
  held: boolean;
}

const UNIT: Record<Kind, string> = { stock: "股", etf: "份", cb: "张" };

/** 候选下拉的「新标的（手工输入）」哨兵值——6 位数字代码不可能与之相撞 */
const NEW_CODE = "__new__";

/** 新标的手工表单初值（字段口径 = 仪表盘自选卡表单，B7：mkt 显式不猜） */
const EMPTY_MANUAL = { code: "", name: "", mkt: "SH", kind: "stock" as Kind, sector: "", limitPct: "" };

interface Props {
  open: boolean;
  candidates: Candidate[];
  cash: number;
  totalMv: number;
  initialCode?: string | null;
  initialSide?: Side;
  initialQty?: number | null;
  onClose: () => void;
  /** 提交成功：调用方重拉流水 + toast（收据数字来自后端） */
  onSubmitted: (receipt: RecordReceipt) => void;
}

export default function TradeDrawer({
  open,
  candidates,
  cash,
  totalMv,
  initialCode,
  initialSide = "buy",
  initialQty = null,
  onClose,
  onSubmitted,
}: Props) {
  const [code, setCode] = useState<string>("");
  const [side, setSide] = useState<Side>("buy");
  const [qtyStr, setQtyStr] = useState("");
  const [priceStr, setPriceStr] = useState("");
  const [note, setNote] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  // 新标的手工输入（B15：空白账户候选池为空的兜底路径）
  const [manual, setManual] = useState(EMPTY_MANUAL);

  /** 手工表单合法 → 合成未持有候选（price/prev 无行情 → 价格手填、跳过区间拦截，后端仍是裁判） */
  const manualCandidate = useMemo<Candidate | null>(() => {
    if (code !== NEW_CODE) return null;
    const c = manual.code.trim();
    const n = manual.name.trim();
    if (!symbolOf(manual.mkt, c) || !n) return null;
    const lp = manual.limitPct.trim() === "" ? NaN : Number(manual.limitPct);
    return {
      code: c, name: n, kind: manual.kind, mkt: manual.mkt, sector: manual.sector.trim(),
      shares: 0, cost: 0, mv: 0, weight: 0,
      limit_pct: Number.isFinite(lp) && lp > 0 ? lp : null,
      price: null, prev: null, held: false,
    };
  }, [code, manual]);

  const cur = code === NEW_CODE ? manualCandidate : (candidates.find((c) => c.code === code) ?? null);

  // 打开时按 initialCode 预选（无则第一个候选；候选池为空 → 落到新标的手工输入），重置全部输入
  useEffect(() => {
    if (!open) return;
    const first = initialCode && candidates.some((c) => c.code === initialCode)
      ? initialCode
      : (candidates[0]?.code ?? NEW_CODE);
    setCode(first);
    setSide(initialSide);
    setQtyStr(initialQty != null ? String(initialQty) : "");
    setNote("");
    setConfirmed(false);
    setSubmitError(null);
    setSubmitting(false);
    setManual(EMPTY_MANUAL);
    const c = candidates.find((x) => x.code === first);
    setPriceStr(c?.price != null ? fixed(c.price, priceDecimals(c.kind)) : "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, initialCode, initialSide, initialQty]);

  const meta = useMemo(() => {
    if (!cur) return null;
    const lot = lotSize(cur.kind);
    const dec = priceDecimals(cur.kind);
    const unit = UNIT[cur.kind];
    // 涨跌停比例：显式配置优先，否则按 §2 推断（ETF 推断为 null → 跳过区间拦截）
    const effPct = cur.limit_pct ?? limitPct(cur.kind, cur.code, false);
    const lim = effPct != null ? limitPrices(cur.kind, cur.prev ?? 0, effPct) : { limit_up: null, limit_down: null };
    return { lot, dec, unit, effPct, lim };
  }, [cur]);

  const qty = parseInt(qtyStr.replace(/\D/g, ""), 10) || 0;
  const refPrice = cur?.price ?? 0;
  const price = parseFloat(priceStr) || refPrice;
  const held = cur?.shares ?? 0;
  const cost = cur?.cost ?? 0;
  const othersMv = Math.max(0, totalMv - (cur?.mv ?? 0));

  /* 实时校验 + 试算（全部 domain 纯函数，与后端逐位一致） */
  const qtyVerdict = cur ? validateQty(cur.kind, side, qty, held) : { valid: true, reason: null };
  let errQty = "";
  if (cur && qty > 0 && qtyVerdict.reason === "not_lot_multiple") {
    errQty = `数量需为 ${meta?.lot} 的整数倍（当前 ${qty}）`;
  } else if (cur && qtyVerdict.reason === "exceeds_held") {
    errQty = `最多可卖 ${fmt(held, 0)} ${meta?.unit}`;
  }

  let badPrice = false;
  if (cur && meta && price > 0 && meta.lim.limit_up != null && meta.lim.limit_down != null) {
    const pv = validatePrice(price, meta.lim.limit_up, meta.lim.limit_down);
    badPrice = !pv.valid && pv.reason !== "invalid_price";
  }

  const outcome: TradeOutcome | null =
    cur && qty > 0 && price > 0
      ? applyTrade({ kind: cur.kind, side, qty, price, held, cost, cash, others_mv: othersMv })
      : null;

  const canSubmit =
    !!cur && !!outcome && qty > 0 && !errQty && !badPrice && qty % (meta?.lot ?? 1) === 0 &&
    (side === "buy" ? !outcome.insufficient_cash : qty <= held) && confirmed && !submitting;

  const tiers = cur ? quickTiers(cur.kind, side, held) : [];

  const submit = async () => {
    if (!cur || !outcome || !canSubmit) return;
    setSubmitting(true);
    setSubmitError(null);
    const isNew = side === "buy" && !cur.held;
    const body: TradeRequest = {
      side,
      code: cur.code,
      qty,
      price,
      ...(note.trim() ? { note: note.trim() } : {}),
      // 新建持仓需携带 meta（B7：mkt 显式；ETF 无法推断的 limit_pct 由配置带来）
      ...(isNew
        ? {
            meta: {
              kind: cur.kind,
              mkt: cur.mkt,
              name: cur.name,
              ...(cur.prev != null ? { prev: cur.prev } : {}),
              ...(cur.sector ? { sector: cur.sector } : {}),
              ...(meta?.effPct != null ? { limit_pct: meta.effPct } : {}),
            },
          }
        : {}),
    };
    const res = await usePortfolio.getState().recordTrade(body);
    setSubmitting(false);
    if (res.ok) {
      onSubmitted(res.receipt);
      onClose();
    } else {
      const e = res.error;
      // 特定拒绝就地高亮：涨跌停越界 / 现金不足（后端为最终裁判，03 §2 / §4）
      let msg = e.message ?? "录入失败";
      if (e.error === "out_of_limit_range" && e.limit_up != null && e.limit_down != null) {
        msg += `（${fixed(e.limit_down, e.decimals ?? 2)} — ${fixed(e.limit_up, e.decimals ?? 2)}）`;
      } else if (e.error === "insufficient_cash" && e.cash_after != null) {
        msg += `（成交后 ${money(e.cash_after)}）`;
      }
      setSubmitError(msg);
    }
  };

  // Escape 关闭（原型同款）
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  const dec = meta?.dec ?? 2;
  const unit = meta?.unit ?? "股";
  const title = held > 0 || side === "sell" ? "加减仓" : "录入第一笔";

  const pvRow = (k: string, v: React.ReactNode, vClass = "") => (
    <div className="row">
      <span className="k">{k}</span>
      <span className={`v ${vClass}`}>{v}</span>
    </div>
  );

  return (
    <>
      <div className="scrim on" onClick={onClose} />
      <aside className="drawer on" role="dialog" aria-modal="true" aria-labelledby="drawer-title" data-od-id="trade-drawer">
        <div className="drawer-head">
          <div style={{ flex: 1 }}>
            <div className="card-title" id="drawer-title">{title}</div>
            <div className="card-sub" id="drawer-sub">
              {cur ? `${cur.name} ${cur.code} · ${KIND_LABEL[cur.kind]}` : "选择标的"}
            </div>
          </div>
          <button className="icon-btn" aria-label="关闭" onClick={onClose}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>

        <div className="drawer-body">
          <div className="seg" role="group" aria-label="方向" style={{ width: "100%", marginBottom: 18 }}>
            {(["buy", "sell"] as Side[]).map((s) => (
              <button key={s} type="button" data-side={s} aria-pressed={side === s} style={{ flex: 1 }} onClick={() => setSide(s)}>
                {s === "buy" ? "买入 / 加仓" : "卖出 / 减仓"}
              </button>
            ))}
          </div>

          <div className="field" id="f-code">
            <label htmlFor="sel-code">标的</label>
            <select className="select" id="sel-code" value={code} onChange={(e) => setCode(e.target.value)}>
              {candidates.map((c) => (
                <option key={c.code} value={c.code}>
                  {c.name} {c.code} · {c.held ? KIND_LABEL[c.kind] : "自选未持有"}
                </option>
              ))}
              <option value={NEW_CODE}>＋ 新标的（手工输入）</option>
            </select>
            {code === NEW_CODE ? (
              <>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 92px", gap: 10, marginTop: 10 }} id="manual-form">
                  <div className="field" style={{ marginBottom: 0 }}>
                    <label htmlFor="m-code">代码（6 位数字）</label>
                    <input className="input num" id="m-code" inputMode="numeric" placeholder="例如 600519" maxLength={6}
                      value={manual.code} onChange={(e) => setManual((m) => ({ ...m, code: e.target.value.replace(/\D/g, "").slice(0, 6) }))} />
                  </div>
                  <div className="field" style={{ marginBottom: 0 }}>
                    <label htmlFor="m-mkt">市场</label>
                    <select className="select" id="m-mkt" value={manual.mkt} onChange={(e) => setManual((m) => ({ ...m, mkt: e.target.value }))}>
                      {["SH", "SZ", "BJ"].map((m) => <option key={m} value={m}>{m}</option>)}
                    </select>
                  </div>
                  <div className="field" style={{ marginBottom: 0, gridColumn: "1 / -1" }}>
                    <label htmlFor="m-name">名称</label>
                    <input className="input" id="m-name" placeholder="例如 贵州茅台"
                      value={manual.name} onChange={(e) => setManual((m) => ({ ...m, name: e.target.value }))} />
                  </div>
                  <div className="field" style={{ marginBottom: 0 }}>
                    <label htmlFor="m-kind">类型</label>
                    <select className="select" id="m-kind" value={manual.kind} onChange={(e) => setManual((m) => ({ ...m, kind: e.target.value as Kind }))}>
                      {(Object.keys(KIND_LABEL) as Kind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
                    </select>
                  </div>
                  <div className="field" style={{ marginBottom: 0 }}>
                    <label htmlFor="m-limit">涨跌停 %（可选）</label>
                    <input className="input num" id="m-limit" inputMode="decimal" placeholder="留空按 §2 推断"
                      value={manual.limitPct} onChange={(e) => setManual((m) => ({ ...m, limitPct: e.target.value }))} />
                  </div>
                  <div className="field" style={{ marginBottom: 0, gridColumn: "1 / -1" }}>
                    <label htmlFor="m-sector">行业（可选，纪律集中度用）</label>
                    <input className="input" id="m-sector" placeholder="例如 白酒"
                      value={manual.sector} onChange={(e) => setManual((m) => ({ ...m, sector: e.target.value }))} />
                  </div>
                </div>
                <div className="hint" id="code-hint">
                  {manualCandidate
                    ? "新标的：买入后将新建持仓。无行情参考价——委托价格请手填；未填昨收则跳过涨跌停拦截（后端仍是最终裁判）。"
                    : "填写 6 位代码与名称后即可录入数量与价格（市场/类型必选，涨跌停 % 留空按 03 §2 推断）。"}
                </div>
              </>
            ) : (
              <div className="hint" id="code-hint">
                {cur
                  ? held > 0
                    ? `当前持有 ${fmt(held, 0)} ${unit} · 成本 ${fmt(cost, dec)} · 现价 ${cur.price != null ? fmt(cur.price, dec) : "—"}`
                    : "尚未持有，买入后将新建持仓。"
                  : "—"}
              </div>
            )}
          </div>

          <div className={`field${errQty ? " invalid" : ""}`} id="f-qty">
            <label htmlFor="in-qty">数量</label>
            <div className="with-suffix">
              <input
                className="input num" id="in-qty" inputMode="numeric" placeholder="例如 200"
                value={qtyStr} onChange={(e) => setQtyStr(e.target.value)}
              />
              <span className="suffix" id="unit">{unit}</span>
            </div>
            <div className="err" id="err-qty" style={errQty ? { display: "block" } : undefined}>{errQty || "请填写有效数量"}</div>
            <div className="hint" id="lot-hint">
              {cur
                ? `${KIND_LABEL[cur.kind]}最小单位 1 手 = ${meta?.lot} ${unit}，数量需为 ${meta?.lot} 的整数倍` +
                  // B12 定稿：奇零股持仓提示一次性清仓豁免
                  (held > 0 && meta && held % meta.lot !== 0
                    ? `；持仓含奇零（${fmt(held, 0)} ${unit}），全部清仓可一次性卖出`
                    : "")
                : "—"}
            </div>
            <div style={{ display: "flex", gap: 6, marginTop: 8, flexWrap: "wrap" }} id="qty-quick">
              {tiers.slice(0, 3).map((q, i) => (
                <button className="btn btn-sm" key={i} type="button" onClick={() => setQtyStr(String(q))}>
                  {side === "sell" ? "减" : "加"} {[10, 25, 50][i]}% · {fmt(q, 0)} {unit}
                </button>
              ))}
              {side === "sell" && held > 0 && tiers.length > 3 && (
                <button className="btn btn-sm" type="button" onClick={() => setQtyStr(String(tiers[3]))}>全部清仓</button>
              )}
            </div>
          </div>

          <div className={`field${badPrice ? " invalid" : ""}`} id="f-price">
            <label htmlFor="in-price">委托价格</label>
            <div className="with-suffix">
              <input
                className="input num" id="in-price" inputMode="decimal"
                value={priceStr} onChange={(e) => setPriceStr(e.target.value)}
              />
              <span className="suffix">元</span>
            </div>
            <div className="err" id="err-price" style={badPrice ? { display: "block" } : undefined}>
              {badPrice && meta?.lim.limit_down != null && meta?.lim.limit_up != null
                ? `价格超出涨跌停区间 ${fmt(meta.lim.limit_down, dec)} — ${fmt(meta.lim.limit_up, dec)}`
                : "价格需在合理区间"}
            </div>
            <div className="hint">
              默认使用最新价；实盘请改为你的挂单价。涨跌停价：{" "}
              <span className="num" id="limit-txt">
                {meta?.lim.limit_up != null && meta?.lim.limit_down != null
                  ? `${fmt(meta.lim.limit_down, dec)} — ${fmt(meta.lim.limit_up, dec)}（±${meta.effPct}%）`
                  : "—（无行情或未配置比例，跳过区间拦截）"}
              </span>
            </div>
          </div>

          <div className="preview" id="preview" data-od-id="trade-preview">
            {pvRow("方向 / 数量", `${side === "buy" ? "买入" : "卖出"} ${qty ? fmt(qty, 0) : "—"} ${unit}`)}
            {pvRow("成交金额", outcome ? money(outcome.amount) : "—")}
            {pvRow(
              "估算费用",
              outcome ? (
                <>
                  {money(outcome.fee)}{" "}
                  <span className="arrow-mock">{outcome.fee <= 5 ? "按最低 5 元佣金" : "佣金 + 税费"}</span>
                </>
              ) : (
                "—"
              ),
            )}
            {pvRow("持仓变动", <>{fmt(held, 0)} → <b>{outcome ? fmt(outcome.after_shares, 0) : "—"}</b></>)}
            {pvRow(
              "摊薄成本",
              <>
                {fmt(cost || price, dec)} → <b>{outcome?.after_cost != null ? fmt(outcome.after_cost, dec) : "—"}</b>
              </>,
            )}
            {pvRow(
              "占组合（估）",
              <>
                {cur ? fixed(cur.weight, 1) : "0.0"}% → <b>{outcome ? fixed(outcome.weight_after, 1) : "—"}%</b>
              </>,
            )}
            {pvRow(
              "现金余额",
              outcome ? (
                <>
                  {money(outcome.cash_after)}
                  {outcome.insufficient_cash ? "（资金不足）" : ""}
                </>
              ) : (
                "—"
              ),
              outcome?.insufficient_cash ? "danger" : "",
            )}
            {outcome?.realized != null && qty > 0 &&
              pvRow(
                "已实现盈亏",
                <>{outcome.realized_pct != null ? pct(outcome.realized_pct) : "—"} · {money(outcome.realized)}</>,
                dirClass(outcome.realized),
              )}
            {outcome?.over_limit && qty > 0 && (
              <div className="row">
                <span className="k" style={{ color: "var(--warn)" }}>纪律提示</span>
                <span className="v" style={{ color: "var(--warn)" }}>单票将超过 25% 上限</span>
              </div>
            )}
          </div>

          {outcome?.insufficient_cash && side === "buy" && (
            <div className="hint" id="cash-hint" style={{ marginTop: 10, color: "var(--warn)" }}>
              可用现金不足——新账户先在持仓页汇总条「可用现金 · 调整」设入实际资金，再录入第一笔。
            </div>
          )}

          {submitError && (
            <div className="err" style={{ display: "block", marginTop: 12 }}>提交被拒：{submitError}</div>
          )}

          <div className="field" style={{ marginTop: 16 }}>
            <label htmlFor="in-note">备注（可选，会写入流水）</label>
            <input className="input" id="in-note" placeholder="例如：按助理建议降到 15%" value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
          <label style={{ display: "flex", gap: 9, alignItems: "flex-start", fontSize: 12, color: "var(--muted)", lineHeight: 1.5 }}>
            <span className="switch" style={{ marginTop: -1 }}>
              <input type="checkbox" id="confirm-flag" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
              <span className="track" />
            </span>
            <span>我已确认这是自己下的决定，助理的建议仅作参考，不代为执行真实委托。</span>
          </label>
        </div>

        <div className="drawer-foot">
          <button className="btn" style={{ flex: "none" }} onClick={onClose}>取消</button>
          <button className="btn btn-primary" style={{ flex: 1 }} disabled={!canSubmit} onClick={() => void submit()}>
            {submitting ? "记入中…" : "记入持仓"}
          </button>
        </div>
      </aside>
    </>
  );
}

/** 提交成功 toast 文案（收据数字来自后端，不在前端复算） */
export function receiptToast(r: RecordReceipt): string {
  const t = r.trade;
  const unit = UNIT[t.kind];
  const sideText = t.side === "buy" ? "买入" : "卖出";
  const rp = t.realized != null ? ` · 已实现 ${t.realized >= 0 ? "+" : ""}${fmt(t.realized, 2)}` : "";
  return `已记入：${sideText} ${t.name} ${fmt(t.qty, 0)} ${unit} @ ${fmt(t.price, priceDecimals(t.kind))}${rp}${t.note ? " · " + t.note : ""}`;
}
