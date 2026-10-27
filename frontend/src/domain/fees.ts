/**
 * 费用估算（03 §3）：佣金（万 2.5、最低 5 元）、印花税（卖出且股票）、过户费（股票，沪深均收）。
 * 输出保留原始精度不做分位舍入（03 附录约定 5）；界面必须始终带「实际以券商结算为准」。
 * 镜像 `backend/crates/zimu-domain/src/fees.rs`。
 */

import type { Kind, Side } from "./common";

export const COMMISSION_RATE = 0.00025;
export const MIN_COMMISSION = 5;
export const STAMP_TAX_RATE = 0.0005;
export const TRANSFER_FEE_RATE = 0.00001;

export type FeeInput = { kind: Kind; side: Side; qty: number; price: number };

export type FeeBreakdown = {
  amount: number;
  commission: number;
  stamp_tax: number;
  transfer_fee: number;
  total: number;
};

/**
 * 费用估算（原型 `holdings.html:320`）。ETF 与转债免印花税、免过户费；
 * 转债「转股」的费用税务不同，范围外（03 §3 ⚠️ 留扩展位）。
 */
export function fee(input: FeeInput): FeeBreakdown {
  const amount = input.qty * input.price;
  const commission = Math.max(MIN_COMMISSION, amount * COMMISSION_RATE);
  const stampTax =
    input.side === "sell" && input.kind === "stock" ? amount * STAMP_TAX_RATE : 0;
  const transferFee = input.kind === "stock" ? amount * TRANSFER_FEE_RATE : 0;
  // 累加顺序与原型/Rust 一致：((佣金 + 印花) + 过户)
  const total = commission + stampTax + transferFee;
  return { amount, commission, stamp_tax: stampTax, transfer_fee: transferFee, total };
}
