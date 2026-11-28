//! 建议生成工具（P4-T6，ADR-0007 的结构性落地）：
//!
//! - `calc_advice`：取数（store + zimu-market）→ `zimu-domain::advice::score_advice`
//!   （03 §10 生产口径）→ **数字列直接写入 `advice_history`**（B9：带
//!   generated_at + data_version）→ 返回 advice_id 与全部数字。
//! - `finalize_advice`：模型文案（reason/risks）**只回填文案列**——数字永不经过
//!   模型转写，留档即可回溯「当时依据什么说的」。
//!
//! 降级边界：日线拿不到 → 拒绝出建议（不出全中性噪声结论）；实时行情拿不到但
//! 日线可用 → price 取末根收盘并在返回里如实标注 price_source。

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Days;
use robit_agent::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use zimu_domain::advice::{score_advice, Action, AdviceInput, Horizon, PositionMode};
// ADR-0024：不再 import check/DisciplineCtx——建议链零纪律；sell_to 仅作为
// 「降到目标仓位的卖出股数」泛式保留（目标 = 得分锚 15%/减半/清仓，非纪律线）
use zimu_domain::discipline::{sell_to, SellToInput};
use zimu_domain::lot::lot_size;
use zimu_domain::Kind;
use zimu_market::clock::now_shanghai;
use zimu_market::error::MarketError;
use zimu_market::symbols::symbol_of;
use zimu_market::synth;
use zimu_store::NewAdvice;

use super::market::split_prefixed;
use super::{market_err, ok_json, store_try};
use crate::AgentDeps;

/// kline 窗口：90 个自然日 ≈ 60 个交易日（技术信号最多要 30 根窗口 + 21 根动量基期）
const KLINE_WINDOW_DAYS: u64 = 90;

fn action_label(a: Action) -> &'static str {
    match a {
        Action::Add => "加仓",
        Action::Buy => "买入",
        Action::Hold => "持有",
        Action::Trim => "减仓",
        Action::Sell => "卖出",
        Action::Dca => "定投",
    }
}

fn horizon_label(h: Horizon) -> &'static str {
    match h {
        Horizon::Short => "短线",
        Horizon::ShortMid => "短中线 2–6 周",
        Horizon::Mid => "中线 1–3 月",
        Horizon::Long => "长线 6 月+",
    }
}

/// position 文案（03 §10：当前值必须是计算值；「当前 x% → 建议 y%」与原型同款）。
/// ADR-0024：上限锚「建议 ≤y%」读法随纪律出链删除——目标只有得分锚形态。
fn position_text(current: f64, suggested: Option<f64>, mode: PositionMode) -> String {
    match mode {
        PositionMode::Target => match suggested {
            Some(s) => format!("当前 {current:.1}% → 建议 {s:.0}%"),
            None => format!("当前 {current:.1}% → 建议 {current:.0}%"),
        },
        PositionMode::Maintain => format!("当前 {current:.1}% → 维持"),
        PositionMode::Dca => format!("当前 {current:.1}% → 每月定投"),
        PositionMode::Exit => format!("当前 {current:.1}% → 建议清仓"),
        PositionMode::Flat => "当前无持仓 → 观望".to_string(),
    }
}

// ============================================================================
// calc_advice
// ============================================================================

pub struct CalcAdviceTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for CalcAdviceTool {
    fn name(&self) -> &str {
        "calc_advice"
    }

    fn description(&self) -> &str {
        // 模型面文案（打分口径 docs/03 §10，ADR-0007 数字归规则 / ADR-0024 纪律出链；
        // 留档写 advice_history 数字列，含 generated_at/data_version）。
        concat!(
            "生成分析结论的全部数字（规则打分，数字不经模型转写）：",
            "score/conf/四维 dims/action/horizon/target 目标区间/stop 止损/position 建议仓位",
            "/中间量 stats（MA5/MA20/动量/区间位置/量比/票数），并当场留档数字、返回 advice_id。",
            "结论完全由行情信号决定。",
            "输入依赖近 90 天日线，日线拿不到会如实报错拒绝出建议。",
            "**返回的每个数字是你文案中数字的唯一来源，不得自行编造或改写**；",
            "写完结论/理由/风险文案后必须调用 finalize_advice(advice_id, reason, risks) 归档，否则留档缺文案。",
        )
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "标的代码：持仓内可用裸代码（600519）；非持仓必须带市场前缀（sh600519）"
                }
            },
            "required": ["code"]
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
        let arg = args
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if arg.is_empty() {
            return Ok(ToolResult::error("code 不能为空"));
        }

        // ---- 标的解析：前缀形态直通；裸代码查持仓（B7 不做号段猜测）----
        let (symbol, code) = match split_prefixed(&arg) {
            Some((p, c)) => (format!("{p}{c}"), c.to_string()),
            None => {
                let h = store_try!(crate::lock(&self.deps.store).holdings())
                    .into_iter()
                    .find(|h| h.code == arg);
                match h.and_then(|h| symbol_of(&h.mkt, &h.code).map(|s| (s, arg.clone()))) {
                    Some(pair) => pair,
                    None => {
                        return Ok(ToolResult::error(format!(
                            "{arg}：不在持仓且未带市场前缀。请用 sh/sz 前缀完整形态（如 sh{arg}），或先在界面录入持仓"
                        )))
                    }
                }
            }
        };

        // ---- 组合上下文（weight/held；ADR-0024：纪律上下文整体拆除）----
        let (holding, portfolio) = {
            let s = crate::lock(&self.deps.store);
            let holding = store_try!(s.holdings()).into_iter().find(|h| h.code == code);
            let portfolio = store_try!(s.portfolio());
            (holding, portfolio)
        };
        let held = holding.is_some();
        let per = portfolio.per.iter().find(|p| p.code == code);
        let weight = per.map(|p| p.weight).unwrap_or(0.0);

        // ---- 实时行情（可降级：失败只影响 price 锚与当日 bar 合成）----
        let quote = self.deps.market.quote(&[symbol.clone()]).await;
        let (qrow, as_of) = match &quote {
            Ok(p) => (
                p.quotes.iter().find(|r| r.symbol == symbol).cloned(),
                Some(p.as_of.clone()),
            ),
            Err(_) => (None, None),
        };
        let quote_note = match &quote {
            Ok(_) => None,
            Err(MarketError::Unavailable(m)) => Some(format!(
                "实时行情不可达（{m}），price 锚回退日线末根收盘——数字口径不变，时点旧一日"
            )),
            Err(e) => Some(format!("实时行情失败（{e}），price 锚回退日线末根收盘")),
        };

        // ---- 日线（硬依赖：拿不到拒绝出建议，不出全中性噪声结论）----
        let now = now_shanghai();
        let start = (now.date_naive() - Days::new(KLINE_WINDOW_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        let mut payload = match self.deps.market.kline(&symbol, None, Some(&start), None).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(match e {
                    MarketError::Unavailable(m) => ToolResult::error(format!(
                        "行情服务不可达，无法生成建议（建议依赖近 {KLINE_WINDOW_DAYS} 天日线）：{m}。请稍后重试，或让用户检查行情服务是否在线"
                    )),
                    other => market_err(other),
                })
            }
        };
        let today = now.date_naive().format("%Y-%m-%d").to_string();
        let mut today_synthesized = false;
        if let Some(row) = &qrow {
            today_synthesized = synth::merge_today(&mut payload, row, &today);
        }

        // closes/volumes 对齐组装（无收盘的 bar 整根丢弃——两序列必须同长）
        let mut closes: Vec<f64> = Vec::new();
        let mut volumes: Vec<Option<f64>> = Vec::new();
        for b in &payload.kline {
            if let Some(c) = b.close {
                closes.push(c);
                volumes.push(b.volume);
            }
        }
        if closes.is_empty() {
            return Ok(ToolResult::error(format!(
                "{code} 近 {KLINE_WINDOW_DAYS} 天日线为空（可能退市或长期停牌），无法出建议——如实告知用户，不要编造数字"
            )));
        }

        // ---- price 锚：实时价优先，缺省回末根收盘 ----
        let (price, price_source) = match qrow.as_ref().and_then(|r| r.price).filter(|p| *p > 0.0)
        {
            Some(p) => (p, "live_quote"),
            None => (*closes.last().expect("closes 非空"), "kline_last_close"),
        };

        // ---- kind：持仓元数据优先，缺省解析行情行（sidecar 显式 kind，B7 同源）----
        let kind = match holding.as_ref().map(|h| h.kind) {
            Some(k) => k,
            None => {
                let raw = qrow
                    .as_ref()
                    .map(|r| r.kind.as_str())
                    .unwrap_or(payload.kind.as_str());
                match serde_json::from_value::<Kind>(json!(raw)) {
                    Ok(k) => k,
                    Err(_) => {
                        return Ok(ToolResult::error(format!(
                            "无法确定 {code} 的标的类型（行情行 kind={raw} 不在 stock/etf/cb 内），拒绝出建议"
                        )))
                    }
                }
            }
        };

        // ---- 领域打分（03 §10 生产口径；黄金向量 advice.json 同款函数）----
        let out = score_advice(&AdviceInput {
            kind,
            closes,
            volumes,
            price,
            held,
            weight,
        });

        // ---- 留档（B9：数字列 + generated_at + data_version）----
        let k_end = payload
            .kline
            .last()
            .map(|b| b.date.clone())
            .unwrap_or_else(|| "none".into());
        let data_version = format!("adv-v1|k={k_end}|q={}", as_of.clone().unwrap_or_else(|| "none".into()));
        let generated_at = now.format("%Y-%m-%d %H:%M").to_string();
        let pos_text =
            position_text(out.position.current, out.position.suggested, out.position.mode);
        let dims_json = serde_json::to_value(&out.dims).unwrap_or_else(|_| json!([]));
        let advice_id = {
            let s = crate::lock(&self.deps.store);
            store_try!(s.insert_advice(&NewAdvice {
                code: code.clone(),
                action: action_label(out.action).to_string(),
                score: out.score,
                conf: out.conf,
                target_lo: out.target_lo,
                target_hi: out.target_hi,
                stop: out.stop,
                position: pos_text.clone(),
                dims: dims_json,
                // ADR-0024：留档列保留（v5 不回滚）但恒写空数组——纪律不再进建议
                discipline_refs: json!([]),
                data_version: data_version.clone(),
                generated_at: generated_at.clone(),
            }))
        };

        // ---- 降仓数量提示（sell_to 泛式：建议仓位低于当前时给可卖股数）----
        // ADR-0024：目标只来自得分锚（减仓 = weight/2、卖出 = 清仓）；单票降到
        // 25% 上限的测算归 Web 端再平衡清单（前端 sellTo 现算），建议链不给。
        let sell_qty_hint = match (held, out.position.mode, out.position.suggested) {
            (true, PositionMode::Exit, _) => holding.as_ref().map(|h| h.shares),
            (true, PositionMode::Target, Some(target)) if target < weight => {
                let shares = holding.as_ref().map(|h| h.shares).unwrap_or(0);
                sell_to(&SellToInput {
                    mv: price * shares as f64,
                    price,
                    total_assets: portfolio.agg.total_assets,
                    target,
                    lot: lot_size(kind),
                })
                .qty
                .into()
            }
            _ => None,
        };

        let name = holding
            .as_ref()
            .map(|h| h.name.clone())
            .or_else(|| qrow.as_ref().and_then(|r| r.name.clone()));
        let target = match (out.target_lo, out.target_hi) {
            (Some(lo), Some(hi)) => json!([lo, hi]),
            _ => Value::Null,
        };

        Ok(ok_json(&json!({
            "advice_id": advice_id,
            "code": code,
            "name": name,
            "action": action_label(out.action),
            "action_code": out.action,
            "horizon": horizon_label(out.horizon),
            "score": out.score,
            "conf": out.conf,
            "target": target,
            "stop": out.stop,
            "position": {
                "current": out.position.current,
                "suggested": out.position.suggested,
                "mode": out.position.mode,
                "text": pos_text,
                "sell_qty_hint": sell_qty_hint,
            },
            "dims": out.dims,
            "stats": out.stats,
            "price_used": price,
            "price_source": price_source,
            "quote_degraded": quote_note,
            "kline": {
                "bars": payload.kline.len(),
                "end": k_end,
                "today_synthesized": today_synthesized,
                "source": payload.source,
                "degraded": payload.degraded,
            },
            "data_version": data_version,
            "generated_at": generated_at,
            "note": concat!(
                "以上数字全部由规则函数产出，数字已留档；",
                "你文案中出现的每个数字必须原样取自本返回（conf 是样本内信号一致性、不是胜率，不得写成涨跌概率）。",
                "本结论完全由行情信号决定。",
                "写完文案后调用 finalize_advice(advice_id, reason, risks) 归档，risks 至少一条。",
            ),
        })))
    }
}

// ============================================================================
// finalize_advice
// ============================================================================

pub struct FinalizeAdviceTool {
    pub deps: Arc<AgentDeps>,
}

#[async_trait]
impl Tool for FinalizeAdviceTool {
    fn name(&self) -> &str {
        "finalize_advice"
    }

    fn description(&self) -> &str {
        // 模型面文案（数字/文案分列不可互改=ADR-0007；risks 硬规则 docs/03 §10）。
        concat!(
            "把你写好的建议文案回填进留档（只动 reason/risks 文案列，数字不可改写）。参数：",
            "advice_id（calc_advice 返回）、",
            "reason 结论文案（其中的数字必须原样来自 calc_advice 返回，并包含失效条件，",
            "如「收盘跌破止损价 x 则结论失效」）、",
            "risks 风险自陈字符串数组（至少一条，不可为空）。",
            "每次 calc_advice 之后必须调用本工具完成归档。",
        )
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "advice_id": { "type": "integer", "description": "calc_advice 返回的留档 id" },
                "reason": { "type": "string", "description": "结论文案（数字原样取自 calc_advice 返回；含目标价/止损/失效条件）" },
                "risks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "风险自陈，至少一条（这个结论可能错在哪）"
                }
            },
            "required": ["advice_id", "reason", "risks"]
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
        let Some(id) = args.get("advice_id").and_then(Value::as_i64) else {
            return Ok(ToolResult::error("advice_id 缺失或不是整数"));
        };
        let reason = args
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if reason.is_empty() {
            return Ok(ToolResult::error("reason 不能为空（结论文案，数字取自 calc_advice 返回）"));
        }
        let risks: Vec<String> = args
            .get("risks")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if risks.is_empty() {
            return Ok(ToolResult::error(
                "risks 至少一条且不可为空串（风险自陈是这个结论可能错在哪，不允许留空）",
            ));
        }

        let s = crate::lock(&self.deps.store);
        let found = store_try!(s.finalize_advice(id, &reason, &risks));
        if !found {
            return Ok(ToolResult::error(format!(
                "advice_id {id} 不存在——先调 calc_advice 生成数字留档，再回填文案"
            )));
        }
        let row = store_try!(s.advice(id));
        drop(s);
        Ok(ok_json(&json!({
            "archived": row,
            "note": "已归档：数字与文案同行留档、可回溯。可在回复末尾告知用户本条建议已留档。",
        })))
    }
}
