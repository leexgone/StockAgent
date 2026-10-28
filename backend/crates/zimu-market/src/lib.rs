//! 子木 · 行情适配器（P4-T4）——datafetch sidecar 的**唯一消费入口**。
//!
//! 边界（docs/04 附录 B「适配器边界」）：
//! - zimu-server 端点与后续 agent 领域工具（P4-T6）全部经本 crate 访问行情，
//!   代码库中不允许出现第二处对 sidecar 的 HTTP 调用。
//! - 本 crate **不定义业务口径**：涨跌停/费用/recalc 全在 `zimu-domain`
//!   （03 唯一来源）；这里只有传输契约（`models`）、降级分类（`error`）、
//!   市场时钟（`clock`，03 §9 生产口径）与当日 bar 合成（`synth`，04 附录 B）。
//! - sidecar 挂了主程序降级不瘫：`MarketError::Unavailable` 是降级信号，
//!   调用方据此走陈旧数据/503/日历启发式，绝不 panic、绝不阻塞启动。
//!
//! 测试从 `MarketApi` trait 注入假实现，不打网络；`HttpMarketClient` 的
//! 错误映射用本机回环测试（连接拒绝 / 裸 TCP 假响应），离线可跑。

pub mod calendar;
pub mod client;
pub mod clock;
pub mod error;
pub mod models;
pub mod symbols;
pub mod synth;

pub use client::{HttpMarketClient, MarketApi};
pub use error::{MarketError, MarketResult};
