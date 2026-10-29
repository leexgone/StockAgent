/**
 * 预警判定（03 §12 生产口径，P4-T7）。
 * 镜像 `backend/crates/zimu-domain/src/alert.rs`——模块划分、字段名、判定顺序一致。
 *
 * 生产端判定由服务端调度器执行并落 `alerts` 表，前端消费留档展示；本镜像保证
 * 口径单一来源（黄金向量 `fixtures/golden/alerts.json` 两边跑同一份）。
 *
 * 口径要点（照抄 03 §12）：
 * - 纯函数无状态：事实 → 触发的预警列表（按判定序 1..8，同标的多条全部产出）；
 * - 数据缺失 → 对应序不参与判定，不伪造；price ≤ 0 → 整行跳过；
 * - level 词汇（§11）：up/down 只用于价格方向，warn 要动作类，不产出 flat；
 * - 同日 (code,type) 去重是调度器职责（需要库状态），不在本层；
 * - text 是展示层模板，不进 fixture。
 */

import { THRESHOLD_EPS, type Kind } from "./common";
import { priceDecimals } from "./lot";

/** 放量倍数：当日累计量 ≥ 2.0 × 近 20 日均量（03 §12 常量表） */
export const VOL_SURGE_MULT = 2.0;
/** 均量最少有效样本（null / ≤0 不计入）；不足 → 放量不参与判定 */
export const VOL_SURGE_MIN_SAMPLES = 5;
/** 转债期限窗口（自然日）：0 ≤ last_trade_date − today ≤ 5 */
export const CB_DEADLINE_DAYS = 5;

// type 封闭词汇（03 §12 判定表）
export const T_LIMIT_UP = "触及涨停";
export const T_LIMIT_DOWN = "触及跌停";
export const T_BELOW_COST = "跌破成本";
export const T_STOP_LOSS = "触及止损线";
export const T_TAKE_PROFIT = "触及止盈线";
export const T_VOL_SURGE = "放量";
export const T_CB_REDEEM = "强赎公告";
export const T_CB_DEADLINE = "临近到期";

// level 词汇（§11）
export const L_UP = "up";
export const L_DOWN = "down";
export const L_WARN = "warn";

/** 单标的判定输入（键与 fixture `alerts.json` 一一对应，snake_case） */
export type AlertItem = {
  kind: Kind;
  /** 现价；≤0 → 整行跳过不伪造 */
  price: number;
  /** 昨收（放量方向判定用）；缺失 → 放量 level 降为 warn */
  prev: number | null;
  /** 当日涨停价（调用方按 §2 拼装；null → 序 1 不参与） */
  limit_up: number | null;
  /** 当日跌停价（同上） */
  limit_down: number | null;
  /** 持仓摊薄成本（§4）；null / ≤0 → 序 3 不参与 */
  cost: number | null;
  /** 最近一条建议留档的止损价；null → 序 4 不参与 */
  stop: number | null;
  /** 最近一条建议留档的目标区间上沿；null → 序 5 不参与 */
  target_hi: number | null;
  /** 当日累计量（与日线 volume 同单位） */
  volume_today: number | null;
  /** 近 20 根日线量（不含今日，升序；null 不计入样本） */
  vol_hist: (number | null)[];
  /** 转债强赎状态（集思录；源降级 → null，序 7 不参与） */
  redeem_status: string | null;
  /** 转债最后交易日 YYYY-MM-DD（序 8 首选） */
  last_trade_date: string | null;
  /** 转债到期日（last_trade_date 缺失时回退） */
  maturity_date: string | null;
};

export type AlertEvalInput = {
  /** 上海日期 YYYY-MM-DD（转债期限的自然日差基准） */
  today: string;
  item: AlertItem;
};

/** 触发的预警（type/level 进 fixture；text 展示层模板不校验） */
export type AlertDraft = { type: string; level: string; text: string };

export type AlertEvalOutput = {
  /** 按 §12 判定序排列；空数组 = 本轮无预警 */
  alerts: AlertDraft[];
};

function fmtPrice(v: number, decimals: number): string {
  return v.toFixed(decimals);
}

/** 带符号两位小数百分比（Rust `{:+.2}` 同款） */
function signed2(v: number): string {
  return `${v >= 0 ? "+" : ""}${v.toFixed(2)}`;
}

/**
 * ISO 日期（YYYY-MM-DD）→ civil 天数（Date.UTC 天序号）。
 * 与 Rust 侧 parse_iso_date 同守卫：月份 1–12、日 1–31，越界/非法 → null；
 * 数值合法但日历溢出（如 02-30）两边同按规范化处理（Date.UTC ≡ days_from_civil）。
 */
function parseIsoDate(s: string): number | null {
  const parts = s.split("-");
  if (parts.length !== 3) return null;
  const y = Number(parts[0]);
  const m = Number(parts[1]);
  const d = Number(parts[2]);
  if (!Number.isInteger(y) || !Number.isInteger(m) || !Number.isInteger(d)) return null;
  if (m < 1 || m > 12 || d < 1 || d > 31) return null;
  return Date.UTC(y, m - 1, d) / 86400000;
}

/** to − from 的自然日差；任一解析失败 → null */
function isoDateDiff(from: string, to: string): number | null {
  const a = parseIsoDate(from);
  const b = parseIsoDate(to);
  if (a === null || b === null) return null;
  return b - a;
}

/** 预警判定（03 §12）。纯函数：去重/落库归调度器。 */
export function evalAlerts(inp: AlertEvalInput): AlertEvalOutput {
  const it = inp.item;
  const alerts: AlertDraft[] = [];
  // 无有效价整行跳过（不伪造）
  if (!(it.price > 0)) return { alerts };
  const d = priceDecimals(it.kind);
  const p = fmtPrice(it.price, d);
  const push = (type: string, level: string, text: string) => alerts.push({ type, level, text });

  // 序 1/2：涨跌停（含边界）
  if (it.limit_up !== null && it.price >= it.limit_up - THRESHOLD_EPS) {
    push(T_LIMIT_UP, L_UP, `现价 ${p} 已触及涨停价 ${fmtPrice(it.limit_up, d)}`);
  }
  if (it.limit_down !== null && it.price <= it.limit_down + THRESHOLD_EPS) {
    push(T_LIMIT_DOWN, L_DOWN, `现价 ${p} 已触及跌停价 ${fmtPrice(it.limit_down, d)}`);
  }

  // 序 3：跌破成本（相等不算——压线算安全）
  if (it.cost !== null && it.cost > 0 && it.price < it.cost - THRESHOLD_EPS) {
    const pnlPct = ((it.price - it.cost) / it.cost) * 100;
    push(
      T_BELOW_COST,
      L_DOWN,
      `现价 ${p} 低于成本 ${fmtPrice(it.cost, d)}，浮亏 ${signed2(pnlPct)}%`,
    );
  }

  // 序 4/5：最近留档的止损/止盈线（含边界）
  if (it.stop !== null && it.price <= it.stop + THRESHOLD_EPS) {
    push(T_STOP_LOSS, L_DOWN, `现价 ${p} 已触及最近建议的止损线 ${fmtPrice(it.stop, d)}`);
  }
  if (it.target_hi !== null && it.price >= it.target_hi - THRESHOLD_EPS) {
    push(T_TAKE_PROFIT, L_WARN, `现价 ${p} 达到最近建议的止盈线 ${fmtPrice(it.target_hi, d)}`);
  }

  // 序 6：放量（有效样本 <5 不参与）
  const valid = it.vol_hist.filter((v): v is number => v !== null && v > 0);
  if (valid.length >= VOL_SURGE_MIN_SAMPLES && it.volume_today !== null && it.volume_today > 0) {
    const avg = valid.reduce((a, b) => a + b, 0) / valid.length;
    if (avg > 0) {
      const r = it.volume_today / avg;
      if (r >= VOL_SURGE_MULT - THRESHOLD_EPS) {
        // 方向 = (price−prev)/prev；prev 缺失/≤0 → warn
        let level = L_WARN;
        let dir = "，昨收缺失、方向不可判";
        if (it.prev !== null && it.prev > 0) {
          const chg = ((it.price - it.prev) / it.prev) * 100;
          level = chg >= -THRESHOLD_EPS ? L_UP : L_DOWN;
          dir = `，涨跌幅 ${signed2(chg)}%`;
        }
        push(
          T_VOL_SURGE,
          level,
          `当日累计量 ${Math.round(it.volume_today)} 为近 20 日均量 ${Math.round(avg)} 的 ${r.toFixed(1)} 倍${dir}`,
        );
      }
    }
  }

  // 序 7/8：转债强赎与期限（kind 门槛）
  if (it.kind === "cb") {
    const rs = it.redeem_status;
    // 封闭判据：含「强赎」且不以「不」开头（「不强赎」不触发）
    if (rs !== null && rs.length > 0 && rs.includes("强赎") && !rs.startsWith("不")) {
      const ltd = it.last_trade_date;
      const tail = ltd !== null && ltd.length > 0 ? `，最后交易日 ${ltd}` : "";
      push(T_CB_REDEEM, L_WARN, `正股触发有条件赎回（${rs}）${tail}`);
    }
    let label: string | null = null;
    let ds: string | null = null;
    if (it.last_trade_date !== null && it.last_trade_date.length > 0) {
      label = "最后交易日";
      ds = it.last_trade_date;
    } else if (it.maturity_date !== null && it.maturity_date.length > 0) {
      label = "到期日";
      ds = it.maturity_date;
    }
    if (label !== null && ds !== null) {
      const days = isoDateDiff(inp.today, ds);
      // 已过期（差 <0）不参与：数据异常，行情侧自会以停牌/退市跳过
      if (days !== null && days >= 0 && days <= CB_DEADLINE_DAYS) {
        push(T_CB_DEADLINE, L_WARN, `距${label} ${ds} 还有 ${days} 天`);
      }
    }
  }

  return { alerts };
}
