/**
 * 领域层公共常量与工具（镜像 `backend/crates/zimu-domain/src/lib.rs`）。
 * 口径唯一来源：`docs/03-domain-rules.md`（正文 + 附录「黄金测试向量」）。
 */

/** 年化交易日常数（03 §7 / §8） */
export const TRADING_DAYS = 244;

/** 夏普分母 ε 守卫（03 §7 公式内常数） */
export const SHARPE_EPS = 1e-9;

/** 阈值比较容差：|value − threshold| ≤ EPS 视为恰好压线（含边界语义，03 附录约定 4 / 比对规则第 5 条） */
export const THRESHOLD_EPS = 1e-9;

/** 标的类型（03 §1；枚举值与 fixture / Rust serde snake_case 一致） */
export type Kind = "stock" | "etf" | "cb";

/** 交易方向 */
export type Side = "buy" | "sell";

/** 分母为 0 → 比率取 0（03 §5 除零防护；JS 中 -0 === 0 成立，负零一并覆盖） */
export function ratio(num: number, den: number): number {
  return den === 0 ? 0 : num / den;
}
