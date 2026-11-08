//! 持仓与账本工具：get_holdings（组合全景）、calc_discipline（纪律四条）、
//! simulate_trade（试算不落库）+ 写账本三件（ADR-0012）：record_trade
//! （成交录入，与界面 `POST /api/holdings/trade` 同一 store 事务）、
//! adjust_cash（现金绝对值校正）、get_trades（流水只读）。数字全部出自
//! `store.*` → `zimu-domain`（recalc/discipline/fees/dilution/lot），本层零口径。

use std::sync::Arc;

use async_trait::async_trait;
use robit_agent::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use zimu_domain::discipline::{check, sell_to, SellToInput};
use zimu_domain::lot::lot_size;
use zimu_store::{RecordError, TradeRequest};

use super::{ok_json, store_try};
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
        "查组合全景（03 §5 口径，数字与界面 /api/holdings 同源）：账户现金、持仓明细（代码/名称/类型/行业/股数/摊薄成本/现价/昨收）、逐笔派生 per[]（当日涨跌 day_chg、当日盈亏 day_val、市值 mv、累计盈亏 pnl/pnl_pct、占总资产 weight）、组合汇总 agg（总资产、当日/累计盈亏与百分比、仓位 exposure、行业分布、HHI 集中度）以及纪律四条 discipline。注意价格是最近一次行情回写的值——需要最新数字先调 refresh_quotes。回答持仓/盈亏必须引用本工具返回的数字。"
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
        let inp = store_try!(s.portfolio_input());
        drop(s);
        let report = check(&inp);
        Ok(ok_json(&json!({
            "portfolio": view,
            "discipline": report,
            "note": "weight 分母是总资产（含现金），行业占比分母是持仓市值（不含现金）——文案里数字旁必须写清分母（03 §6）",
        })))
    }
}

// ============================================================================
// calc_discipline
// ============================================================================

pub struct CalcDisciplineTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for CalcDisciplineTool {
    fn name(&self) -> &str {
        "calc_discipline"
    }

    fn description(&self) -> &str {
        "仓位纪律四条体检（03 §6，分母不统一务必照抄）：单票占总资产 ≤25%、单一行业占市值 ≤35%、可转债合计占总资产 ≤12%、现金保留 ≥10%。返回每条的 value/threshold/pass（压线算安全）；单票超限附 reduce_hint——按 §6 sell_to 泛化公式算出的「降到 25% 需卖出的股数（按手向上取整）」。文案里数字旁必须写清分母。"
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
        let inp = store_try!(crate::lock(&self.deps.store).portfolio_input());
        let report = check(&inp);

        // 单票超限时的降仓提示：对占比最大的持仓按 §6 sell_to 出数（可回溯黄金向量）
        let total_assets = inp.cash + inp.holdings.iter().map(|h| h.price * h.shares).sum::<f64>();
        let reduce_hint = if !report.single_stock.pass {
            inp.holdings
                .iter()
                .map(|h| (h, h.price * h.shares))
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(h, mv)| {
                    let lot = lot_size(h.kind);
                    let qty = sell_to(&SellToInput {
                        mv,
                        price: h.price,
                        total_assets,
                        target: zimu_domain::discipline::LIMIT_SINGLE_STOCK,
                        lot,
                    })
                    .qty;
                    json!({
                        "code": h.code,
                        "qty": qty,
                        "lot": lot,
                        "note": "降到 25% 上限需卖出的股数（03 §6 sell_to，按手向上取整）",
                    })
                })
        } else {
            None
        };

        Ok(ok_json(&json!({
            "report": report,
            "reduce_hint": reduce_hint,
            "thresholds": [
                "单票占组合 ≤ 25%（分子 mv，分母 total_assets 含现金）",
                "单一行业占市值 ≤ 35%（分子 Σmv(sector)，分母 total_mv 不含现金）",
                "可转债合计占组合 ≤ 12%（分子 Σmv(cb)，分母 total_assets 含现金）",
                "现金最低保留 ≥ 10%（分子 cash，分母 total_assets 含现金）",
            ],
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
