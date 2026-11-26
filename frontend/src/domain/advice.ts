/**
 * 分析结论打分（03 §10 生产口径，P4-T6；ADR-0007：数字归规则，文字归模型）。
 * 镜像 `backend/crates/zimu-domain/src/advice.rs`——模块划分、字段名、运算顺序一致。
 *
 * 生产端打分由服务端生成并留档（advice_history），前端消费留档展示；本镜像保证
 * 口径单一来源（黄金向量 `fixtures/golden/advice.json` 两边跑同一份），P5 analysis
 * 屏如需本地即时重算直接可用。
 *
 * 口径要点（照抄 03 §10 生产口径）：
 * - 信号票 v1..v4（技术）+ vv（量能），数据不足的票不参与并如实记降级；
 * - 无数据源维度（基本面/消息面）与量能缺失/平量的资金面记中性值 NEUTRAL_DIM = 60；
 * - score = (2t + m + f + n) / 5，阈值 70/55/45 三带；
 * - conf = 0.5 + |Σ票| / (2n票)，可用票 < 3 → 0.5（样本内信号一致性，不是胜率）；
 * - 得分定 action；纪律只附参考（ADR-0023）：四条仓位纪律不拦截、不改写 action——
 *   越线事实以 DisciplineRef 附注随建议产出（仅偏多带、仅未过项、§6 表序），
 *   15% 目标带与 25% 上限锚仍是 suggested 数字来源（sizing 属仓位目标，不属纪律）；
 * - target/stop 锚 = 现价，half-up：floor(v × 10^d + 0.5) / 10^d（与 Rust 同表达式）。
 */

import { THRESHOLD_EPS, type Kind } from "./common";
import {
  LIMIT_CASH_RESERVE,
  LIMIT_CB_TOTAL,
  LIMIT_SECTOR,
  LIMIT_SINGLE_STOCK,
} from "./discipline";
import { priceDecimals } from "./lot";

/** 无数据源/无信号维度的中性分（中性带 55–69 的中值，03 §10 生产口径） */
export const NEUTRAL_DIM = 60;
/** 偏多时的单票目标仓位 %（加仓/买入的建议仓位） */
export const TARGET_WEIGHT = 15;
/** score 带阈值：≥70 偏多 / ≥55 中性 / ≥45 减仓 / <45 卖出（03 §10） */
export const SCORE_BULL = 70;
export const SCORE_NEUTRAL = 55;
export const SCORE_TRIM = 45;
/** 每张技术票对维度分的步长：t = 50 + 12.5 × Σ票 */
const VOTE_STEP = 12.5;
/** 20 日动量票的 ±5% 带 */
const MOM_BAND = 5;
/** 30 日区间位置票：> 0.9 追高保护 −1，< 0.2 低位 +1 */
const POS_HIGH = 0.9;
const POS_LOW = 0.2;
/** 量比放量阈值（近5日均量/近30日均量） */
const VOL_SURGE = 1.2;
/** 可用票少于 3 张时 conf 归 0.5（信号样本不足） */
const MIN_CONF_VOTES = 3;
/** 区间位置窗口（近 30 根收盘） */
const RANGE_WINDOW = 30;

/** 结论标签（03 §10 action 枚举；中文标签是展示层） */
export type Action = "add" | "buy" | "hold" | "trim" | "sell" | "dca";
/** 持有周期（short 短线 / short_mid 短中线 2–6 周 / mid 中线 1–3 月 / long 长线 6 月+） */
export type Horizon = "short" | "short_mid" | "mid" | "long";
/** 建议仓位模式 */
export type PositionMode = "target" | "maintain" | "dca" | "exit" | "flat";

export type AdviceInput = {
  kind: Kind;
  /** 升序日收盘（末位最新） */
  closes: number[];
  /** 与 closes 同长的日成交量；null = 该 bar 缺量 */
  volumes: (number | null)[];
  /** 现价（target/stop 锚；≤0 视为缺失 → 双 null） */
  price: number;
  held: boolean;
  /** 占总资产 %（recalc weight；未持仓 = 0） */
  weight: number;
  /** 纪律参考上下文（ADR-0023：只作参考判定，不参与 action 决策；缺省 = 只出 single_stock ref） */
  discipline_ctx?: DisciplineCtx;
};

/** 纪律参考上下文（百分数值，与 §6 同口径；缺项的规则不参与参考） */
export type DisciplineCtx = {
  /** 现金占总资产 %（cash_reserve.value） */
  cash_pct?: number | null;
  /** 该标的所在行业占市值 %（分母 total_mv，与 §6 sector 同口径，非组合最大行业） */
  sector_pct?: number | null;
  /** 转债合计占总资产 %（cb_total.value，仅 kind=cb 参与） */
  cb_total_pct?: number | null;
};

/** 纪律参考项（ADR-0023）：仅未过项入列、仅偏多带生成、固定序 = §6 表序。
 * rule/value/threshold 进 fixture 比对；label/text 展示层事实模板不校验（dims[].t 同待遇）。 */
export type DisciplineRef = {
  /** 规则键（封闭词汇）："single_stock" | "sector" | "cb_total" | "cash_reserve" */
  rule: string;
  /** 中文规则名（展示层） */
  label: string;
  /** 当前值 % */
  value: number;
  /** 参考线 %（LIMIT_* 常量） */
  threshold: number;
  /** 一句话提示（展示层模板，数字内嵌） */
  text: string;
};

export type AdviceDim = { k: string; s: number; t: string };
export type PositionAdvice = {
  current: number;
  /** maintain/dca 为 null（维持/定投无单点目标） */
  suggested: number | null;
  mode: PositionMode;
};

/** 中间量全量透出：文案里引用的每个数字都能在这里回溯 */
export type AdviceStats = {
  n_bars: number;
  ma5: number | null;
  ma20: number | null;
  mom5: number | null;
  mom20: number | null;
  pos30: number | null;
  high30: number | null;
  low30: number | null;
  vol_ratio: number | null;
  vote_sum: number;
  vote_count: number;
};

export type AdviceOutput = {
  action: Action;
  horizon: Horizon;
  score: number;
  /** 样本内信号一致性 0–1（不是胜率） */
  conf: number;
  dims: AdviceDim[];
  target_lo: number | null;
  target_hi: number | null;
  stop: number | null;
  position: PositionAdvice;
  /** 纪律参考（仅偏多带非空；序 = §6 表序，ADR-0023） */
  discipline_refs: DisciplineRef[];
  stats: AdviceStats;
};

/** half-up 舍入到 d 位小数：floor(v × 10^d + 0.5) / 10^d（与 Rust 镜像同表达式） */
function roundDec(v: number, d: number): number {
  const scale = 10 ** d;
  return Math.floor(v * scale + 0.5) / scale;
}

/** 带 ε 守卫的符号（恰好相等 → 0，防浮点噪声翻票） */
function signEps(x: number): number {
  if (x > THRESHOLD_EPS) return 1;
  if (x < -THRESHOLD_EPS) return -1;
  return 0;
}

function mean(v: number[]): number {
  return v.reduce((a, b) => a + b, 0) / v.length;
}

/** 近 min(n,k) 根均量：窗口内任一 bar 缺量（null / 越界 / ≤0）→ null（不伪造量能信号） */
function volMean(volumes: (number | null)[], n: number, k: number): number | null {
  const w = Math.min(k, n);
  if (w < 1 || volumes.length < n) return null;
  let sum = 0;
  for (let i = n - w; i < n; i++) {
    const x = volumes[i];
    if (x === null || x === undefined || !(x > 0)) return null;
    sum += x;
  }
  return sum / w;
}

/** 打分主函数（03 §10 生产口径；黄金向量 fixtures/golden/advice.json） */
export function scoreAdvice(inp: AdviceInput): AdviceOutput {
  const n = inp.closes.length;
  const last: number | null = n >= 1 ? inp.closes[n - 1] : null;

  // ---- 中间量（每个都进 stats 供回溯）----
  const ma5 = n >= 5 ? mean(inp.closes.slice(n - 5)) : null;
  const ma20 = n >= 20 ? mean(inp.closes.slice(n - 20)) : null;
  const momOf = (back: number): number | null => {
    if (n < back + 1 || last === null) return null;
    const base = inp.closes[n - back - 1];
    if (!(base > 0)) return null;
    return (last / base - 1) * 100;
  };
  const mom5 = momOf(5);
  const mom20 = momOf(20);

  let high30: number | null = null;
  let low30: number | null = null;
  let pos30: number | null = null;
  if (n >= 1) {
    const w = inp.closes.slice(Math.max(0, n - RANGE_WINDOW));
    high30 = Math.max(...w);
    low30 = Math.min(...w);
    // v4 最少样本 5 根（不足 → 票不参与，pos30 也不报——短窗口的「区间位置」是噪声）
    if (n >= 5) {
      pos30 = high30 > low30 ? ((last ?? low30) - low30) / (high30 - low30) : 0.5;
    }
  }

  // ---- 信号票 ----
  const techVotes: number[] = [];
  if (ma5 !== null && ma20 !== null) techVotes.push(signEps(ma5 - ma20)); // v1 均线
  if (last !== null && ma20 !== null) techVotes.push(signEps(last - ma20)); // v2 价位
  if (mom20 !== null) {
    // v3 动量：±5% 带外才计票
    techVotes.push(
      mom20 > MOM_BAND + THRESHOLD_EPS ? 1 : mom20 < -(MOM_BAND + THRESHOLD_EPS) ? -1 : 0,
    );
  }
  if (pos30 !== null) {
    // v4 区间位置：高位追高保护 / 低位关注
    techVotes.push(pos30 > POS_HIGH + THRESHOLD_EPS ? -1 : pos30 < POS_LOW - THRESHOLD_EPS ? 1 : 0);
  }
  const techSum = techVotes.reduce((a, b) => a + b, 0);
  const t = Math.min(100, Math.max(0, 50 + VOTE_STEP * techSum));

  // 量能票：vol_ratio 与 mom5 都可算才参与（缺失 → 中性 60，不投票）
  const vol5 = volMean(inp.volumes, n, 5);
  const vol30 = volMean(inp.volumes, n, RANGE_WINDOW);
  const volRatio = vol5 !== null && vol30 !== null && vol30 > 0 ? vol5 / vol30 : null;
  let money = NEUTRAL_DIM;
  let moneyVote: number | null = null;
  if (volRatio !== null && mom5 !== null) {
    if (volRatio >= VOL_SURGE - THRESHOLD_EPS) {
      if (mom5 > THRESHOLD_EPS) {
        money = 70;
        moneyVote = 1; // 放量上涨：资金流入迹象
      } else if (mom5 < -THRESHOLD_EPS) {
        money = 30;
        moneyVote = -1; // 放量下跌：资金流出迹象
      } else {
        moneyVote = 0;
      }
    } else {
      moneyVote = 0; // 平量/缩量：无方向信号
    }
  }

  const voteSum = techSum + (moneyVote ?? 0);
  const voteCount = techVotes.length + (moneyVote !== null ? 1 : 0);
  const conf =
    voteCount < MIN_CONF_VOTES ? 0.5 : 0.5 + Math.abs(voteSum) / (2 * voteCount);

  // 基本面/消息面：无数据源 → 中性（接入后先改 03 §10 与两边实现）
  const score = (2 * t + money + NEUTRAL_DIM + NEUTRAL_DIM) / 5;

  // ---- action 决策表（得分定 action；纪律只附参考，ADR-0023，03 §10 生产口径）----
  const w = inp.weight;
  // overCap 只决定 suggested 锚取 25 还是 15，不再改写 action（ADR-0023）
  const overCap = inp.held && w > LIMIT_SINGLE_STOCK + THRESHOLD_EPS;
  const maintain: PositionAdvice = { current: w, suggested: null, mode: "maintain" };
  const flat: PositionAdvice = { current: w, suggested: 0, mode: "flat" };
  const toTarget = (target: number): PositionAdvice => ({
    current: w,
    suggested: target,
    mode: "target",
  });

  let action: Action;
  let position: PositionAdvice;
  if (score >= SCORE_BULL - THRESHOLD_EPS) {
    if (!inp.held) {
      action = "buy";
      position = toTarget(TARGET_WEIGHT);
    } else if (overCap) {
      // 超上限偏多票：action 仍为加仓，suggested 锚上限 25%（上限锚读法「建议 ≤25%」，
      // 非降仓指令——减仓股数归 calc_discipline.reduce_hint 本家，ADR-0023 D2）
      action = "add";
      position = toTarget(LIMIT_SINGLE_STOCK);
    } else if (w < TARGET_WEIGHT - THRESHOLD_EPS) {
      action = "add";
      position = toTarget(TARGET_WEIGHT);
    } else {
      action = "hold";
      position = maintain; // 仓位已达目标带（sizing 锚，非纪律），不再加
    }
  } else if (score >= SCORE_NEUTRAL - THRESHOLD_EPS) {
    if (inp.kind === "etf") {
      action = "dca";
      position = { current: w, suggested: null, mode: "dca" };
    } else if (inp.held) {
      action = "hold";
      position = maintain;
    } else {
      action = "hold";
      position = flat;
    }
  } else if (score >= SCORE_TRIM - THRESHOLD_EPS) {
    if (inp.held) {
      action = "trim";
      position = toTarget(roundDec(w / 2, 1)); // 减半，1 位小数
    } else {
      action = "hold";
      position = flat;
    }
  } else if (inp.held) {
    action = "sell";
    position = { current: w, suggested: 0, mode: "exit" };
  } else {
    action = "hold";
    position = flat;
  }

  const horizon: Horizon =
    action === "add" || action === "buy"
      ? "mid"
      : action === "dca"
        ? "long"
        : action === "hold"
          ? inp.kind === "etf"
            ? "long"
            : "mid"
          : action === "trim"
            ? "short_mid"
            : "short";

  // ---- target / stop（锚 = 现价；转债无区间目标、止损 0.95；定投双 null）----
  const d = priceDecimals(inp.kind);
  const priced = inp.price > 0;
  const stopFactor = inp.kind === "cb" ? 0.95 : 0.92;
  let targetLo: number | null = null;
  let targetHi: number | null = null;
  if ((action === "add" || action === "buy") && priced && inp.kind !== "cb") {
    targetLo = roundDec(inp.price * 1.08, d);
    targetHi = roundDec(inp.price * 1.15, d);
  }
  let stop: number | null = null;
  if ((action === "add" || action === "buy") && priced) {
    // 买入/加仓必须有止损（03 §10）
    stop = roundDec(inp.price * stopFactor, d);
  } else if (
    (action === "hold" || action === "trim" || action === "sell") &&
    priced &&
    inp.held
  ) {
    stop = roundDec(inp.price * stopFactor, d); // 持有/减仓/卖出的防线
  }

  const dims: AdviceDim[] = [
    { k: "技术面", s: t, t: techObservation(n, ma5, ma20, mom20, pos30, t) },
    { k: "基本面", s: NEUTRAL_DIM, t: "未接入基本面数据源，按中性处理" },
    { k: "资金面", s: money, t: moneyObservation(volRatio, mom5, moneyVote) },
    { k: "消息面", s: NEUTRAL_DIM, t: "未接入消息面数据源，按中性处理" },
  ];

  // ---- 纪律参考（ADR-0023：仅偏多带生成、仅未过项入列、序 = §6 表序）----
  const disciplineRefs: DisciplineRef[] =
    score >= SCORE_BULL - THRESHOLD_EPS ? buildDisciplineRefs(inp, w) : [];

  return {
    action,
    horizon,
    score,
    conf,
    dims,
    target_lo: targetLo,
    target_hi: targetHi,
    stop,
    position,
    discipline_refs: disciplineRefs,
    stats: {
      n_bars: n,
      ma5,
      ma20,
      mom5,
      mom20,
      pos30,
      high30,
      low30,
      vol_ratio: volRatio,
      vote_sum: voteSum,
      vote_count: voteCount,
    },
  };
}

/** 纪律参考构建（ADR-0023）：仅未过项入列（压线 = 过，THRESHOLD_EPS 复用），
 * 固定序 = §6 表序 single_stock → sector → cb_total → cash_reserve；
 * text 展示层事实模板（不进 fixture，dims[].t 同待遇），基调「参考线 / 拍板在你」。 */
function buildDisciplineRefs(inp: AdviceInput, w: number): DisciplineRef[] {
  const refs: DisciplineRef[] = [];
  if (inp.held && w > LIMIT_SINGLE_STOCK + THRESHOLD_EPS) {
    refs.push({
      rule: "single_stock",
      label: "单票仓位参考线",
      value: w,
      threshold: LIMIT_SINGLE_STOCK,
      text: `当前 ${w.toFixed(1)}% 已越 25% 单票参考线：建议锚为上限，是否执行由你拍板`,
    });
  }
  const ctx = inp.discipline_ctx;
  if (ctx) {
    const sector = ctx.sector_pct;
    if (sector !== null && sector !== undefined && sector > LIMIT_SECTOR + THRESHOLD_EPS) {
      refs.push({
        rule: "sector",
        label: "行业仓位参考线",
        value: sector,
        threshold: LIMIT_SECTOR,
        text: `所在行业占市值 ${sector.toFixed(1)}% 已越 35% 行业参考线：注意集中度风险，是否执行由你拍板`,
      });
    }
    const cb = ctx.cb_total_pct;
    if (inp.kind === "cb" && cb !== null && cb !== undefined && cb > LIMIT_CB_TOTAL + THRESHOLD_EPS) {
      refs.push({
        rule: "cb_total",
        label: "转债合计参考线",
        value: cb,
        threshold: LIMIT_CB_TOTAL,
        text: `组合转债合计 ${cb.toFixed(1)}% 已越 12% 转债参考线：仅供参考，是否执行由你拍板`,
      });
    }
    const cash = ctx.cash_pct;
    if (cash !== null && cash !== undefined && cash < LIMIT_CASH_RESERVE - THRESHOLD_EPS) {
      refs.push({
        rule: "cash_reserve",
        label: "现金保留参考线",
        value: cash,
        threshold: LIMIT_CASH_RESERVE,
        text: `现金仅占 ${cash.toFixed(1)}%，低于 10% 现金参考线：仅供参考，是否执行由你拍板`,
      });
    }
  }
  return refs;
}

/** 技术面观察：有什么数据说什么话，缺失如实标注（降级不伪装） */
function techObservation(
  n: number,
  ma5: number | null,
  ma20: number | null,
  mom20: number | null,
  pos30: number | null,
  t: number,
): string {
  if (n < 5) return `日线仅 ${n} 根（<5），技术信号样本不足，按中性处理`;
  const parts: string[] = [];
  if (ma5 !== null && ma20 !== null) {
    const rel = ma5 > ma20 ? "上方" : ma5 < ma20 ? "下方" : "持平";
    parts.push(`MA5 在 MA20 ${rel}`);
  } else {
    parts.push("MA20 样本不足（<20 根）");
  }
  parts.push(mom20 !== null ? `20 日动量 ${signed1(mom20)}%` : "20 日动量样本不足（<21 根）");
  if (pos30 !== null) parts.push(`30 日区间位置 ${(pos30 * 100).toFixed(0)}%`);
  parts.push(`技术面得分 ${t.toFixed(1)}`);
  return parts.join("；");
}

/** 资金面观察（量能是资金面的代理指标，如实说明） */
function moneyObservation(
  volRatio: number | null,
  mom5: number | null,
  vote: number | null,
): string {
  if (volRatio === null || mom5 === null || vote === null) {
    return "量能数据缺失，按中性处理（资金面以量能为代理指标）";
  }
  const vr = volRatio.toFixed(2);
  const m5 = signed1(mom5);
  let body: string;
  if (vote === 1) body = `近 5 日放量（量比 ${vr}）且 5 日动量 ${m5}%，资金流入迹象`;
  else if (vote === -1) body = `近 5 日放量（量比 ${vr}）但 5 日动量 ${m5}%，资金流出迹象`;
  else if (volRatio < 0.8) body = `近 5 日缩量（量比 ${vr}），观望情绪浓`;
  else body = `量能平稳（量比 ${vr}），无明确方向信号`;
  return `${body}；资金面以量能为代理指标，非真实资金流数据`;
}

/** Rust `{:+.1}` 同款：正数带 +、负数带 −、一位小数 */
function signed1(v: number): string {
  return `${v >= 0 ? "+" : ""}${v.toFixed(1)}`;
}
