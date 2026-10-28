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
use zimu_domain::dilution::{apply_trade, ApplyTradeInput, TradeOutcome, SINGLE_STOCK_LIMIT};
use zimu_domain::fees::{fee, FeeBreakdown, FeeInput};
use zimu_domain::lot::{lot_size, price_decimals, validate_qty, QtyReason};
use zimu_domain::price_limit::{limit_prices, validate_price};
use zimu_domain::recalc::{recalc, Agg, Holding, PerHolding, PortfolioInput};
use zimu_domain::{Kind, Side};

/// schema 版本（PRAGMA user_version）。新增迁移在 `MIGRATIONS` 尾部追加，
/// 永不修改已发布版本的 SQL——迁移只前进（P4-T2 迁移机制约定）。
pub const CURRENT_VERSION: i64 = 1;

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
)];

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Serialize)]
pub struct SimPreview {
    pub fee_detail: FeeBreakdown,
    pub outcome: TradeOutcome,
    /// 非拦截性提示：如「单票将超过 25% 上限」（原型只在预览提示，不拦提交）
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
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

    /// 组合全景：存储行 + `zimu-domain::recalc` 的逐笔派生与汇总（03 §5 口径）。
    /// price/prev 缺失时按「无行情」降级：price 暂以成本占位（不产生假涨跌），
    /// prev 置 0 → dayChg/dayVal 为 0。
    pub fn portfolio(&self) -> Result<PortfolioView> {
        let holdings = self.holdings()?;
        let account = self.account()?;
        let inp = PortfolioInput {
            cash: account.cash,
            holdings: holdings
                .iter()
                .map(|h| Holding {
                    code: h.code.clone(),
                    kind: h.kind,
                    sector: h.sector.clone(),
                    shares: h.shares as f64,
                    cost: h.cost,
                    price: h.price.unwrap_or(h.cost),
                    prev: h.prev.unwrap_or(0.0),
                })
                .collect(),
        };
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

#[derive(Debug, Serialize)]
pub struct PortfolioView {
    pub account: AccountRow,
    pub holdings: Vec<HoldingRow>,
    /// 与 holdings 同序对齐（recalc 输出按输入序）
    pub per: Vec<PerHolding>,
    pub agg: Agg,
}
