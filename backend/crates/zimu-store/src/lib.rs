//! 子木 · 存储层（P4-T1/T2）——SQLite schema、迁移机制与「手工逐笔录录入」事务。
//!
//! 边界（ADR-0005 / ADR-0006 / ADR-0007 的落点）：
//! - **校验口径不在这里定义**：整手、涨跌停、费用、摊薄、仓位与纪律全部调用
//!   `zimu-domain` 的纯函数（口径唯一来源 `docs/03`，已被黄金向量锁死）。
//!   本 crate 只负责持久化与事务组织。
//! - **敏感数据永不入库（git 意义上的库）**：`.db` 文件在根 `.gitignore` 覆盖内；
//!   这里落的是运行数据，不是演示样本。演示样本永远只活在 `design/` 与 fixture。
//! - **奇零股可存储（B12 前提）**：`holdings.shares` 是普通整数，不加整手约束——
//!   送股/债转股会产生非整手持仓，卖出校验口径定稿前必须先保证存得下。
//! - **行情字段可空**：`price / prev / limit_pct` 由 P4-T4 行情适配器填充；
//!   录入时先以成交价占位 `price`，无行情时涨跌与涨跌停按 03 §2/§5 的
//!   「无数据 → 跳过 / 取 0」语义降级。
//! - **B9 正式解**：`advice_history` 带 `generated_at` 与 `data_version`，
//!   分析结论留档从这里出（P4-T6 起写入）。

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zimu_domain::dilution::{apply_trade, ApplyTradeInput, TradeOutcome, SINGLE_STOCK_LIMIT};
use zimu_domain::fees::{fee, FeeBreakdown, FeeInput};
use zimu_domain::lot::{lot_size, price_decimals, validate_qty, QtyReason};
use zimu_domain::price_limit::{limit_prices, validate_price};
use zimu_domain::recalc::{recalc, Agg, Holding, PerHolding, PortfolioInput};
use zimu_domain::{Kind, Side};

/// schema 版本（PRAGMA user_version）。新增迁移在 `MIGRATIONS` 尾部追加，
/// 永不修改已发布版本的 SQL——迁移只前进（P4-T2 迁移机制约定）。
pub const CURRENT_VERSION: i64 = 4;

const MIGRATIONS: &[(i64, &str)] = &[(
    1,
    r#"
CREATE TABLE account (
  id     INTEGER PRIMARY KEY CHECK (id = 1),          -- 单账户个人应用：恒一行
  name   TEXT NOT NULL DEFAULT '我的账户',
  init   TEXT NOT NULL DEFAULT '我',
  style  TEXT NOT NULL DEFAULT '',
  broker TEXT NOT NULL DEFAULT '',
  cash   REAL NOT NULL DEFAULT 0
);

CREATE TABLE holdings (
  code       TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  mkt        TEXT NOT NULL CHECK (mkt IN ('SH','SZ')), -- 显式字段，不按代码首字母猜（B7）
  kind       TEXT NOT NULL CHECK (kind IN ('stock','etf','cb')),
  sector     TEXT NOT NULL DEFAULT '',
  shares     INTEGER NOT NULL CHECK (shares > 0),      -- 任意整数股；奇零股必须存得下（B12）
  cost       REAL NOT NULL,                            -- 摊薄成本（03 §4 口径）
  price      REAL,                                     -- 行情字段：P4-T4 填充；清仓前成交占位
  prev       REAL,                                     -- 昨收：≤0/空 = 无行情，跳过涨跌停拦截（03 §2）
  limit_pct  INTEGER,                                  -- 比例是标的属性，行情源自动填充（03 §2）
  updated_at TEXT NOT NULL
);

CREATE TABLE trades (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  ts             TEXT NOT NULL,
  code           TEXT NOT NULL,
  name           TEXT NOT NULL,
  kind           TEXT NOT NULL,
  side           TEXT NOT NULL CHECK (side IN ('buy','sell')),
  qty            INTEGER NOT NULL CHECK (qty > 0),
  price          REAL NOT NULL,
  amount         REAL NOT NULL,
  fee            REAL NOT NULL,
  fee_commission REAL NOT NULL,                        -- 费用三分项（03 §3），留档可审计
  fee_stamp      REAL NOT NULL,
  fee_transfer   REAL NOT NULL,
  cash_delta     REAL NOT NULL,
  realized       REAL,                                 -- 仅卖出且有成本基准；清仓行 after_cost=null（03 附录约定 3）
  note           TEXT NOT NULL DEFAULT '',
  holding_code   TEXT                                    -- 关联标的码（**不是外键**）：
                                                         -- 流水必须在清仓后仍留档，引用完整性会挡 DELETE
);

-- B9 正式解：每条建议带生成时间与输入数据版本，可回溯「当时依据什么说的」
CREATE TABLE advice_history (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  code         TEXT NOT NULL,
  action       TEXT NOT NULL,
  score        REAL NOT NULL,                           -- ADR-0007：数字归规则，打分由领域函数产出后写入
  conf         REAL NOT NULL,                           -- 样本内信号一致性，不是胜率（CLAUDE.md 安全边界）
  target_lo    REAL,
  target_hi    REAL,
  stop         REAL,
  position     TEXT NOT NULL DEFAULT '',
  reason       TEXT NOT NULL DEFAULT '',                -- 文字归模型：结论文案从这里存
  dims         TEXT NOT NULL DEFAULT '[]',              -- JSON：四维拆解 [{k,s,t}]
  risks        TEXT NOT NULL DEFAULT '[]',              -- JSON：风险自陈 [string]
  data_version TEXT NOT NULL,
  generated_at TEXT NOT NULL
);
CREATE INDEX idx_advice_code ON advice_history(code, generated_at);

CREATE TABLE alerts (
  id     INTEGER PRIMARY KEY AUTOINCREMENT,
  ts     TEXT NOT NULL,
  code   TEXT NOT NULL,
  name   TEXT NOT NULL,
  type   TEXT NOT NULL,
  level  TEXT NOT NULL,                                 -- 好坏轴语义，与原型 .ok/.warn/.danger 同源
  text   TEXT NOT NULL,
  done   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE equity_snapshots (
  date          TEXT PRIMARY KEY,                        -- 每交易日一条；15:05 调度写入是 P4-T7
  ts            TEXT NOT NULL,
  total_assets  REAL NOT NULL, total_mv REAL NOT NULL, cash REAL NOT NULL,
  day_pnl       REAL NOT NULL, day_pnl_pct REAL NOT NULL,
  total_pnl     REAL NOT NULL, total_pnl_pct REAL NOT NULL,
  exposure      REAL NOT NULL, hhi REAL NOT NULL, top3_sector REAL NOT NULL
);

CREATE TABLE push_log (
  id      INTEGER PRIMARY KEY AUTOINCREMENT,
  ts      TEXT NOT NULL,
  channel TEXT NOT NULL,
  kind    TEXT NOT NULL,
  ok      INTEGER NOT NULL,
  detail  TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_trades_ts ON trades(ts);
"#,
    ),
    (
        2,
        r#"
-- B13 定稿（2026-10-29）：自选表——观察标的（未持仓，无 shares/cost）。
-- 03 §12 判定范围扩到自选（无持仓形态：序 3/4/5 不参与）；列语义与 holdings
-- 静态列同款（B7 显式 mkt、limit_pct 是标的属性——数据层配置优先）。
CREATE TABLE watchlist (
  code       TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  mkt        TEXT NOT NULL CHECK (mkt IN ('SH','SZ')),
  kind       TEXT NOT NULL CHECK (kind IN ('stock','etf','cb')),
  limit_pct  INTEGER,
  sector     TEXT NOT NULL DEFAULT '',
  note       TEXT NOT NULL DEFAULT '',
  added_at   TEXT NOT NULL
);
"#,
    ),
    (
        3,
        r#"
-- ADR-0009 / 03 §13（2026-11-01 定稿）：持仓直接维护的审计留痕表。
-- 快照校正不写 trades（不伪造零费用交易污染费用/已实现口径）；
-- before/after = 持仓行可编辑字段快照 JSON（create → before NULL，
-- delete → after NULL；price/prev 是行情字段不可编辑，不进快照）。
CREATE TABLE holding_edits (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  ts          TEXT NOT NULL,
  op          TEXT NOT NULL CHECK (op IN ('create','update','delete')),
  code        TEXT NOT NULL,
  name        TEXT NOT NULL,             -- 操作时刻的标的名称（删除后仍可展示）
  before_json TEXT,
  after_json  TEXT,
  note        TEXT NOT NULL DEFAULT ''
);
"#,
    ),
    (
        4,
        r#"
-- T2/ADR-0011（2026-11-07 拍板）：智能体接入模块的服务端会话真值 + IM 入站白名单。
-- chat_messages：Web 会话档案（通道独立——QQ/微信会话历史归 robit 自持库，不进这张表）。
--   session 恒 'web'（词汇表留通道扩展位）；tool 行**只供展示与审计，复活回灌时
--   只取 user/assistant 文本对**——回灌行情/持仓陈旧数字违反 03「派生数字现算」。
--   thinking 段不入库（框架「thinking 不写回 history」口径，docs/02 展示口径同源）。
-- channel_bindings：IM 入站白名单（BoundOnly decorator 的判定表）。绑定是运营动作：
--   个人应用频率下每次 recv 直查本表即可，不做缓存。
CREATE TABLE chat_messages (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  session   TEXT NOT NULL,
  role      TEXT NOT NULL CHECK (role IN ('user','assistant','tool')),
  text      TEXT NOT NULL DEFAULT '',    -- role='tool' 时为空串（内容在 tool_json）
  tool_json TEXT,                        -- role='tool'：{name,args,result,is_error} 展示快照
  ts        TEXT NOT NULL
);
CREATE INDEX idx_chat_messages_session ON chat_messages(session, id);

CREATE TABLE channel_bindings (
  channel  TEXT NOT NULL,                -- 'qq'（词汇表；'wechat' 等随 robit 模块到货扩展）
  chat_id  TEXT NOT NULL,                -- 平台会话 id（QQ 侧形如 private:{openid} / group:{openid}）
  kind     TEXT NOT NULL CHECK (kind IN ('private','group')),
  label    TEXT NOT NULL DEFAULT '',
  bound_at TEXT NOT NULL,
  active   INTEGER NOT NULL DEFAULT 1,   -- unbind 置 0 留痕（不 DELETE：推送历史/审计要能对上号）
  PRIMARY KEY (channel, chat_id)
);
"#,
    ),
];

// ============================================================================
// 行类型
// ============================================================================

fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Stock => "stock",
        Kind::Etf => "etf",
        Kind::Cb => "cb",
    }
}

fn kind_from(s: &str) -> Result<Kind> {
    Ok(match s {
        "stock" => Kind::Stock,
        "etf" => Kind::Etf,
        "cb" => Kind::Cb,
        other => anyhow::bail!("未知 kind: {other}"),
    })
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AccountRow {
    pub name: String,
    pub init: String,
    pub style: String,
    pub broker: String,
    pub cash: f64,
}

impl Default for AccountRow {
    fn default() -> Self {
        AccountRow {
            name: "我的账户".into(),
            init: "我".into(),
            style: String::new(),
            broker: String::new(),
            cash: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HoldingRow {
    pub code: String,
    pub name: String,
    pub mkt: String,
    pub kind: Kind,
    pub sector: String,
    pub shares: i64,
    pub cost: f64,
    pub price: Option<f64>,
    pub prev: Option<f64>,
    pub limit_pct: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TradeRow {
    pub id: i64,
    pub ts: String,
    pub code: String,
    pub name: String,
    pub kind: Kind,
    pub side: Side,
    pub qty: i64,
    pub price: f64,
    pub amount: f64,
    pub fee: f64,
    pub fee_commission: f64,
    pub fee_stamp: f64,
    pub fee_transfer: f64,
    pub cash_delta: f64,
    pub realized: Option<f64>,
    pub note: String,
}

// ============================================================================
// 录入请求与结果
// ============================================================================

/// 新建持仓所需的标的元信息（卖出与已有标的买入不需要）。
/// 真实流程里这些来自行情搜索/行情源（P4-T4），P4-T1 由请求方携带。
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NewHoldingMeta {
    pub name: String,
    /// 'SH' | 'SZ'
    pub mkt: String,
    pub kind: Kind,
    #[serde(default)]
    pub sector: String,
    /// 昨收与涨跌停比例：缺省 = 无行情，跳过区间拦截（03 §2）
    pub prev: Option<f64>,
    pub limit_pct: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TradeRequest {
    pub code: String,
    pub side: Side,
    pub qty: i64,
    pub price: f64,
    #[serde(default)]
    pub note: String,
    /// 首笔建仓必带；标的已存在于 holdings 时可省略
    pub meta: Option<NewHoldingMeta>,
}

/// 试算预览（抽屉「预估」区的数据源；与原型 `holdings.html` calc() 同构）
#[derive(Debug, Serialize, ToSchema)]
pub struct SimPreview {
    pub fee_detail: FeeBreakdown,
    pub outcome: TradeOutcome,
    /// 非拦截性提示：如「单票将超过 25% 上限」（原型只在预览提示，不拦提交）
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecordReceipt {
    pub trade: TradeRow,
    /// 清仓后为 null（持仓行已移除）
    pub holding_after: Option<HoldingRow>,
    /// 本笔卖出是否导致清仓移除持仓行
    pub removed: bool,
    pub preview: SimPreview,
}

/// 录入拒绝原因（HTTP 层据此选状态码）
#[derive(Debug, Clone)]
pub enum RecordError {
    /// 数量/标的信息类输入问题
    BadInput(String),
    /// 卖出/加仓但持仓里没有这只标的
    NoHolding { code: String },
    /// 新建持仓缺标的元信息
    NeedMeta { code: String },
    /// 委托价越出当日涨跌停区间
    OutOfLimitRange { price: f64, lo: f64, hi: f64, prec: u32 },
    /// 现金不足（原型按钮置灰语义）
    InsufficientCash { cash_after: f64 },
    /// 存储层内部错误（SQL/IO）——非用户输入问题，HTTP 层应给 500
    Store(String),
}

/// 只读查询路径上的 anyhow/rusqlite 错误统一归为 Store（与用户拒绝区分）
impl From<anyhow::Error> for RecordError {
    fn from(e: anyhow::Error) -> Self {
        RecordError::Store(format!("{e:#}"))
    }
}

impl From<rusqlite::Error> for RecordError {
    fn from(e: rusqlite::Error) -> Self {
        RecordError::Store(e.to_string())
    }
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::BadInput(m) => write!(f, "{m}"),
            RecordError::NoHolding { code } => write!(f, "持仓中没有 {code}，无法卖出/减仓"),
            RecordError::NeedMeta { code } => {
                write!(f, "{code} 尚未持有，新建持仓需要提供标的信息（名称/市场/类型）")
            }
            RecordError::OutOfLimitRange { .. } => write!(f, "委托价格超出当日涨跌停区间"),
            RecordError::InsufficientCash { .. } => write!(f, "现金不足"),
            RecordError::Store(m) => write!(f, "存储层错误：{m}"),
        }
    }
}

impl std::error::Error for RecordError {}

// ============================================================================
// Store
// ============================================================================

pub struct Store {
    conn: Connection,
}

impl Store {
    /// 打开（或创建）SQLite 文件并跑迁移。WAL 模式：常驻服务读写并发更稳。
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
        let conn = Connection::open(path)
            .with_context(|| format!("打开 SQLite 失败：{path}"))?;
        Self::init(conn)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let mut store = Store { conn };
        store.migrate()?;
        store.ensure_account()?;
        Ok(store)
    }

    /// user_version 逐版前进；每版一个事务，全部成功才算迁移完成。
    fn migrate(&mut self) -> Result<()> {
        let mut version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .context("读取 user_version 失败")?;
        for &(target, sql) in MIGRATIONS {
            if version >= target {
                continue;
            }
            let tx = self
                .conn
                .transaction()
                .with_context(|| format!("迁移事务开启失败（→ v{target}）"))?;
            tx.execute_batch(sql)
                .with_context(|| format!("迁移 v{target} 执行失败"))?;
            tx.pragma_update(None, "user_version", target)?;
            tx.commit()?;
            version = target;
        }
        if version < CURRENT_VERSION {
            anyhow::bail!("schema 版本 {version} 落后于程序期望的 {CURRENT_VERSION}：请检查迁移表");
        }
        Ok(())
    }

    pub fn version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// 单例账户行（不存在则以默认值建；存在不动现金——重启不丢数据）
    pub fn ensure_account(&self) -> Result<()> {
        let d = AccountRow::default();
        self.conn.execute(
            "INSERT OR IGNORE INTO account (id, name, init, style, broker, cash) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![d.name, d.init, d.style, d.broker, d.cash],
        )?;
        Ok(())
    }

    pub fn set_account(&self, acc: &AccountRow) -> Result<()> {
        self.conn.execute(
            "UPDATE account SET name=?1, init=?2, style=?3, broker=?4, cash=?5 WHERE id=1",
            params![acc.name, acc.init, acc.style, acc.broker, acc.cash],
        )?;
        Ok(())
    }

    /// 首次配置 / 调整账户：None 字段保持原值（onboarding 通常只带 cash）。
    /// 现金为负按拒绝处理——现金下限是业务事实，不是 UI 校验。
    pub fn update_account(
        &self,
        name: Option<&str>,
        init: Option<&str>,
        style: Option<&str>,
        broker: Option<&str>,
        cash: Option<f64>,
    ) -> std::result::Result<AccountRow, RecordError> {
        if let Some(c) = cash {
            if !(c >= 0.0) {
                return Err(RecordError::BadInput("现金不能为负".into()));
            }
        }
        let cur = self.account().map_err(RecordError::from)?;
        let next = AccountRow {
            name: name.unwrap_or(&cur.name).to_string(),
            init: init.unwrap_or(&cur.init).to_string(),
            style: style.unwrap_or(&cur.style).to_string(),
            broker: broker.unwrap_or(&cur.broker).to_string(),
            cash: cash.unwrap_or(cur.cash),
        };
        self.set_account(&next).map_err(RecordError::from)?;
        Ok(next)
    }

    pub fn account(&self) -> Result<AccountRow> {
        Ok(self.conn.query_row(
            "SELECT name, init, style, broker, cash FROM account WHERE id=1",
            [],
            |r| {
                Ok(AccountRow {
                    name: r.get(0)?,
                    init: r.get(1)?,
                    style: r.get(2)?,
                    broker: r.get(3)?,
                    cash: r.get(4)?,
                })
            },
        )?)
    }

    pub fn holdings(&self) -> Result<Vec<HoldingRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT code, name, mkt, kind, sector, shares, cost, price, prev, limit_pct, updated_at
             FROM holdings ORDER BY (COALESCE(price, cost) * shares) DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let kind_s: String = r.get(3)?;
                Ok(HoldingRow {
                    code: r.get(0)?,
                    name: r.get(1)?,
                    mkt: r.get(2)?,
                    kind: kind_from(&kind_s)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string()))))?,
                    sector: r.get(4)?,
                    shares: r.get(5)?,
                    cost: r.get(6)?,
                    price: r.get(7)?,
                    prev: r.get(8)?,
                    limit_pct: r.get(9)?,
                    updated_at: r.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn trades(&self, limit: i64) -> Result<Vec<TradeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, code, name, kind, side, qty, price, amount, fee,
                    fee_commission, fee_stamp, fee_transfer, cash_delta, realized, note
             FROM trades ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                let kind_s: String = r.get(4)?;
                let side_s: String = r.get(5)?;
                Ok(TradeRow {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    code: r.get(2)?,
                    name: r.get(3)?,
                    kind: kind_from(&kind_s)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string()))))?,
                    side: match side_s.as_str() {
                        "buy" => Side::Buy,
                        _ => Side::Sell,
                    },
                    qty: r.get(6)?,
                    price: r.get(7)?,
                    amount: r.get(8)?,
                    fee: r.get(9)?,
                    fee_commission: r.get(10)?,
                    fee_stamp: r.get(11)?,
                    fee_transfer: r.get(12)?,
                    cash_delta: r.get(13)?,
                    realized: r.get(14)?,
                    note: r.get(15)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 领域输入组装（唯一来源）：price/prev 缺失时按「无行情降级」（03 §5，
    /// P4-T4 定稿）——price 以成本顶替，prev 视为等于 price → dayChg/dayVal 取 0，
    /// **不伪造盈亏**（prev 置 0 会把 dayVal 算成整笔市值——禁止）。
    /// `recalc` 与 `discipline::check` 等一切领域计算的输入都从这里出（P4-T6 起
    /// agent 工具直接消费，杜绝第二份降级语义拼装）。
    pub fn portfolio_input(&self) -> Result<PortfolioInput> {
        let holdings = self.holdings()?;
        let account = self.account()?;
        Ok(PortfolioInput {
            cash: account.cash,
            holdings: holdings
                .iter()
                .map(|h| {
                    let price = h.price.unwrap_or(h.cost);
                    Holding {
                        code: h.code.clone(),
                        kind: h.kind,
                        sector: h.sector.clone(),
                        shares: h.shares as f64,
                        cost: h.cost,
                        price,
                        prev: h.prev.unwrap_or(price),
                    }
                })
                .collect(),
        })
    }

    /// 组合全景：存储行 + `zimu-domain::recalc` 的逐笔派生与汇总（03 §5 口径）。
    pub fn portfolio(&self) -> Result<PortfolioView> {
        let holdings = self.holdings()?;
        let account = self.account()?;
        let inp = self.portfolio_input()?;
        let out = recalc(&inp);
        Ok(PortfolioView {
            account,
            holdings,
            per: out.per,
            agg: out.agg,
        })
    }

    fn holding_of(&self, code: &str) -> Result<Option<HoldingRow>> {
        Ok(self
            .holdings()?
            .into_iter()
            .find(|h| h.code == code))
    }

    /// 其他持仓市值合计（原型抽屉口径：取快照，不随本笔价格变化）。
    /// 无 quote 的行以成本计——只用于仓位估算，不影响费用。
    fn others_mv(&self, except_code: &str) -> Result<f64> {
        let sum: f64 = self.conn.query_row(
            "SELECT COALESCE(SUM((CASE WHEN price IS NULL THEN cost ELSE price END) * shares), 0.0)
             FROM holdings WHERE code != ?1",
            params![except_code],
            |r| r.get(0),
        )?;
        Ok(sum)
    }

    /// 校验 + 试算（不落库）。P4-T6 的 simulate_trade 领域工具直接走这里。
    /// 校验顺序照原型抽屉：数量>0 → 标的/方向合法性 → 整手与可卖 →
    /// 涨跌停区间（有昨收才拦，03 §2）→ 现金充足。「超 25% 上限」不拒绝、进 warnings。
    pub fn simulate_trade(&self, req: &TradeRequest) -> std::result::Result<SimPreview, RecordError> {
        self.preview(req).map(|(p, _)| p)
    }

    pub fn record_trade(
        &self,
        req: &TradeRequest,
    ) -> std::result::Result<RecordReceipt, RecordError> {
        // preview 移入闭包（成功路径里唯一消费者是收据构造），引用在闭包内取、
        // 结束时随 `preview` 字段一并移动，避免跨闭包借用。
        let (preview, row_state) = self.preview(req)?;

        let tx = &self.conn;
        // 事务：验证通过后才动笔。SQLite 默认 autocommit，这里用显式 BEGIN IMMEDIATE。
        tx.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| RecordError::BadInput(format!("事务开启失败：{e}")))?;
        let result = (move || -> std::result::Result<RecordReceipt, RecordError> {
            let outcome = &preview.outcome;
            let fd = &preview.fee_detail;
            let ts = tx
                .query_row("SELECT strftime('%Y-%m-%d %H:%M','now','localtime')", [], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(|e| RecordError::BadInput(format!("读取时间失败：{e}")))?;

            let cash_after = outcome.cash_after;
            let (holding_after, removed) = match req.side {
                Side::Buy => {
                    let (code, name, mkt, kind_s, sector, prev, limit_pct) = match row_state {
                        RowState::Existing(ref h) => (
                            h.code.clone(),
                            h.name.clone(),
                            h.mkt.clone(),
                            kind_str(h.kind).to_string(),
                            h.sector.clone(),
                            h.prev,
                            h.limit_pct,
                        ),
                        RowState::New(ref meta) => (
                            req.code.clone(),
                            meta.name.clone(),
                            meta.mkt.clone(),
                            kind_str(meta.kind).to_string(),
                            meta.sector.clone(),
                            meta.prev,
                            meta.limit_pct,
                        ),
                    };
                    let after_shares = outcome.after_shares as i64;
                    let after_cost = outcome.after_cost.ok_or_else(|| {
                        RecordError::BadInput("买入后持仓数必须为正（内部错误）".into())
                    })?;
                    tx.execute(
                        "INSERT INTO holdings (code,name,mkt,kind,sector,shares,cost,price,prev,limit_pct,updated_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                         ON CONFLICT(code) DO UPDATE SET
                           shares=?6, cost=?7, price=?8, prev=COALESCE(?9, holdings.prev),
                           limit_pct=COALESCE(?10, holdings.limit_pct), updated_at=?11",
                        params![code, name, mkt, kind_s, sector, after_shares, after_cost,
                                req.price, prev, limit_pct, ts],
                    )
                    .map_err(|e| RecordError::BadInput(format!("写入持仓失败：{e}")))?;
                    (
                        Some(HoldingRow {
                            code,
                            name,
                            mkt,
                            kind: kind_from(&kind_s).map_err(|e| RecordError::BadInput(e.to_string()))?,
                            sector,
                            shares: after_shares,
                            cost: after_cost,
                            price: Some(req.price),
                            prev,
                            limit_pct,
                            updated_at: ts.clone(),
                        }),
                        false,
                    )
                }
                Side::Sell => {
                    let h = match row_state {
                        RowState::Existing(ref h) => h.clone(),
                        _ => unreachable!("卖出必然是 Existing"),
                    };
                    if outcome.after_shares <= 0.0 {
                        tx.execute("DELETE FROM holdings WHERE code=?1", params![h.code])
                            .map_err(|e| RecordError::BadInput(format!("清仓删除失败：{e}")))?;
                        (None, true)
                    } else {
                        tx.execute(
                            "UPDATE holdings SET shares=?1, price=?2, updated_at=?3 WHERE code=?4",
                            params![outcome.after_shares as i64, req.price, ts, h.code],
                        )
                        .map_err(|e| RecordError::BadInput(format!("写入持仓失败：{e}")))?;
                        (
                            Some(HoldingRow {
                                shares: outcome.after_shares as i64,
                                price: Some(req.price),
                                updated_at: ts.clone(),
                                ..h
                            }),
                            false,
                        )
                    }
                }
            };

            let cash_delta = match req.side {
                Side::Buy => -(fd.amount + fd.total),
                Side::Sell => fd.amount - fd.total,
            };
            tx.execute(
                "INSERT INTO trades (ts,code,name,kind,side,qty,price,amount,fee,fee_commission,
                   fee_stamp,fee_transfer,cash_delta,realized,note,holding_code)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                params![
                    ts, req.code, holding_name_for(&row_state),
                    kind_str(row_kind(&row_state)), side_str(req.side), req.qty, req.price,
                    fd.amount, fd.total, fd.commission, fd.stamp_tax, fd.transfer_fee,
                    cash_delta, outcome.realized, req.note, req.code
                ],
            )
            .map_err(|e| RecordError::BadInput(format!("写入流水失败：{e}")))?;

            tx.execute("UPDATE account SET cash=?1 WHERE id=1", params![cash_after])
                .map_err(|e| RecordError::BadInput(format!("更新现金失败：{e}")))?;
            Ok(RecordReceipt {
                trade: TradeRow {
                    id: tx.last_insert_rowid(),
                    ts,
                    code: req.code.clone(),
                    name: holding_name_for(&row_state),
                    kind: row_kind(&row_state),
                    side: req.side,
                    qty: req.qty,
                    price: req.price,
                    amount: fd.amount,
                    fee: fd.total,
                    fee_commission: fd.commission,
                    fee_stamp: fd.stamp_tax,
                    fee_transfer: fd.transfer_fee,
                    cash_delta,
                    realized: outcome.realized,
                    note: req.note.clone(),
                },
                holding_after,
                removed,
                preview,
            })
        })();

        match result {
            Ok(receipt) => {
                self.conn
                    .execute_batch("COMMIT")
                    .map_err(|e| RecordError::BadInput(format!("提交失败：{e}")))?;
                Ok(receipt)
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }

    /// 校验 + 试算的内部实现；同时返回行状态（新/旧）供写入分支使用。
    fn preview(&self, req: &TradeRequest) -> std::result::Result<(SimPreview, RowState), RecordError> {
        if req.qty <= 0 {
            return Err(RecordError::BadInput("数量必须大于 0".into()));
        }
        // 否定式是有意的 NaN 守卫：price 为 NaN 时 `NaN > 0.0` 为 false，
        // 直接 `price <= 0.0` 会漏掉 NaN，故取反（同 price_limit.rs 的 NaN 处理）
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(req.price > 0.0) {
            return Err(RecordError::BadInput("委托价格必须大于 0".into()));
        }
        let existing = self.holding_of(&req.code)?;
        let bad = |m: String| Err::<(SimPreview, RowState), RecordError>(RecordError::BadInput(m));

        // 标的与方向上下文（现金来源恒为 account 行）
        let (kind, held, cost, row_state) = match (existing, req.side) {
            (Some(h), _) => (h.kind, h.shares, h.cost, RowState::Existing(h)),
            (None, Side::Buy) => {
                let meta = req.meta.clone().ok_or(RecordError::NeedMeta {
                    code: req.code.clone(),
                })?;
                if meta.mkt != "SH" && meta.mkt != "SZ" {
                    return bad(format!("市场码非法：{}（应为 SH/SZ）", meta.mkt));
                }
                (meta.kind, 0, 0.0, RowState::New(meta))
            }
            (None, Side::Sell) => {
                return Err(RecordError::NoHolding {
                    code: req.code.clone(),
                })
            }
        };

        let account_cash = self.account()?.cash;

        // 整手与可卖数量（B12 定稿前与原型一致：买卖一律整手拦截）
        let verdict = validate_qty(kind, req.side, req.qty, held);
        if !verdict.valid {
            let lot = lot_size(kind);
            let unit = match kind {
                Kind::Stock => "股",
                Kind::Etf => "份",
                Kind::Cb => "张",
            };
            let msg = match verdict.reason {
                Some(QtyReason::ExceedsHeld) => format!("最多可卖 {held} {unit}"),
                _ => format!("数量需为 {lot} 的整数倍（当前 {}）", req.qty),
            };
            return bad(msg);
        }

        // 涨跌停：昨收/比例齐了才拦（03 §2：无行情跳过区间拦截）
        let (prev, limit_pct) = match &row_state {
            RowState::Existing(h) => (h.prev, h.limit_pct),
            RowState::New(m) => (m.prev, m.limit_pct),
        };
        if let (Some(prev), Some(pct)) = (prev, limit_pct) {
            let lim = limit_prices(kind, prev, pct);
            if let (Some(up), Some(down)) = (lim.limit_up, lim.limit_down) {
                let pv = validate_price(req.price, up, down);
                if !pv.valid {
                    return Err(RecordError::OutOfLimitRange {
                        price: req.price,
                        lo: down,
                        hi: up,
                        prec: price_decimals(kind),
                    });
                }
            }
        }

        let others_mv = self.others_mv(&req.code)?;
        let fee_detail = fee(FeeInput {
            kind,
            side: req.side,
            qty: req.qty as f64,
            price: req.price,
        });
        let outcome = apply_trade(&ApplyTradeInput {
            kind,
            side: req.side,
            qty: req.qty as f64,
            price: req.price,
            held: held as f64,
            cost,
            cash: account_cash,
            others_mv,
        });
        if outcome.insufficient_cash {
            return Err(RecordError::InsufficientCash {
                cash_after: outcome.cash_after,
            });
        }

        let mut warnings = Vec::new();
        if outcome.over_limit {
            warnings.push(format!(
                "单票将超过 {:.0}% 上限",
                SINGLE_STOCK_LIMIT
            ));
        }
        let lot = zimu_domain::lot::lot_size(kind);
        if held > 0 && held % lot != 0 {
            warnings.push(format!("当前持仓含非整手部分（{held}），送股/转股产生，卖出可整手或一次性全卖（B12 待定稿）"));
        }
        Ok((
            SimPreview {
                fee_detail,
                outcome,
                warnings,
            },
            row_state,
        ))
    }
}

/// 标的行状态：已持有（带行数据）或新建（带元信息）。preview 只产出这两种。
#[derive(Debug, Clone)]
enum RowState {
    Existing(HoldingRow),
    New(NewHoldingMeta),
}

fn row_kind(s: &RowState) -> Kind {
    match s {
        RowState::Existing(h) => h.kind,
        RowState::New(m) => m.kind,
    }
}

fn holding_name_for(s: &RowState) -> String {
    match s {
        RowState::Existing(h) => h.name.clone(),
        RowState::New(m) => m.name.clone(),
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PortfolioView {
    pub account: AccountRow,
    pub holdings: Vec<HoldingRow>,
    /// 与 holdings 同序对齐（recalc 输出按输入序）
    pub per: Vec<PerHolding>,
    pub agg: Agg,
}

// ============================================================================
// 行情回写（P4-T4）
// ============================================================================

/// 行情回写条目。**口径边界**：有效性过滤（status==ok、price>0）与 `limit_pct`
/// 推断（`zimu-domain::price_limit::limit_pct`）都在调用方（server 适配层）完成，
/// 存储层只负责持久化——COALESCE 语义：`None` 字段保持原值不动
///（数据层已配置的 `limit_pct` 永不被推断值/空值覆盖，03 §2）。
#[derive(Debug, Clone)]
pub struct QuoteUpdate {
    pub code: String,
    pub price: Option<f64>,
    pub prev: Option<f64>,
    pub name: Option<String>,
    pub limit_pct: Option<i64>,
    /// 回写时刻（上海，YYYY-MM-DD HH:MM，与录入流水同格式）
    pub ts: String,
}

impl Store {
    /// 批量回写行情（单事务，全成或全不成）。返回实际更新的行数；
    /// `code` 不在 holdings 里静默跳过（调用方多给了标的不是存储层错误）。
    pub fn apply_quotes(&self, ups: &[QuoteUpdate]) -> Result<usize> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<usize> {
            let mut n = 0;
            for u in ups {
                n += self.conn.execute(
                    "UPDATE holdings SET
                       price=COALESCE(?1, price), prev=COALESCE(?2, prev),
                       name=COALESCE(?3, name), limit_pct=COALESCE(?4, limit_pct),
                       updated_at=?5
                     WHERE code=?6",
                    params![u.price, u.prev, u.name, u.limit_pct, u.ts, u.code],
                )?;
            }
            Ok(n)
        })();
        match result {
            Ok(n) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(n)
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }

    /// 批量补全行业（P7-T6/ADR-0016）——**仅写 sector='' 的行**（手工非空值永不
    /// 被自动覆盖）。单事务全成或全不成；`code` 不存在静默跳过；返回实际写入行数。
    /// 调用方保证 sector 非空。触 `updated_at`（行确实变了，apply_quotes 同款）；
    /// **不写 holding_edits**（数据层富化非用户校正，行情回写同款）。
    /// `WHERE sector=''` 天然幂等（二次调用 0 行），不用 CASE WHEN（避免白触非空行）。
    pub fn fill_sectors(&self, fills: &[(String, String)], ts: &str) -> Result<usize> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<usize> {
            let mut n = 0;
            for (code, sector) in fills {
                n += self.conn.execute(
                    "UPDATE holdings SET sector=?1, updated_at=?2
                     WHERE code=?3 AND sector=''",
                    params![sector, ts, code],
                )?;
            }
            Ok(n)
        })();
        match result {
            Ok(n) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(n)
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }
}

// ============================================================================
// 建议留档（advice_history，P4-T6 起写入——B9 正式解的写入侧）
// ============================================================================

/// 分析结论留档行。**数字归规则**（`zimu-domain::advice` 产出 → `insert_advice`
/// 落数字列），**文字归模型**（`finalize_advice` 回填 reason/risks，ADR-0007）——
/// 数字列一经写入不经过模型转写，留档即可回溯「当时依据什么说的」。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdviceRow {
    pub id: i64,
    pub code: String,
    /// 结论标签（中文展示态：加仓/买入/持有/减仓/卖出/定投，由工具自 domain 枚举映射）
    pub action: String,
    pub score: f64,
    /// 样本内信号一致性，不是胜率（CLAUDE.md 安全边界）
    pub conf: f64,
    pub target_lo: Option<f64>,
    pub target_hi: Option<f64>,
    pub stop: Option<f64>,
    /// 「当前 x% → 建议 y%」（工具侧拼装，03 §10 position 文案）
    pub position: String,
    /// 模型结论文案；空串 = 数字已留档、文案未回填
    pub reason: String,
    /// 四维拆解 `[{k,s,t}]`（03 §10 固定四维；domain `AdviceDim` 序列化，坏 JSON 读回 `[]`）
    pub dims: serde_json::Value,
    /// 风险自陈（03 §10：至少一条——非空校验在工具层，存储层如实存）
    pub risks: Vec<String>,
    /// 输入数据版本 `adv-v1|k={日线末bar|none}|q={行情as_of|none}`（B9）
    pub data_version: String,
    /// 生成时刻（上海，YYYY-MM-DD HH:MM）
    pub generated_at: String,
}

/// 建议留档的数字列（reason/risks 留空，等 `finalize_advice` 回填文案）。
pub struct NewAdvice {
    pub code: String,
    pub action: String,
    pub score: f64,
    pub conf: f64,
    pub target_lo: Option<f64>,
    pub target_hi: Option<f64>,
    pub stop: Option<f64>,
    pub position: String,
    pub dims: serde_json::Value,
    pub data_version: String,
    pub generated_at: String,
}

fn map_advice_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AdviceRow> {
    let dims_text: String = r.get(10)?;
    let risks_text: String = r.get(11)?;
    Ok(AdviceRow {
        id: r.get(0)?,
        code: r.get(1)?,
        action: r.get(2)?,
        score: r.get(3)?,
        conf: r.get(4)?,
        target_lo: r.get(5)?,
        target_hi: r.get(6)?,
        stop: r.get(7)?,
        position: r.get(8)?,
        reason: r.get(9)?,
        dims: serde_json::from_str(&dims_text).unwrap_or_else(|_| serde_json::json!([])),
        risks: serde_json::from_str(&risks_text).unwrap_or_default(),
        data_version: r.get(12)?,
        generated_at: r.get(13)?,
    })
}

const ADVICE_COLS: &str = "id, code, action, score, conf, target_lo, target_hi, stop,
     position, reason, dims, risks, data_version, generated_at";

impl Store {
    /// 建议数字列入档，返回行 id（B9：每条建议带生成时间与数据版本）。
    pub fn insert_advice(&self, a: &NewAdvice) -> Result<i64> {
        let dims_text = serde_json::to_string(&a.dims).context("dims 序列化失败")?;
        self.conn.execute(
            "INSERT INTO advice_history
               (code, action, score, conf, target_lo, target_hi, stop,
                position, reason, dims, risks, data_version, generated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'',?9,'[]',?10,?11)",
            params![
                a.code,
                a.action,
                a.score,
                a.conf,
                a.target_lo,
                a.target_hi,
                a.stop,
                a.position,
                dims_text,
                a.data_version,
                a.generated_at
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 模型文案回填（文字归模型：只动 reason/risks，数字列不可改写——ADR-0007
    /// 的结构性保证）。返回 `false` = id 不存在。允许重复回填（末次为准）。
    pub fn finalize_advice(&self, id: i64, reason: &str, risks: &[String]) -> Result<bool> {
        let risks_text = serde_json::to_string(risks).context("risks 序列化失败")?;
        let n = self.conn.execute(
            "UPDATE advice_history SET reason=?1, risks=?2 WHERE id=?3",
            params![reason, risks_text, id],
        )?;
        Ok(n > 0)
    }

    pub fn advice(&self, id: i64) -> Result<Option<AdviceRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ADVICE_COLS} FROM advice_history WHERE id = ?1"
        ))?;
        let mut rows = stmt.query_map(params![id], map_advice_row)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// 按生成时间倒序（新在前）列出留档；`code` 为 None 时全量。
    pub fn list_advice(&self, code: Option<&str>, limit: i64) -> Result<Vec<AdviceRow>> {
        let sql = if code.is_some() {
            format!("SELECT {ADVICE_COLS} FROM advice_history WHERE code = ?1 ORDER BY generated_at DESC, id DESC LIMIT ?2")
        } else {
            format!("SELECT {ADVICE_COLS} FROM advice_history ORDER BY generated_at DESC, id DESC LIMIT ?1")
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match code {
            Some(c) => stmt.query_map(params![c, limit], map_advice_row)?,
            None => stmt.query_map(params![limit], map_advice_row)?,
        };
        rows.collect::<Result<Vec<_>, _>>().context("读取建议留档失败")
    }
}

// ============================================================================
// 预警留档（alerts，P4-T7 起写入——判定口径在 zimu-domain::alert，03 §12）
// ============================================================================

/// 预警行。`type`/`level`/`text` 全部由规则函数产出（调度器只搬运，不改写）；
/// `done` 是用户归档态（API 置位，不删行）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AlertRow {
    pub id: i64,
    /// 触发时刻（上海，YYYY-MM-DD HH:MM）
    pub ts: String,
    pub code: String,
    pub name: String,
    /// 封闭词汇（03 §12 判定表）：触及涨停/触及跌停/跌破成本/触及止损线/
    /// 触及止盈线/放量/强赎公告/临近到期
    #[serde(rename = "type")]
    pub alert_type: String,
    /// up/down/warn（§11：up/down 只用于价格方向；引擎不产出 flat）
    pub level: String,
    pub text: String,
    pub done: bool,
}

/// 新预警行（调度器判定后写入；ts 由调用方给上海时刻）
pub struct NewAlert {
    pub ts: String,
    pub code: String,
    pub name: String,
    pub alert_type: String,
    pub level: String,
    pub text: String,
}

const ALERT_COLS: &str = "id, ts, code, name, type, level, text, done";

fn map_alert_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AlertRow> {
    Ok(AlertRow {
        id: r.get(0)?,
        ts: r.get(1)?,
        code: r.get(2)?,
        name: r.get(3)?,
        alert_type: r.get(4)?,
        level: r.get(5)?,
        text: r.get(6)?,
        done: r.get::<_, i64>(7)? != 0,
    })
}

impl Store {
    pub fn insert_alert(&self, a: &NewAlert) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO alerts (ts, code, name, type, level, text, done)
             VALUES (?1,?2,?3,?4,?5,?6,0)",
            params![a.ts, a.code, a.name, a.alert_type, a.level, a.text],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 新在前（ts DESC, id DESC）；`done` 为 None 时不过滤。
    pub fn list_alerts(&self, done: Option<bool>, limit: i64) -> Result<Vec<AlertRow>> {
        let sql = if done.is_some() {
            format!("SELECT {ALERT_COLS} FROM alerts WHERE done = ?1 ORDER BY ts DESC, id DESC LIMIT ?2")
        } else {
            format!("SELECT {ALERT_COLS} FROM alerts ORDER BY ts DESC, id DESC LIMIT ?1")
        };
        let mut stmt = self.conn.prepare(&sql)?;
        // done 列是 INTEGER 0/1：显式转 i64 绑定，不依赖 bool 的 ToSql 行为
        let rows = match done {
            Some(d) => stmt.query_map(params![i64::from(d), limit], map_alert_row)?,
            None => stmt.query_map(params![limit], map_alert_row)?,
        };
        rows.collect::<Result<Vec<_>, _>>().context("读取预警失败")
    }

    /// 归档（done=1，不删行）。返回更新后的行；id 不存在 → None。
    pub fn mark_alert_done(&self, id: i64) -> Result<Option<AlertRow>> {
        let n = self
            .conn
            .execute("UPDATE alerts SET done = 1 WHERE id = ?1", params![id])?;
        if n == 0 {
            return Ok(None);
        }
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {ALERT_COLS} FROM alerts WHERE id = ?1"))?;
        let mut rows = stmt.query_map(params![id], map_alert_row)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// 同日 (code,type) 是否已有——调度器去重依据（03 §12：每自然日至多一条）。
    /// `date` 是上海日期 YYYY-MM-DD，按 ts 前缀匹配。
    pub fn alert_exists_on(&self, code: &str, alert_type: &str, date: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM alerts WHERE code = ?1 AND type = ?2 AND ts LIKE ?3",
            params![code, alert_type, format!("{date}%")],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }
}

// ============================================================================
// 净值快照（equity_snapshots，P4-T7：15:05 盘后链写入，03 §12）
// ============================================================================

/// 每交易日一行的组合快照。数字直接取 `recalc` 的 `Agg` 原值（不舍入，
/// 逐位可回溯）；`date` 主键 upsert——服务重启不重复（幂等）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EquitySnapshotRow {
    /// 交易日 YYYY-MM-DD（主键）
    pub date: String,
    /// 写入时刻（上海，YYYY-MM-DD HH:MM）
    pub ts: String,
    pub total_assets: f64,
    pub total_mv: f64,
    pub cash: f64,
    pub day_pnl: f64,
    pub day_pnl_pct: f64,
    pub total_pnl: f64,
    pub total_pnl_pct: f64,
    pub exposure: f64,
    pub hhi: f64,
    pub top3_sector: f64,
}

impl EquitySnapshotRow {
    /// 从 `recalc` 汇总拼装（数据搬运，不定义口径——列与 Agg 字段一一对应）
    pub fn from_agg(date: &str, ts: &str, agg: &Agg) -> Self {
        Self {
            date: date.to_string(),
            ts: ts.to_string(),
            total_assets: agg.total_assets,
            total_mv: agg.total_mv,
            cash: agg.cash,
            day_pnl: agg.day_pnl,
            day_pnl_pct: agg.day_pnl_pct,
            total_pnl: agg.total_pnl,
            total_pnl_pct: agg.total_pnl_pct,
            exposure: agg.exposure,
            hhi: agg.hhi,
            top3_sector: agg.top3_sector,
        }
    }
}

const SNAPSHOT_COLS: &str = "date, ts, total_assets, total_mv, cash, day_pnl, day_pnl_pct,
     total_pnl, total_pnl_pct, exposure, hhi, top3_sector";

fn map_snapshot_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<EquitySnapshotRow> {
    Ok(EquitySnapshotRow {
        date: r.get(0)?,
        ts: r.get(1)?,
        total_assets: r.get(2)?,
        total_mv: r.get(3)?,
        cash: r.get(4)?,
        day_pnl: r.get(5)?,
        day_pnl_pct: r.get(6)?,
        total_pnl: r.get(7)?,
        total_pnl_pct: r.get(8)?,
        exposure: r.get(9)?,
        hhi: r.get(10)?,
        top3_sector: r.get(11)?,
    })
}

impl Store {
    /// 幂等写入（INSERT OR REPLACE，date 主键）：同日重复跑盘后链不产生第二行。
    pub fn upsert_equity_snapshot(&self, row: &EquitySnapshotRow) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO equity_snapshots
               (date, ts, total_assets, total_mv, cash, day_pnl, day_pnl_pct,
                total_pnl, total_pnl_pct, exposure, hhi, top3_sector)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                row.date,
                row.ts,
                row.total_assets,
                row.total_mv,
                row.cash,
                row.day_pnl,
                row.day_pnl_pct,
                row.total_pnl,
                row.total_pnl_pct,
                row.exposure,
                row.hhi,
                row.top3_sector
            ],
        )?;
        Ok(())
    }

    pub fn equity_snapshot(&self, date: &str) -> Result<Option<EquitySnapshotRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SNAPSHOT_COLS} FROM equity_snapshots WHERE date = ?1"
        ))?;
        let mut rows = stmt.query_map(params![date], map_snapshot_row)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// date 升序（净值曲线的输入形态）；`limit` 取**最近** N 条（先倒序截断再反转）。
    pub fn equity_snapshots(&self, limit: i64) -> Result<Vec<EquitySnapshotRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SNAPSHOT_COLS} FROM equity_snapshots ORDER BY date DESC LIMIT ?1"
        ))?;
        let mut rows = stmt
            .query_map(params![limit], map_snapshot_row)?
            .collect::<Result<Vec<_>, _>>()
            .context("读取净值快照失败")?;
        rows.reverse();
        Ok(rows)
    }
}

// ============================================================================
// 推送日志（push_log，P4-T7：复盘触发/渠道推送的统一留痕）
// ============================================================================

/// 推送/触发留痕行。`ok=1` 只表示请求被受理（如复盘已注入 agent 会话），
/// 产出是否完成以会话事件与 advice_history 为准（03 §12 留档）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PushLogRow {
    pub id: i64,
    /// 上海时刻 YYYY-MM-DD HH:MM
    pub ts: String,
    /// agent / wechat / qq（P6 渠道落地后扩展）
    pub channel: String,
    /// post_market_review / alert_push / …（封闭词汇随渠道落地扩展）
    pub kind: String,
    pub ok: bool,
    pub detail: String,
}

impl Store {
    pub fn insert_push_log(
        &self,
        ts: &str,
        channel: &str,
        kind: &str,
        ok: bool,
        detail: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO push_log (ts, channel, kind, ok, detail) VALUES (?1,?2,?3,?4,?5)",
            params![ts, channel, kind, ok as i64, detail],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 新在前（ts DESC, id DESC）
    pub fn push_logs(&self, limit: i64) -> Result<Vec<PushLogRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, channel, kind, ok, detail FROM push_log
             ORDER BY ts DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(PushLogRow {
                id: r.get(0)?,
                ts: r.get(1)?,
                channel: r.get(2)?,
                kind: r.get(3)?,
                ok: r.get::<_, i64>(4)? != 0,
                detail: r.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().context("读取推送日志失败")
    }
}

// ============================================================================
// 自选（watchlist，B13 定稿 2026-10-29：schema v2；03 §12 判定范围扩到自选）
// ============================================================================

/// 自选行（观察标的，未持仓——无 shares/cost；预警判定按「无持仓形态」，
/// 见 03 §12：cost/stop/target_hi 不喂，序 3/4/5 不参与）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WatchlistRow {
    pub code: String,
    pub name: String,
    /// 'SH' | 'SZ'（显式字段，不按代码首字母猜——B7）
    pub mkt: String,
    pub kind: Kind,
    /// §2 比例是标的属性（ETF 无法由代码推断 → 数据层配置；同 holdings.limit_pct 语义）
    pub limit_pct: Option<i64>,
    pub sector: String,
    /// 观察备注（纯展示，不参与任何计算）
    pub note: String,
    /// 加入时刻（上海 YYYY-MM-DD HH:MM；upsert 不刷新）
    pub added_at: String,
}

/// 自选添加请求。**按 code upsert**：重复添加 = 更新名称/元信息/备注（幂等，不报错）。
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NewWatchlist {
    pub code: String,
    pub name: String,
    /// 'SH' | 'SZ'
    pub mkt: String,
    pub kind: Kind,
    #[serde(default)]
    pub limit_pct: Option<i64>,
    #[serde(default)]
    pub sector: String,
    #[serde(default)]
    pub note: String,
}

/// 自选录入拒绝原因（HTTP 层据此选状态码：BadInput → 422，Store → 500）
#[derive(Debug, Clone)]
pub enum WatchlistError {
    BadInput(String),
    Store(String),
}

impl std::fmt::Display for WatchlistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(m) => write!(f, "输入不合法：{m}"),
            Self::Store(m) => write!(f, "存储层：{m}"),
        }
    }
}

impl std::error::Error for WatchlistError {}

const WATCHLIST_COLS: &str = "code, name, mkt, kind, limit_pct, sector, note, added_at";

fn map_watchlist_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WatchlistRow> {
    let kind_s: String = r.get(3)?;
    Ok(WatchlistRow {
        code: r.get(0)?,
        name: r.get(1)?,
        mkt: r.get(2)?,
        // kind 有写入时的应用层校验 + CHECK 约束双保险；未知值（外改库）按 stock 容错不 panic
        kind: kind_from(&kind_s).unwrap_or(Kind::Stock),
        limit_pct: r.get(4)?,
        sector: r.get(5)?,
        note: r.get(6)?,
        added_at: r.get(7)?,
    })
}

impl Store {
    /// 添加/更新自选（按 code upsert 幂等；added_at 保留首次值）。
    /// 校验与新建持仓同款（B7 显式 mkt；名称非空）。
    pub fn add_watchlist(
        &self,
        w: &NewWatchlist,
        added_at: &str,
    ) -> std::result::Result<WatchlistRow, WatchlistError> {
        if w.code.trim().is_empty() {
            return Err(WatchlistError::BadInput("code 不能为空".into()));
        }
        if w.name.trim().is_empty() {
            return Err(WatchlistError::BadInput("name 不能为空".into()));
        }
        if w.mkt != "SH" && w.mkt != "SZ" {
            return Err(WatchlistError::BadInput(format!(
                "市场码非法：{}（应为 SH/SZ）",
                w.mkt
            )));
        }
        self.conn
            .execute(
                "INSERT INTO watchlist (code, name, mkt, kind, limit_pct, sector, note, added_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(code) DO UPDATE SET
                   name=excluded.name, mkt=excluded.mkt, kind=excluded.kind,
                   limit_pct=excluded.limit_pct, sector=excluded.sector, note=excluded.note",
                params![
                    w.code,
                    w.name,
                    w.mkt,
                    kind_str(w.kind),
                    w.limit_pct,
                    w.sector,
                    w.note,
                    added_at
                ],
            )
            .map_err(|e| WatchlistError::Store(e.to_string()))?;
        self.watchlist_row(&w.code)?
            .ok_or_else(|| WatchlistError::Store("写入后读回失败".into()))
    }

    /// 删除自选；返回 None = code 不存在（HTTP 404）。删除幂等（重复删不报错）。
    pub fn remove_watchlist(
        &self,
        code: &str,
    ) -> std::result::Result<Option<WatchlistRow>, WatchlistError> {
        let row = self.watchlist_row(code)?;
        if row.is_some() {
            self.conn
                .execute("DELETE FROM watchlist WHERE code = ?1", params![code])
                .map_err(|e| WatchlistError::Store(e.to_string()))?;
        }
        Ok(row)
    }

    /// 全量自选（加入时间序，稳定走查）。
    pub fn watchlist(&self) -> std::result::Result<Vec<WatchlistRow>, WatchlistError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {WATCHLIST_COLS} FROM watchlist ORDER BY added_at, code"
            ))
            .map_err(|e| WatchlistError::Store(e.to_string()))?;
        let rows = stmt
            .query_map([], map_watchlist_row)
            .map_err(|e| WatchlistError::Store(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| WatchlistError::Store(e.to_string()))
    }

    fn watchlist_row(
        &self,
        code: &str,
    ) -> std::result::Result<Option<WatchlistRow>, WatchlistError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {WATCHLIST_COLS} FROM watchlist WHERE code = ?1"
            ))
            .map_err(|e| WatchlistError::Store(e.to_string()))?;
        let mut rows = stmt
            .query_map(params![code], map_watchlist_row)
            .map_err(|e| WatchlistError::Store(e.to_string()))?;
        match rows.next() {
            Some(r) => Ok(Some(r.map_err(|e| WatchlistError::Store(e.to_string()))?)),
            None => Ok(None),
        }
    }
}

// ============================================================================
// 持仓直接维护（ADR-0009 / 03 §13 定稿 2026-11-01：快照校正，schema v3）
// ============================================================================

/// 维护操作类型（`holding_edits.op`，DB CHECK 同款小写串）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EditOp {
    Create,
    Update,
    Delete,
}

impl EditOp {
    fn as_str(self) -> &'static str {
        match self {
            EditOp::Create => "create",
            EditOp::Update => "update",
            EditOp::Delete => "delete",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "create" => EditOp::Create,
            "update" => EditOp::Update,
            "delete" => EditOp::Delete,
            other => anyhow::bail!("未知维护 op: {other}"),
        })
    }
}

/// 持仓行**可编辑字段**快照（`holding_edits` 的 before/after）。
/// price/prev 是行情字段（刷新链回写，03 §13 不可编辑）——不进快照。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HoldingSnapshot {
    pub code: String,
    pub name: String,
    /// 'SH' | 'SZ'
    pub mkt: String,
    pub kind: Kind,
    pub sector: String,
    pub shares: i64,
    pub cost: f64,
    pub limit_pct: Option<i64>,
}

impl HoldingSnapshot {
    fn of(h: &HoldingRow) -> Self {
        HoldingSnapshot {
            code: h.code.clone(),
            name: h.name.clone(),
            mkt: h.mkt.clone(),
            kind: h.kind,
            sector: h.sector.clone(),
            shares: h.shares,
            cost: h.cost,
            limit_pct: h.limit_pct,
        }
    }
}

/// 维护留痕行（`GET /api/holding_edits`「维护记录」卡数据源）。
/// create → before 为 null；delete → after 为 null。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HoldingEditRow {
    pub id: i64,
    /// 上海 YYYY-MM-DD HH:MM（与 trades.ts 同口径）
    pub ts: String,
    pub op: EditOp,
    pub code: String,
    /// 操作时刻的标的名称（持仓行删除后仍可展示）
    pub name: String,
    pub before: Option<HoldingSnapshot>,
    pub after: Option<HoldingSnapshot>,
    pub note: String,
}

/// 直接维护入参（整行覆盖 upsert，「看到什么抄什么」）。
/// 校验见 03 §13：允许奇零股、跳过 lot 与涨跌停（校正不是委托）。
#[derive(Debug, Clone)]
pub struct HoldingUpsert {
    pub code: String,
    pub name: String,
    /// 'SH' | 'SZ'（显式字段，B7）
    pub mkt: String,
    pub kind: Kind,
    pub sector: String,
    /// ≥ 1 任意整数（红利零股/转股零股真实存在，B12 卖出侧豁免承接）
    pub shares: i64,
    /// 券商摊薄成本口径（03 §4），≥ 0；调整不产生已实现盈亏
    pub cost: f64,
    /// None = 新建 NULL、更新保留原值（03 §13 / §2 配置语义）
    pub limit_pct: Option<i64>,
    /// 维护备注（只进 holding_edits 留痕，不进持仓行）
    pub note: String,
}

/// 直接维护拒绝原因（HTTP 层映射：BadInput → 400、Store → 500，watchlist 同款）
#[derive(Debug, Clone)]
pub enum HoldingEditError {
    BadInput(String),
    Store(String),
}

impl std::fmt::Display for HoldingEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(m) => write!(f, "输入不合法：{m}"),
            Self::Store(m) => write!(f, "存储层：{m}"),
        }
    }
}

impl std::error::Error for HoldingEditError {}

const EDITS_COLS: &str = "id, ts, op, code, name, before_json, after_json, note";

fn row_conv(e: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
}

fn map_edit_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<HoldingEditRow> {
    let op_s: String = r.get(2)?;
    let bj: Option<String> = r.get(5)?;
    let aj: Option<String> = r.get(6)?;
    let parse = |s: String| serde_json::from_str::<HoldingSnapshot>(&s).map_err(row_conv);
    Ok(HoldingEditRow {
        id: r.get(0)?,
        ts: r.get(1)?,
        op: EditOp::parse(&op_s).map_err(row_conv)?,
        code: r.get(3)?,
        name: r.get(4)?,
        before: bj.map(parse).transpose()?,
        after: aj.map(parse).transpose()?,
        note: r.get(7)?,
    })
}

impl Store {
    /// 直接维护 upsert（03 §13）：整行覆盖 + 审计留痕**同事务**。
    /// 不动现金、不写 trades、不改 price/prev（行情字段归刷新链，SET 子句不含它们）。
    /// 幂等：可编辑字段与现有行逐位一致 → 不写库不留痕，返回 `(现有行, false)`；
    /// 有变更返回 `(写入后的行, true)`。
    pub fn upsert_holding(
        &self,
        req: &HoldingUpsert,
        ts: &str,
    ) -> std::result::Result<(HoldingRow, bool), HoldingEditError> {
        self.validate_upsert(req)?;
        let existing = self
            .holding_of(&req.code)
            .map_err(|e| HoldingEditError::Store(e.to_string()))?;

        // limit_pct None：新建 NULL、更新保留原值（03 §13）
        let eff_limit = req
            .limit_pct
            .or_else(|| existing.as_ref().and_then(|h| h.limit_pct));

        // 幂等判定：可编辑字段逐位一致 → no-op（f64 用位相等——同一表单重提交）
        if let Some(h) = &existing {
            if h.name == req.name
                && h.mkt == req.mkt
                && h.kind == req.kind
                && h.sector == req.sector
                && h.shares == req.shares
                && h.cost == req.cost
                && h.limit_pct == eff_limit
            {
                return Ok((h.clone(), false));
            }
        }

        let op = if existing.is_some() {
            EditOp::Update
        } else {
            EditOp::Create
        };
        let before = existing.as_ref().map(HoldingSnapshot::of);

        // 事务：持仓写入 + 留痕同生共死（record_trade 同款 BEGIN IMMEDIATE 模式）
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| HoldingEditError::Store(format!("事务开启失败：{e}")))?;
        let result = (|| -> std::result::Result<HoldingRow, HoldingEditError> {
            self.conn
                .execute(
                    "INSERT INTO holdings (code,name,mkt,kind,sector,shares,cost,price,prev,limit_pct,updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,NULL,?8,?9)
                     ON CONFLICT(code) DO UPDATE SET
                       name=?2, mkt=?3, kind=?4, sector=?5, shares=?6, cost=?7,
                       limit_pct=?8, updated_at=?9",
                    params![
                        req.code, req.name, req.mkt, kind_str(req.kind), req.sector,
                        req.shares, req.cost, eff_limit, ts
                    ],
                )
                .map_err(|e| HoldingEditError::Store(format!("写入持仓失败：{e}")))?;
            let row = self
                .holding_of(&req.code)
                .map_err(|e| HoldingEditError::Store(e.to_string()))?
                .ok_or_else(|| HoldingEditError::Store("写入后读回失败".into()))?;
            self.insert_edit(
                op,
                &req.code,
                &req.name,
                before,
                Some(HoldingSnapshot::of(&row)),
                &req.note,
                ts,
            )?;
            Ok(row)
        })();
        match result {
            Ok(row) => {
                self.conn
                    .execute_batch("COMMIT")
                    .map_err(|e| HoldingEditError::Store(format!("提交失败：{e}")))?;
                Ok((row, true))
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }

    /// 删除持仓行（03 §13）：整行移除 + delete 留痕同事务。
    /// `trades` 历史保留（holding_code 非外键）、累计已实现不回滚、现金不动。
    /// 返回 None = code 不存在（HTTP 404）；重复删同样 None（不报错）。
    pub fn remove_holding(
        &self,
        code: &str,
        ts: &str,
    ) -> std::result::Result<Option<HoldingRow>, HoldingEditError> {
        let row = self
            .holding_of(code)
            .map_err(|e| HoldingEditError::Store(e.to_string()))?;
        let Some(row) = row else {
            return Ok(None);
        };
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| HoldingEditError::Store(format!("事务开启失败：{e}")))?;
        let result = (|| -> std::result::Result<(), HoldingEditError> {
            self.conn
                .execute("DELETE FROM holdings WHERE code = ?1", params![code])
                .map_err(|e| HoldingEditError::Store(format!("删除持仓失败：{e}")))?;
            self.insert_edit(
                EditOp::Delete,
                code,
                &row.name,
                Some(HoldingSnapshot::of(&row)),
                None,
                "",
                ts,
            )
        })();
        match result {
            Ok(()) => {
                self.conn
                    .execute_batch("COMMIT")
                    .map_err(|e| HoldingEditError::Store(format!("提交失败：{e}")))?;
                Ok(Some(row))
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }

    /// 维护留痕（id 降序 = 最近在前；「维护记录」卡数据源）
    pub fn holding_edits(
        &self,
        limit: i64,
    ) -> std::result::Result<Vec<HoldingEditRow>, HoldingEditError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {EDITS_COLS} FROM holding_edits ORDER BY id DESC LIMIT ?1"
            ))
            .map_err(|e| HoldingEditError::Store(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit], map_edit_row)
            .map_err(|e| HoldingEditError::Store(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| HoldingEditError::Store(e.to_string()))
    }

    /// 03 §13 校验：6 位数字代码、SH/SZ、名称非空、shares ≥ 1（允许奇零）、
    /// cost ≥ 0、limit_pct 为正。**不套 lot 与涨跌停**（校正不是委托）。
    fn validate_upsert(&self, req: &HoldingUpsert) -> std::result::Result<(), HoldingEditError> {
        let bad = |m: String| Err(HoldingEditError::BadInput(m));
        if req.code.len() != 6 || !req.code.bytes().all(|b| b.is_ascii_digit()) {
            return bad(format!("代码需为 6 位数字（当前 {}）", req.code));
        }
        if req.name.trim().is_empty() {
            return bad("name 不能为空".into());
        }
        if req.mkt != "SH" && req.mkt != "SZ" {
            return bad(format!("市场码非法：{}（应为 SH/SZ）", req.mkt));
        }
        if req.shares < 1 {
            return bad(format!("shares 需 ≥ 1（允许奇零股；当前 {}）", req.shares));
        }
        // 否定式是有意的 NaN 守卫（同 preview 的 price 校验惯例）
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(req.cost >= 0.0) {
            return bad("cost 不能为负数或 NaN".into());
        }
        if let Some(p) = req.limit_pct {
            if p <= 0 {
                return bad(format!("limit_pct 需为正数（当前 {p}）"));
            }
        }
        Ok(())
    }

    fn insert_edit(
        &self,
        op: EditOp,
        code: &str,
        name: &str,
        before: Option<HoldingSnapshot>,
        after: Option<HoldingSnapshot>,
        note: &str,
        ts: &str,
    ) -> std::result::Result<(), HoldingEditError> {
        let enc = |s: Option<HoldingSnapshot>| -> std::result::Result<Option<String>, HoldingEditError> {
            s.map(|v| {
                serde_json::to_string(&v)
                    .map_err(|e| HoldingEditError::Store(format!("维护快照序列化失败：{e}")))
            })
            .transpose()
        };
        self.conn
            .execute(
                "INSERT INTO holding_edits (ts, op, code, name, before_json, after_json, note)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![ts, op.as_str(), code, name, enc(before)?, enc(after)?, note],
            )
            .map_err(|e| HoldingEditError::Store(format!("写入维护留痕失败：{e}")))?;
        Ok(())
    }
}

// ============================================================================
// 会话档案与通道绑定（chat_messages / channel_bindings，schema v4，T2/ADR-0011）
// ============================================================================

/// Web 会话档案行。`role='tool'` 行只供展示（`GET /api/chat/history`）；
/// 复活回灌只取 user/assistant（陈旧数字不进上下文，03「派生数字现算」）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ChatMessageRow {
    pub id: i64,
    /// 会话键（通道独立：Web 恒 'web'；IM 会话历史在 robit 自持库，不在本表）
    pub session: String,
    /// 'user' | 'assistant' | 'tool'
    pub role: String,
    pub text: String,
    /// role='tool' 的展示快照 {name,args,result,is_error}；其余为 null
    pub tool_json: Option<String>,
    /// 上海时刻 YYYY-MM-DD HH:MM（与 push_log/trades 同格式）
    pub ts: String,
}

/// IM 入站绑定（白名单行）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ChannelBindingRow {
    pub channel: String,
    pub chat_id: String,
    /// 'private' | 'group'（对齐 robit ChatType）
    pub kind: String,
    pub label: String,
    pub bound_at: String,
    pub active: bool,
}

impl Store {
    /// 追加一条会话档案行，返回行 id。role 词汇由 CHECK 约束把关（user/assistant/tool）。
    pub fn append_chat_message(
        &self,
        session: &str,
        role: &str,
        text: &str,
        tool_json: Option<&str>,
        ts: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO chat_messages (session, role, text, tool_json, ts) VALUES (?1,?2,?3,?4,?5)",
            params![session, role, text, tool_json, ts],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 按会话读档案：**新在前**（id DESC，分页语义与 push_logs 同族）；
    /// `before_id` 取该 id 之前（更早）的页；第二返回值 `has_more` = 是否还有更早的。
    /// 返回顺序是"从新到旧"，需要时间正序的调用方（复活回灌）自行 reverse。
    pub fn chat_messages(
        &self,
        session: &str,
        limit: i64,
        before_id: Option<i64>,
    ) -> Result<(Vec<ChatMessageRow>, bool)> {
        let sql = match before_id {
            Some(_) =>
                "SELECT id, session, role, text, tool_json, ts FROM chat_messages
                 WHERE session = ?2 AND id < ?3 ORDER BY id DESC LIMIT ?1",
            None =>
                "SELECT id, session, role, text, tool_json, ts FROM chat_messages
                 WHERE session = ?1 ORDER BY id DESC LIMIT ?2",
        };
        let map_row = |r: &rusqlite::Row| -> rusqlite::Result<ChatMessageRow> {
            Ok(ChatMessageRow {
                id: r.get(0)?,
                session: r.get(1)?,
                role: r.get(2)?,
                text: r.get(3)?,
                tool_json: r.get(4)?,
                ts: r.get(5)?,
            })
        };
        // 多取一条探页尾（has_more），返回时裁掉；饱和加避免 i64::MAX 全量取时溢出
        let probe = limit.saturating_add(1);
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match before_id {
            Some(bid) => stmt
                .query_map(params![probe, session, bid], map_row)?
                .collect::<Result<Vec<_>, _>>()?,
            None => stmt
                .query_map(params![session, probe], map_row)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        let has_more = rows.len() as i64 > limit;
        let mut rows = rows;
        if has_more {
            rows.pop();
        }
        Ok((rows, has_more))
    }

    /// 清空会话档案（Web 的 /new 语义由上层组合：先清档案再给 agent 发 "/clear"）。
    pub fn clear_chat_messages(&self, session: &str) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM chat_messages WHERE session = ?1", params![session])?)
    }

    /// 绑定/重绑：同 (channel, chat_id) 幂等 upsert——重新绑定会刷新 kind/label/bound_at
    /// 并把 active 拉回 1（unbind 过的老 id 重绑不留双行）。
    pub fn bind_channel(
        &self,
        channel: &str,
        chat_id: &str,
        kind: &str,
        label: &str,
        bound_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO channel_bindings (channel, chat_id, kind, label, bound_at, active)
             VALUES (?1,?2,?3,?4,?5,1)
             ON CONFLICT(channel, chat_id) DO UPDATE
               SET kind = excluded.kind, label = excluded.label,
                   bound_at = excluded.bound_at, active = 1",
            params![channel, chat_id, kind, label, bound_at],
        )?;
        Ok(())
    }

    /// 解绑 = 置 active=0 留痕（DELETE 会断推送审计的对应关系）。返回是否命中活跃行。
    pub fn unbind_channel(&self, channel: &str, chat_id: &str) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE channel_bindings SET active = 0
             WHERE channel = ?1 AND chat_id = ?2 AND active = 1",
            params![channel, chat_id],
        )?;
        Ok(n > 0)
    }

    /// 当前活跃绑定（BoundOnly decorator 的判定面与 GET 列表端点共用）。
    /// `channel` 为 None 时返回全部通道。bound_at 升序（先绑在前）。
    pub fn channel_bindings(&self, channel: Option<&str>) -> Result<Vec<ChannelBindingRow>> {
        let mut stmt = match channel {
            Some(_) => self.conn.prepare(
                "SELECT channel, chat_id, kind, label, bound_at, active FROM channel_bindings
                 WHERE active = 1 AND channel = ?1 ORDER BY bound_at ASC",
            )?,
            None => self.conn.prepare(
                "SELECT channel, chat_id, kind, label, bound_at, active FROM channel_bindings
                 WHERE active = 1 ORDER BY bound_at ASC",
            )?,
        };
        let map = |r: &rusqlite::Row| -> rusqlite::Result<ChannelBindingRow> {
            Ok(ChannelBindingRow {
                channel: r.get(0)?,
                chat_id: r.get(1)?,
                kind: r.get(2)?,
                label: r.get(3)?,
                bound_at: r.get(4)?,
                active: r.get::<_, i64>(5)? != 0,
            })
        };
        let rows = match channel {
            Some(c) => stmt.query_map(params![c], map)?,
            None => stmt.query_map([], map)?,
        };
        rows.collect::<Result<Vec<_>, _>>().context("读取通道绑定失败")
    }

    /// 单点判定（白名单热路径）：(channel, chat_id) 是否活跃绑定。
    pub fn is_channel_bound(&self, channel: &str, chat_id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM channel_bindings
             WHERE channel = ?1 AND chat_id = ?2 AND active = 1",
            params![channel, chat_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }
}
