//! 持仓侧工具：get_holdings（组合全景）、calc_discipline（纪律四条）、
//! simulate_trade（试算不落库）。数字全部出自 `store.portfolio*` →
//! `zimu-domain`（recalc/discipline/fees/dilution/lot），本层零口径。

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
        "试算一笔委托（只读预览，不落库、不下单——程序只给建议，成交由用户确认）：整手与可卖数量校验（03 §1）、涨跌停区间校验（§2，有昨收才拦）、费用三分项（§3：佣金/印花税/过户费，A 股口径）、摊薄成本或已实现盈亏与现金变化（§4）、超 25% 上限等非拦截性警告。参数：code、side（buy/sell）、qty 股数、price 委托价、note 可选；标的未持有且要买入时必须给 meta {name, mkt: SH|SZ, kind: stock|etf|cb, sector, prev?, limit_pct?}。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "标的代码，如 600519" },
                "side": { "type": "string", "enum": ["buy", "sell"], "description": "交易方向" },
                "qty": { "type": "integer", "description": "股数/张数（整手校验见 03 §1）" },
                "price": { "type": "number", "description": "委托价（涨跌停校验见 03 §2）" },
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
            "note": "试算预览，未落库。费用/摊薄/整手/涨跌停数字与录入抽屉同一套领域函数（03 §1–§4），可直接引用；程序不代下单，录入由用户在界面确认",
        })))
    }
}
