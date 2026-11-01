import { useEffect, useState } from "react";
import { priceDecimals, type Kind } from "../domain";
import { fixed } from "../lib/format";
import { KIND_LABEL } from "../lib/kind";
import type { HoldingEditRequest } from "../store/portfolio";
import { usePortfolio } from "../store/portfolio";

/**
 * 持仓直接维护表单（ADR-0009 / 03 §13：快照校正——初始化映射 / 纠错 / 场外变动）。
 * 与 TradeDrawer 是两种心智：那边是**事件流**（试算 ≠ 下单，费用/已实现/现金联动），
 * 这边是**校正**（「看到什么抄什么」整行覆盖——不生成流水、不动现金、无试算预览）。
 * 校验口径 03 §13：6 位代码、SH/SZ、名称非空、shares ≥ 1 整数（**允许奇零股**，
 * lot 校验只约束委托路径）、cost ≥ 0；price/prev 是行情字段不可编辑（归刷新链）。
 * 后端仍是最终裁判（PUT /api/holdings/{code}，拒绝体 bad_input 就地展示）。
 */

const UNIT: Record<Kind, string> = { stock: "股", etf: "份", cb: "张" };

export interface HoldingFormInitial {
  code: string;
  name: string;
  mkt: string;
  kind: Kind;
  sector: string;
  shares: number;
  cost: number;
  limit_pct?: number | null;
}

interface Props {
  open: boolean;
  /** null = 新增（直接录入）；行对象 = 编辑（code/mkt 是身份字段，锁定不可改） */
  initial: HoldingFormInitial | null;
  onClose: () => void;
  /** 保存成功：调用方 toast + 重拉维护留痕 */
  onSaved: (msg: string) => void;
}

export default function HoldingFormDrawer({ open, initial, onClose, onSaved }: Props) {
  const isEdit = initial !== null;
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [mkt, setMkt] = useState("SH");
  const [kind, setKind] = useState<Kind>("stock");
  const [sector, setSector] = useState("");
  const [sharesStr, setSharesStr] = useState("");
  const [costStr, setCostStr] = useState("");
  const [limitStr, setLimitStr] = useState("");
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  // 打开时按 initial 灌入（编辑）或清空（新增）
  useEffect(() => {
    if (!open) return;
    setCode(initial?.code ?? "");
    setName(initial?.name ?? "");
    setMkt(initial?.mkt ?? "SH");
    setKind(initial?.kind ?? "stock");
    setSector(initial?.sector ?? "");
    setSharesStr(initial ? String(initial.shares) : "");
    setCostStr(initial ? fixed(initial.cost, priceDecimals(initial.kind)) : "");
    setLimitStr(initial?.limit_pct != null ? String(initial.limit_pct) : "");
    setNote("");
    setSubmitError(null);
    setSubmitting(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, initial]);

  // Escape 关闭（TradeDrawer 同款）
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  /* 客户端即时校验（镜像 03 §13；后端仍是最终裁判） */
  const shares = parseInt(sharesStr.replace(/\D/g, ""), 10);
  const cost = parseFloat(costStr);
  const limitPct = limitStr.trim() === "" ? null : Number(limitStr);
  const codeOk = isEdit || /^[0-9]{6}$/.test(code.trim());
  const nameOk = name.trim().length > 0;
  const sharesOk = Number.isInteger(shares) && shares >= 1;
  const costOk = Number.isFinite(cost) && cost >= 0;
  const limitOk = limitPct === null || (Number.isFinite(limitPct) && limitPct > 0);
  const canSubmit = codeOk && nameOk && sharesOk && costOk && limitOk && !submitting;

  const firstError = !codeOk
    ? "代码需为 6 位数字"
    : !nameOk
      ? "名称不能为空"
      : !sharesOk
        ? "数量需为 ≥ 1 的整数（允许奇零股）"
        : !costOk
          ? "成本需为 ≥ 0 的数字"
          : !limitOk
            ? "涨跌停 % 需为正数或留空"
            : null;

  const submit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setSubmitError(null);
    const body: HoldingEditRequest = {
      name: name.trim(),
      mkt,
      kind,
      sector: sector.trim(),
      shares,
      cost,
      // 留空 = 新建 NULL、编辑保留原值（03 §13 / §2 配置语义）
      ...(limitPct != null ? { limit_pct: limitPct } : {}),
      ...(note.trim() ? { note: note.trim() } : {}),
    };
    const ok = await usePortfolio.getState().upsertHolding(code.trim(), body);
    setSubmitting(false);
    if (ok) {
      onSaved(`${isEdit ? "已调整" : "已录入"}：${name.trim()} ${code.trim()} · ${shares} ${UNIT[kind]}（快照校正，未生成流水）`);
      onClose();
    } else {
      setSubmitError(usePortfolio.getState().error ?? "保存失败（后端不可用？）");
    }
  };

  if (!open) return null;
  const dec = priceDecimals(kind);

  return (
    <>
      <div className="scrim on" onClick={onClose} />
      <aside className="drawer on" role="dialog" aria-modal="true" aria-labelledby="hform-title" data-od-id="holding-form-drawer">
        <div className="drawer-head">
          <div style={{ flex: 1 }}>
            <div className="card-title" id="hform-title">{isEdit ? "编辑持仓（校正）" : "直接录入持仓"}</div>
            <div className="card-sub" id="hform-sub">
              快照校正（ADR-0009）：整行覆盖、不生成流水、不动现金——买卖请走加减仓抽屉
            </div>
          </div>
          <button className="icon-btn" aria-label="关闭" onClick={onClose}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>

        <div className="drawer-body">
          <div style={{ display: "grid", gridTemplateColumns: "1fr 96px", gap: 12 }}>
            <div className={`field${!codeOk ? " invalid" : ""}`} style={{ marginBottom: 0 }}>
              <label htmlFor="hf-code">代码（6 位数字）</label>
              <input className="input num" id="hf-code" inputMode="numeric" placeholder="例如 600519" maxLength={6}
                disabled={isEdit} value={code}
                onChange={(e) => setCode(e.target.value.replace(/\D/g, "").slice(0, 6))} />
            </div>
            <div className="field" style={{ marginBottom: 0 }}>
              <label htmlFor="hf-mkt">市场</label>
              {/* 全系统市场码口径 = SH/SZ（DB CHECK + 后端校验）；北交所不在当前范围 */}
              <select className="select" id="hf-mkt" disabled={isEdit} value={mkt} onChange={(e) => setMkt(e.target.value)}>
                <option value="SH">SH · 沪</option>
                <option value="SZ">SZ · 深</option>
              </select>
            </div>
            <div className="field" style={{ marginBottom: 0, gridColumn: "1 / -1" }}>
              <label htmlFor="hf-name">名称</label>
              <input className="input" id="hf-name" placeholder="例如 贵州茅台" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div className="field" style={{ marginBottom: 0 }}>
              <label htmlFor="hf-kind">类型</label>
              <select className="select" id="hf-kind" value={kind} onChange={(e) => setKind(e.target.value as Kind)}>
                {(Object.keys(KIND_LABEL) as Kind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
              </select>
            </div>
            <div className="field" style={{ marginBottom: 0 }}>
              <label htmlFor="hf-limit">涨跌停 %（可选）</label>
              <input className="input num" id="hf-limit" inputMode="decimal" placeholder={isEdit ? "留空保留原值" : "留空按 §2 推断"}
                value={limitStr} onChange={(e) => setLimitStr(e.target.value)} />
            </div>
            <div className="field" style={{ marginBottom: 0, gridColumn: "1 / -1" }}>
              <label htmlFor="hf-sector">行业（可选，纪律集中度用）</label>
              <input className="input" id="hf-sector" placeholder="例如 白酒" value={sector} onChange={(e) => setSector(e.target.value)} />
            </div>
            <div className={`field${sharesStr !== "" && !sharesOk ? " invalid" : ""}`} style={{ marginBottom: 0 }}>
              <label htmlFor="hf-shares">数量（{UNIT[kind]}）</label>
              <input className="input num" id="hf-shares" inputMode="numeric" placeholder="例如 150"
                value={sharesStr} onChange={(e) => setSharesStr(e.target.value)} />
            </div>
            <div className={`field${costStr !== "" && !costOk ? " invalid" : ""}`} style={{ marginBottom: 0 }}>
              <label htmlFor="hf-cost">摊薄成本（元）</label>
              <input className="input num" id="hf-cost" inputMode="decimal" placeholder={`例如 ${fixed(1400.5, dec)}`}
                value={costStr} onChange={(e) => setCostStr(e.target.value)} />
            </div>
          </div>

          <div className="hint" id="hf-hint" style={{ marginTop: 10 }}>
            数量允许奇零股（红利送股 / 转股后的零股真实存在，03 §13）；整手委托与费用试算走「记一笔加减仓」抽屉。
            成本按你券商 App 的摊薄成本口径抄录，调整成本不产生已实现盈亏。现价 / 昨收是行情字段，保存后由刷新链自动回写。
          </div>

          {firstError && (sharesStr !== "" || costStr !== "" || code !== "" || name !== "") && (
            <div className="err" style={{ display: "block", marginTop: 10 }}>{firstError}</div>
          )}
          {submitError && (
            <div className="err" style={{ display: "block", marginTop: 10 }}>保存被拒：{submitError}</div>
          )}

          <div className="field" style={{ marginTop: 14 }}>
            <label htmlFor="hf-note">维护备注（可选，写入维护记录）</label>
            <input className="input" id="hf-note" placeholder="例如：按券商对账单初始化" value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
        </div>

        <div className="drawer-foot">
          <button className="btn" style={{ flex: "none" }} onClick={onClose}>取消</button>
          <button className="btn btn-primary" style={{ flex: 1 }} disabled={!canSubmit} onClick={() => void submit()}>
            {submitting ? "保存中…" : isEdit ? "保存校正" : "录入持仓"}
          </button>
        </div>
      </aside>
    </>
  );
}
