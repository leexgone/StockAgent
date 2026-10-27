/**
 * 摊薄成本、已实现盈亏与调整后仓位（03 §4）：抽屉试算的纯函数形态。
 * 镜像 `backend/crates/zimu-domain/src/dilution.rs`，运算顺序照抄保证浮点逐位一致。
 */

import { THRESHOLD_EPS, type Kind, type Side } from "./common";
import { fee } from "./fees";

/** 单票占组合上限 %（03 §6 第一条；试算超限判定用它） */
export const SINGLE_STOCK_LIMIT = 25;

export type ApplyTradeInput = {
  kind: Kind;
  side: Side;
  qty: number;
  price: number;
  /** 当前持仓数量 */
  held: number;
  /** 当前摊薄成本（无持仓传 0） */
  cost: number;
  /** 当前现金 */
  cash: number;
  /** 其他持仓当前市值合计（原型取快照，不随本笔价格变化） */
  others_mv: number;
};

export type TradeOutcome = {
  amount: number;
  fee: number;
  after_shares: number;
  /** 清仓（after_shares = 0）时持仓行移除 → null（03 附录约定 3） */
  after_cost: number | null;
  cash_after: number;
  /** cash_after < 0（原型：数字标绿并在提交判定中拦截，不弹错误框） */
  insufficient_cash: boolean;
  /** 仅卖出且有成本基准时有值 */
  realized: number | null;
  /** realized / (cost × qty) × 100 */
  realized_pct: number | null;
  /** others_mv + after_shares×price + cash_after（分母随本笔交易变化） */
  ta_after: number;
  weight_after: number;
  /** weight_after > 25%（压线含边界，THRESHOLD_EPS 守卫） */
  over_limit: boolean;
};

/** 试算一笔交易（原型 `holdings.html:352`）。买入费用计入摊薄成本；卖出不改成本。 */
export function applyTrade(inp: ApplyTradeInput): TradeOutcome {
  const breakdown = fee({ kind: inp.kind, side: inp.side, qty: inp.qty, price: inp.price });
  const amount = breakdown.amount;
  const f = breakdown.total;

  const afterShares =
    inp.side === "buy" ? inp.held + inp.qty : Math.max(inp.held - inp.qty, 0);
  const cashAfter =
    inp.side === "buy" ? inp.cash - amount - f : inp.cash + amount - f;
  const insufficientCash = cashAfter < 0;

  // 买入费用计入成本；held=0 新建仓即 (amount+f)/qty；清仓 → null
  let afterCost: number | null;
  if (inp.side === "buy") {
    afterCost = afterShares > 0 ? (inp.cost * inp.held + amount + f) / afterShares : null;
  } else {
    afterCost = afterShares > 0 ? inp.cost : null;
  }

  let realized: number | null = null;
  let realizedPct: number | null = null;
  if (inp.side === "sell" && inp.cost > 0) {
    realized = (inp.price - inp.cost) * inp.qty - f;
    realizedPct = (realized / (inp.cost * inp.qty)) * 100;
  }

  const taAfter = inp.others_mv + afterShares * inp.price + cashAfter;
  const weightAfter = taAfter !== 0 ? ((afterShares * inp.price) / taAfter) * 100 : 0;
  const overLimit = weightAfter > SINGLE_STOCK_LIMIT + THRESHOLD_EPS;

  return {
    amount,
    fee: f,
    after_shares: afterShares,
    after_cost: afterCost,
    cash_after: cashAfter,
    insufficient_cash: insufficientCash,
    realized,
    realized_pct: realizedPct,
    ta_after: taAfter,
    weight_after: weightAfter,
    over_limit: overLimit,
  };
}
