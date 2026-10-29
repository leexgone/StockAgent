/**
 * 市场时钟视图模型（P5-T2）。
 *
 * 生产口径在后端（03 §9，`GET /api/market/status` → MarketClock）：状态文案
 * `state_text` 是前端展示唯一来源；倒计时按 `next_at` 由本地每秒走针
 * （契约注释：「API 只给事实，倒计时文案由前端走针」）。
 * 后端不可用时退化为本地启发式 `marketClock()`（与原型 data.js 同款，
 * 工作日 + 时段判断，不含节假日）——只作降级展示，不作任何交易判定。
 */

import type { components } from "../api/schema";

export type MarketClock = components["schemas"]["MarketClock"];

export interface Clock {
  state: string;
  dot: string;
  next: string;
  time: string;
  live: boolean;
}

/** next_event（机器可读）→ 倒计时文案里的中文事件名 */
const NEXT_EVENT_LABEL: Record<string, string> = {
  auction: "集合竞价",
  open: "开盘",
  lunch: "午间休市",
  pm_open: "下午开盘",
  close: "收盘",
};

function cd(m: number): string {
  const mm = Math.max(0, Math.round(m));
  const h = Math.floor(mm / 60);
  return (h ? h + " 小时 " : "") + (mm % 60) + " 分";
}

/** 后端时钟（03 §9）→ 展示视图模型；`now` 每秒变化驱动倒计时走针 */
export function clockView(c: MarketClock, now: Date): Clock {
  const time = now.toTimeString().slice(0, 8);
  let next = "";
  if (c.next_at) {
    const t = new Date(c.next_at).getTime();
    if (Number.isFinite(t)) {
      const mins = (t - now.getTime()) / 60000;
      const label = NEXT_EVENT_LABEL[c.next_event] ?? c.next_event;
      next = mins > 0 ? `距${label} ${cd(mins)}` : `${label}已过`;
    }
  } else if (c.state === "non_trading_day" && c.next_trade_day) {
    next = `下一交易日 ${c.next_trade_day}`;
  }
  return { state: c.state_text, dot: c.dot, next, time, live: c.live };
}

/** 本地启发式时钟（后端不可用时的降级展示；与原型 data.js marketClock 相同） */
export function marketClock(now: Date): Clock {
  const dow = now.getDay();
  const mins = now.getHours() * 60 + now.getMinutes();
  const hhmm = now.toTimeString().slice(0, 8);
  const isTrade = dow >= 1 && dow <= 5;
  if (!isTrade) {
    const next = new Date(now);
    next.setDate(next.getDate() + (dow === 6 ? 2 : 1));
    return { state: "休市", dot: "closed", next: "下一交易日 " + (next.getMonth() + 1) + "/" + next.getDate() + " 09:15 集合竞价", time: hhmm, live: false };
  }
  const at = (h: number, m: number) => h * 60 + m;
  if (mins < at(9, 15)) return { state: "盘前", dot: "closed", next: "距集合竞价 " + cd(at(9, 15) - mins), time: hhmm, live: false };
  if (mins < at(9, 30)) return { state: "集合竞价", dot: "live", next: "距开盘 " + cd(at(9, 30) - mins), time: hhmm, live: true };
  if (mins < at(11, 30)) return { state: "交易中 · 早盘", dot: "live", next: "距午间休市 " + cd(at(11, 30) - mins), time: hhmm, live: true };
  if (mins < at(13, 0)) return { state: "午间休市", dot: "lunch", next: "距下午开盘 " + cd(at(13, 0) - mins), time: hhmm, live: false };
  if (mins < at(15, 0)) return { state: "交易中 · 午盘", dot: "live", next: "距收盘 " + cd(at(15, 0) - mins), time: hhmm, live: true };
  if (mins < at(15, 30)) return { state: "盘后定价交易", dot: "lunch", next: "复盘时段 · 次日 09:15 集合竞价", time: hhmm, live: false };
  return { state: "已收盘", dot: "closed", next: "距次日集合竞价 " + cd(at(9, 15) + 24 * 60 - mins), time: hhmm, live: false };
}
