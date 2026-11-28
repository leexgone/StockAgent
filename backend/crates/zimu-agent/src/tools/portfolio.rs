//! 持仓与账本工具：get_holdings（组合全景）、simulate_trade（试算不落库）+
//! 写账本三件（ADR-0012）：record_trade（成交录入，与界面
//! `POST /api/holdings/trade` 同一 store 事务）、adjust_cash（现金绝对值校正）、
//! get_trades（流水只读）。数字全部出自 `store.*` → `zimu-domain`
//! （recalc/fees/dilution/lot），本层零口径。
//! ADR-0024：calc_discipline（纪律四条体检 + reduce_hint）整体退役——仓位纪律
//! 只保留 Web 端测算展示（红绿灯卡/再平衡清单，前端对 /api/holdings 原值现算）；
//! 组合中性事实（weight/行业分布/HHI/现金占比）仍在 get_holdings 里照常可得。

use std::sync::Arc;

use async_trait::async_trait;
use robit_agent::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use zimu_store::{HoldingEditError, HoldingUpsert, RecordError, TradeRequest};

use super::{ok_json, store_err, store_try};
use crate::AgentDeps;

// ============================================================================
// get_holdings
// ============================================================================

pub struct GetHoldingsTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for GetHoldingsTool {
    fn name(&self) -> &str {
        "get_holdings"
    }

    fn description(&self) -> &str {
        "查组合全景（03 §5 口径，数字与界面 /api/holdings 同源）：账户现金、持仓明细（代码/名称/类型/行业/股数/摊薄成本/现价/昨收）、逐笔派生 per[]（当日涨跌 day_chg、当日盈亏 day_val、市值 mv、累计盈亏 pnl/pnl_pct、占总资产 weight）、组合汇总 agg（总资产、当日/累计盈亏与百分比、仓位 exposure、行业分布、HHI 集中度）。注意价格是最近一次行情回写的值——需要最新数字先调 refresh_quotes。回答持仓/盈亏必须引用本工具返回的数字。"
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let s = crate::lock(&self.deps.store);
        let view = store_try!(s.portfolio());
        drop(s);
        // ADR-0024：返回不再携带 discipline 块——组合中性事实照常（per/agg 原值），
        // 仓位纪律的阈值判定与降仓测算退居 Web 端（红绿灯卡/再平衡清单现算）。
        Ok(ok_json(&json!({
            "portfolio": view,
            "note": "weight 分母是总资产（含现金），行业占比分母是持仓市值（不含现金）——文案里数字旁必须写清分母（03 §5）",
        })))
    }
}

// ============================================================================
// simulate_trade
// ============================================================================

pub struct SimulateTradeTool {
    pub deps: Arc<AgentDeps>,
}

/// RecordError → 面向对话的拒绝文本（带结构化细节，数字与 HTTP 422 契约同源）
fn record_err(e: RecordError) -> ToolResult {
    let detail = match &e {
        RecordError::OutOfLimitRange {
            price,
            lo,
            hi,
            prec,
        } => Some(json!({
            "price": price, "limit_down": lo, "limit_up": hi, "decimals": prec,
        })),
        RecordError::InsufficientCash { cash_after } => Some(json!({ "cash_after": cash_after })),
        _ => None,
    };
    let mut content = format!("委托被规则拒绝：{e}");
    if let Some(d) = detail {
        content.push_str(&format!("\n细节：{}", d));
    }
    ToolResult::error(content)
}

#[async_trait]
impl Tool for SimulateTradeTool {
    fn name(&self) -> &str {
        "simulate_trade"
    }

    fn description(&self) -> &str {
        "试算一笔成交（只读预览，不落库——确认要点无误后调 record_trade 落账）：整手与可卖数量校验（03 §1）、涨跌停区间校验（§2，有昨收才拦）、费用三分项（§3：佣金/印花税/过户费，A 股口径）、摊薄成本或已实现盈亏与现金变化（§4）、超 25% 上限等非拦截性警告。参数：code、side（buy/sell）、qty 股数、price 成交价、note 可选；标的未持有且要买入时必须给 meta {name, mkt: SH|SZ, kind: stock|etf|cb, sector, prev?, limit_pct?}。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "标的代码，如 600519" },
                "side": { "type": "string", "enum": ["buy", "sell"], "description": "交易方向" },
                "qty": { "type": "integer", "description": "股数/张数（整手校验见 03 §1）" },
                "price": { "type": "number", "description": "成交价（涨跌停校验见 03 §2）" },
                "note": { "type": "string", "description": "备注（可选）" },
                "meta": {
                    "type": "object",
                    "description": "首笔建仓必带：{name, mkt: SH|SZ, kind: stock|etf|cb, sector, prev?, limit_pct?}",
                    "properties": {
                        "name": { "type": "string" },
                        "mkt": { "type": "string", "enum": ["SH", "SZ"] },
                        "kind": { "type": "string", "enum": ["stock", "etf", "cb"] },
                        "sector": { "type": "string" },
                        "prev": { "type": ["number", "null"] },
                        "limit_pct": { "type": ["integer", "null"] }
                    },
                    "required": ["name", "mkt", "kind"]
                }
            },
            "required": ["code", "side", "qty", "price"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let req: TradeRequest = match serde_json::from_value(args) {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "参数不符：{e}。需要 code、side(buy/sell)、qty 整数股、price 委托价；首笔建仓另需 meta{{name,mkt,kind,…}}"
                )))
            }
        };
        let preview = match crate::lock(&self.deps.store).simulate_trade(&req) {
            Ok(p) => p,
            Err(e) => return Ok(record_err(e)),
        };
        Ok(ok_json(&json!({
            "preview": preview,
            "note": "试算预览，未落库。费用/摊薄/整手/涨跌停数字与录入抽屉同一套领域函数（03 §1–§4），可直接引用；向用户复述要点、确认后调 record_trade 落账（ADR-0012）",
        })))
    }
}

// ============================================================================
// record_trade（写账本，ADR-0012：自动落账 + 读回，流水只增不改）
// ============================================================================

pub struct RecordTradeTool {
    pub deps: Arc<AgentDeps>,
}

/// 来源标记：流水是混合来源（界面 / 智能体 / 将来的 CSV），note 是唯一
/// 免费留痕位（TradeRow.note 入库并进 API/前端展示，ADR-0012）
const AGENT_NOTE_MARK: &str = "（经智能体录入）";

#[async_trait]
impl Tool for RecordTradeTool {
    fn name(&self) -> &str {
        "record_trade"
    }

    fn description(&self) -> &str {
        "把一笔**已成交**的买卖记入账户流水（写账本：trades 流水 + 持仓变动 + 现金/费用/已实现盈亏联动，03 §1–§4 全量校验，与界面录入抽屉同一事务）。本工具是记账不是执行——系统不连接任何交易系统。写账本三纪律（ADR-0012）：① 数量价格必须来自用户明示，缺任何一项先问，禁止用行情价、记忆或猜测补数；② 先调 simulate_trade 试算并向用户复述要点，再调本工具落账，落账后把收据完整宣读（时间/标的/方向/数量/价格/费用三分项/变动后现金与持仓）；③ 账本只增不改——录错先 get_trades 定位原笔，向用户说明后反向一笔冲正（note 注明「冲正 #原id」），绝不承诺删除重记。参数：code、side（buy/sell）、qty 股数、price 成交价、note 可选；标的未持有且要买入时必须给 meta {name, mkt: SH|SZ, kind: stock|etf|cb, sector, prev?, limit_pct?}。"
    }

    fn parameters_schema(&self) -> Value {
        // 与 simulate_trade 逐字段同形（同一个 TradeRequest 反序列化路径）
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "标的代码，如 600519" },
                "side": { "type": "string", "enum": ["buy", "sell"], "description": "交易方向" },
                "qty": { "type": "integer", "description": "股数/张数（整手校验见 03 §1）" },
                "price": { "type": "number", "description": "成交价（涨跌停校验见 03 §2）" },
                "note": { "type": "string", "description": "备注（可选；工具会自动追加来源标记）" },
                "meta": {
                    "type": "object",
                    "description": "首笔建仓必带：{name, mkt: SH|SZ, kind: stock|etf|cb, sector, prev?, limit_pct?}",
                    "properties": {
                        "name": { "type": "string" },
                        "mkt": { "type": "string", "enum": ["SH", "SZ"] },
                        "kind": { "type": "string", "enum": ["stock", "etf", "cb"] },
                        "sector": { "type": "string" },
                        "prev": { "type": ["number", "null"] },
                        "limit_pct": { "type": ["integer", "null"] }
                    },
                    "required": ["name", "mkt", "kind"]
                }
            },
            "required": ["code", "side", "qty", "price"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        // ADR-0012 拍板：自动落账 + 读回（不启用逐笔确认）。确认门禁机制在
        // 三端健在（Web POST /api/chat/confirm、QQ Confirmer 关键词、CLI [Y/n]），
        // 翻回只需此处改 true——但启用前先补 ADR-0012 被否决项 B 的两个已知洞
        // （Web confirm 无超时挂起面、QQ 关键词全等匹配的注入面）。
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let mut req: TradeRequest = match serde_json::from_value(args) {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "参数不符：{e}。需要 code、side(buy/sell)、qty 整数股、price 成交价；首笔建仓另需 meta{{name,mkt,kind,…}}"
                )))
            }
        };
        // 来源标记：工具层强制追加，不依赖模型自觉（审计面，ADR-0012）
        req.note = if req.note.trim().is_empty() {
            AGENT_NOTE_MARK.to_string()
        } else {
            format!("{}{}", req.note.trim(), AGENT_NOTE_MARK)
        };
        let receipt = match crate::lock(&self.deps.store).record_trade(&req) {
            Ok(r) => r,
            Err(e) => return Ok(record_err(e)),
        };
        Ok(ok_json(&json!({
            "receipt": receipt,
            "note": "已落账（自动落账 + 读回，ADR-0012）。请立即向用户完整宣读收据：时间、标的、方向、数量、价格、费用三分项（佣金/印花税/过户费）、现金变动、变动后持仓与现金余额，请用户当场核对。账本只增不改；录错用反向一笔冲正。",
        })))
    }
}

// ============================================================================
// adjust_cash（写账本：现金侧快照校正，绝对值覆盖）
// ============================================================================

pub struct AdjustCashTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for AdjustCashTool {
    fn name(&self) -> &str {
        "adjust_cash"
    }

    fn description(&self) -> &str {
        "把可用现金校正到用户明示的数值（现金侧快照校正：绝对值覆盖，不写流水、不动期初资金，与界面「可用现金 · 调整」同一语义——ADR-0009 现金单一校正入口）。纪律：数字必须来自用户明示（对账单/截图读数也要向用户复述确认），缺项先问；禁止传增量/delta（工具只收校正后的绝对值）。负现金拒绝。返回 before/after/delta（delta 由后端计算）——落账后向用户宣读调整前后两个数。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cash": { "type": "number", "description": "校正后的可用现金绝对值（元，≥0）" },
                "reason": { "type": "string", "description": "校正原因（可选，回显在返回里）" }
            },
            "required": ["cash"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        false // ADR-0012 自动落账 + 读回（同 record_trade 注释）
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let Some(cash) = args.get("cash").and_then(Value::as_f64) else {
            return Ok(ToolResult::error(
                "参数不符：需要 cash（校正后的可用现金绝对值，数字 ≥0）".to_string(),
            ));
        };
        let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");
        let s = crate::lock(&self.deps.store);
        let before = store_try!(s.account()).cash;
        let updated = match s.update_account(None, None, None, None, Some(cash)) {
            Ok(a) => a,
            Err(e) => return Ok(record_err(e)),
        };
        drop(s);
        Ok(ok_json(&json!({
            "before": before,
            "after": updated.cash,
            "delta": updated.cash - before,
            "reason": reason,
            "note": "已落账。向用户宣读：调整前后两个数（before → after）与差额；现金校正不写流水、不动期初资金（ADR-0009 现金单一校正入口）。",
        })))
    }
}

// ============================================================================
// get_trades（只读：成交流水账本）
// ============================================================================

pub struct GetTradesTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for GetTradesTool {
    fn name(&self) -> &str {
        "get_trades"
    }

    fn description(&self) -> &str {
        "查成交流水（最新在前；账本只增不改）。用途：核对既往成交、定位要冲正的原笔（冲正前必须先查原笔的 id/code/side/qty/price）。每条含时间、标的、方向、数量、价格、金额、费用三分项、现金变动、已实现盈亏、备注（备注尾「（经智能体录入）」= 智能体写入的流水）。limit 缺省 20、上限 50——对话侧省 context，与 HTTP 端 /api/trades 的 50/500 有意不同；全量历史看界面流水卡。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "返回条数上限（缺省 20，上限 50）" }
            }
        })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let limit = args
            .get("limit")
            .and_then(Value::as_i64)
            .unwrap_or(20)
            .clamp(1, 50);
        let rows = store_try!(crate::lock(&self.deps.store).trades(limit));
        let count = rows.len();
        Ok(ok_json(&json!({ "trades": rows, "count": count, "limit": limit })))
    }
}

// ============================================================================
// upsert_holding / remove_holding（写账本：持仓快照校正，ADR-0009/0013）
// ============================================================================

/// 维护留痕与校正写入的 ts 用上海时刻（与 holdings_api::now_sh 同口径）
fn now_ts() -> String {
    zimu_market::clock::now_shanghai()
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// HoldingEditError → 面向对话文本：BadInput 校验细节如实透传（模型可转述纠正），
/// Store 走脱敏通道（细节进日志）
fn holding_edit_err(e: HoldingEditError) -> ToolResult {
    match e {
        HoldingEditError::BadInput(_) => ToolResult::error(format!("持仓校正被拒：{e}")),
        HoldingEditError::Store(_) => store_err(e),
    }
}

/// 工具入参中转（`HoldingUpsert` 无 Deserialize derive，照 holdings_api 的
/// `HoldingEditRequest` 同款形状；code 是字段不是路径——工具面没有 URL）
#[derive(serde::Deserialize)]
struct UpsertArgs {
    code: String,
    name: String,
    /// 'SH' | 'SZ'（显式字段不按代码猜，B7）
    mkt: String,
    kind: zimu_domain::Kind,
    #[serde(default)]
    sector: String,
    shares: i64,
    cost: f64,
    #[serde(default)]
    limit_pct: Option<i64>,
    #[serde(default)]
    note: String,
}

pub struct UpsertHoldingTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for UpsertHoldingTool {
    fn name(&self) -> &str {
        "upsert_holding"
    }

    fn description(&self) -> &str {
        "直接维护持仓行（快照校正，ADR-0009/0013）：初始化映射（券商存量持仓搬入）、定期校准漂移、纠错、场外变动（送股零股/退市/债转股）。**整行覆盖**语义——先调 get_holdings 读现值，只改用户明示的字段，其余字段原样带回（sector 留空会清掉行业）；数字必须来自用户明示（券商界面/对账单读数），缺项先问不猜。校验刻意宽松（03 §13：shares≥1 允许奇零股、跳过 lot 与涨跌停——校正不是委托）；price/prev 行情字段不可改（归刷新链）。三不动：现金、trades 流水、已实现盈亏均不变（现金校正另走 adjust_cash）。审计痕自动写 holding_edits（before/after 行快照 + note 来源标记，同事务）。幂等：与现值逐位一致则不写库不留痕。参数：code、name、mkt(SH|SZ)、kind(stock|etf|cb)、shares、cost 必填；sector、limit_pct、note 可选。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "标的代码（6 位数字）" },
                "name": { "type": "string", "description": "标的名称（非空）" },
                "mkt": { "type": "string", "enum": ["SH", "SZ"], "description": "市场（显式给出，不按代码猜）" },
                "kind": { "type": "string", "enum": ["stock", "etf", "cb"], "description": "标的类型" },
                "sector": { "type": "string", "description": "行业（整行覆盖：不改也要原样带回）" },
                "shares": { "type": "integer", "description": "股数/张数（≥1 任意整数，允许奇零股）" },
                "cost": { "type": "number", "description": "券商摊薄成本（≥0；调整不产生已实现盈亏）" },
                "limit_pct": { "type": ["integer", "null"], "description": "涨跌停 %（可选：缺省新建为 null、更新保留原值）" },
                "note": { "type": "string", "description": "维护备注（可选，只进 holding_edits 留痕；工具会自动追加来源标记）" }
            },
            "required": ["code", "name", "mkt", "kind", "shares", "cost"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        // ADR-0013 选项 A：沿用自动落账 + 读回（硬门禁评估见被否决项 B / 07 C13）
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let a: UpsertArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "参数不符：{e}。需要 code、name、mkt(SH|SZ)、kind(stock|etf|cb)、shares 整数≥1、cost ≥0；整行覆盖——先 get_holdings 读现值，未变字段原样带回"
                )))
            }
        };
        let note = if a.note.trim().is_empty() {
            AGENT_NOTE_MARK.to_string()
        } else {
            format!("{}{}", a.note.trim(), AGENT_NOTE_MARK)
        };
        let upsert = HoldingUpsert {
            code: a.code,
            name: a.name,
            mkt: a.mkt,
            kind: a.kind,
            sector: a.sector,
            shares: a.shares,
            cost: a.cost,
            limit_pct: a.limit_pct,
            note,
        };
        let (row, changed) = match crate::lock(&self.deps.store).upsert_holding(&upsert, &now_ts())
        {
            Ok(r) => r,
            Err(e) => return Ok(holding_edit_err(e)),
        };
        let note = if changed {
            "已落账（快照校正：现金、trades 流水、已实现盈亏三不动——校正不是交易；审计痕已写 holding_edits）。向用户宣读校正后的行：代码/名称/股数/成本；涉及现金变化另走 adjust_cash。"
        } else {
            "与现值逐位一致——未写库、未留痕（幂等）。向用户说明该持仓与现值一致，无需校正。"
        };
        Ok(ok_json(&json!({
            "holding": row,
            "changed": changed,
            "note": note,
        })))
    }
}

pub struct RemoveHoldingTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for RemoveHoldingTool {
    fn name(&self) -> &str {
        "remove_holding"
    }

    fn description(&self) -> &str {
        "删除持仓行（快照校正，ADR-0009/0013）。**对话内两步确认纪律**：调用前必须向用户复述将被删除的整行（代码/名称/股数/成本，来自 get_holdings 现值）并得到明确同意——对应界面删除的两步确认交互。语义：trades 流水保留、累计已实现盈亏不回滚、现金不动——流水与持仓的连续性自校正点断开，必须向用户如实说明。留痕自动写 holding_edits（op=delete + before 行快照，可照快照恢复）。标的不在持仓时如实报错。参数：code。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "要删除的持仓代码（6 位数字）" }
            },
            "required": ["code"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        // ADR-0013 选项 A：软门禁（人格两步确认纪律）；框架硬门禁评估见被否决项 B / 07 C13
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let Some(code) = args.get("code").and_then(Value::as_str) else {
            return Ok(ToolResult::error(
                "参数不符：需要 code（6 位数字持仓代码）".to_string(),
            ));
        };
        match crate::lock(&self.deps.store).remove_holding(code, &now_ts()) {
            Ok(Some(row)) => Ok(ok_json(&json!({
                "removed": row,
                "note": "已删除。向用户宣读被删行内容，并如实说明：trades 流水保留、累计已实现盈亏不回滚、现金不动——连续性自校正点断开（03 §13）；如需恢复可按 holding_edits 的 before 快照重新 upsert_holding。",
            }))),
            Ok(None) => Ok(ToolResult::error(format!(
                "持仓中没有 {code}（get_holdings 可查当前持仓）"
            ))),
            Err(e) => Ok(holding_edit_err(e)),
        }
    }
}
