//! 子木 · 领域纯函数库（P3-T3）。
//!
//! 口径唯一来源：`docs/03-domain-rules.md`（正文 + 附录「黄金测试向量」）。
//! 黄金向量：仓库根 `fixtures/golden/*.json`，由 `tests/golden.rs` runner 逐例断言；
//! 前端 TS 共享层（P3-T4）跑同一份文件，两边实现永不互相「对拍」，只对 fixture。
//!
//! 设计约定：
//! - **纯函数**：无 I/O、无全局状态、无时钟；输入输出全部走 serde 结构体，
//!   字段名与 fixture JSON 键一一对应（snake_case）。
//! - **f64 算术**：与原型 `design/assets/data.js` 及未来 TS 层同为 IEEE754 double，
//!   黄金向量按容差比对（03 附录比对规则），不引入 decimal 类型造成两套口径。
//! - **除零防护**：分母为 0 时各比率取 0（03 §5），不产生 NaN/Inf。
//! - **阈值判定带 ε 守卫**：[`THRESHOLD_EPS`]，恰好压线按「含边界」语义处理
//!   （03 附录约定 4 / 比对规则第 5 条），防浮点噪声翻转边界用例。

pub mod advice;
pub mod alert;
pub mod bench;
pub mod dilution;
pub mod discipline;
pub mod fees;
pub mod lot;
pub mod perf;
pub mod price_limit;
pub mod recalc;
pub mod vol_synth;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 年化交易日常数（03 §7 / §8）
pub const TRADING_DAYS: f64 = 244.0;

/// 夏普分母 ε 守卫（03 §7 公式内常数）
pub const SHARPE_EPS: f64 = 1e-9;

/// 阈值比较容差：|value − threshold| ≤ EPS 视为恰好压线（含边界语义）
pub const THRESHOLD_EPS: f64 = 1e-9;

/// 标的类型（03 §1）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A 股普通股
    Stock,
    /// 场内 ETF
    Etf,
    /// 可转债
    Cb,
}

/// 交易方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

/// 分母为 0 → 比率取 0（03 §5 除零防护）。
/// pub：宿主工具层同款守卫复用（ADR-0023 该票行业占比现算），口径不在 domain 外重写。
pub fn ratio(num: f64, den: f64) -> f64 {
    if den == 0.0 {
        0.0
    } else {
        num / den
    }
}
