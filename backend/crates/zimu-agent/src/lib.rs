//! 子木 · zimu-agent —— agent 应用层（P4-T6）。
//!
//! 定位：**领域工具 + Skill 模板 + 应用服务**的公共 crate，zimu-server（SSE 会话）
//! 与 zimu-agent-cli（调试 REPL）共同消费——P2 期两处重复的 echo 工具随本 crate
//! 收敛（agent_bridge.rs 的「P4-T6 抽公共 crate」承诺兑现）。
//!
//! 边界（ADR-0005/0007，CLAUDE.md 约定）：
//! - **数字口径只在 `zimu-domain`**：本 crate 的工具只做取数（`zimu-store` /
//!   `zimu-market`）→ 调领域函数 → 组装 JSON 返回，**不重定义任何口径**；
//! - **数字归规则，文字归模型**（ADR-0007）：`calc_advice` 把规则函数产出的
//!   score/conf/dims/target/stop 直接写入 `advice_history` 数字列并返回
//!   advice_id，模型文案经 `finalize_advice` 只回填 reason/risks——数字永不
//!   经过模型转写；
//! - **行情只经 `zimu-market`**（sidecar 唯一消费入口）取数，sidecar 挂了工具
//!   如实报错降级，不伪造数字；
//! - 系统不对接任何交易系统（ADR-0012/0013）：写账本工具（record_trade/
//!   adjust_cash/get_trades/upsert_holding/remove_holding）自动落账 + 读回，
//!   纪律 = 数字来自用户明示、先试算后落账并宣读收据、流水只增不改错账冲正、
//!   快照校正整行覆盖先读后改、删除持仓对话内两步确认；「永不代下单」是
//!   表述纪律，不是能力限制。
//!
//! Skill 系统模板在仓库根 `skills/`（每目录一个 SKILL.md，robit 约定），内嵌
//! `03` 口径引用与免责边界；ADR-0021 起任播种源——应用初始化播种进
//! `<wd>/.robit/skills`（缺则播种永不覆盖），加载单源 = 该目录，用户可自增技能；
//! `ZIMU_SKILLS_DIR` 可覆盖播种源。

pub mod assembly;
pub mod scheduler;
pub mod service;
pub mod skills;
pub mod tools;

use std::sync::{Arc, Mutex, MutexGuard};

use zimu_market::calendar::CalendarCache;
use zimu_market::sector::SectorMapCache;
use zimu_market::MarketApi;
use zimu_store::Store;

/// 持仓库共享句柄（与 zimu-server holdings_api 的同名别名是同一类型）
pub type SharedStore = Arc<Mutex<Store>>;

/// 锁中毒不毒化服务：与 server 侧同一语义（拿 inner 继续用）
pub fn lock(store: &SharedStore) -> MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// 工具集共享依赖：持仓库 + 行情适配器（+ 市场时钟用的日历缓存）。
/// server 装配时传入与 HTTP API 同一个 store/market 句柄。
pub struct AgentDeps {
    pub store: SharedStore,
    /// sidecar 唯一消费入口（zimu-market）
    pub market: Arc<dyn MarketApi>,
    /// 日历缓存（1h TTL）：market_status 工具的时钟输入。与 server HTTP 侧
    /// MarketState 的缓存各自独立——多打一次 sidecar 无害，换来 crate 间零耦合。
    pub calendar: CalendarCache,
    /// 行业映射缓存（24h TTL，P7-T6）：刷新链顺带补空串 sector 的映射输入。
    /// 与 server HTTP 侧 MarketState 的缓存各自独立（同 calendar 的零耦合取舍）。
    pub sector_map: SectorMapCache,
}

impl AgentDeps {
    pub fn new(store: SharedStore, market: Arc<dyn MarketApi>) -> Arc<Self> {
        Arc::new(Self {
            store,
            market,
            calendar: CalendarCache::default(),
            sector_map: SectorMapCache::default(),
        })
    }
}
