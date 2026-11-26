//! 诊断报告取数工具（P7-T10）：`calc_diagnosis`——组合区间风险体检的数字面。
//!
//! 数字全部出自 store.*（equity_snapshots / advice_history 留档原值）→
//! `zimu-domain`（bench::align_bench + perf，03 §7 黄金向量口径），本层零口径。
//! 与界面「组合诊断」屏同一数据源同一公式：屏 = 前端 TS 镜像现算，本工具 =
//! Rust 侧同款（两侧只对 fixture，永不互相对拍）。
//!
//! 降级纪律（03 §7 基准对齐段 + 附录固定口径 6）：
//! - 净值样本空 / <2 → **诚实报错拒绝出数**（不以 0 冒充安全）；
//! - 基准（sh000300 价格指数）kline 失败 / 窗口空 / 对齐失败 → **降级不拒绝**：
//!   `bench.available=false` + reason，perf 吃空数组走 `bench_*` 全 0 既有形态，
//!   文案层禁止「跑赢/跑输」（description 与 notes 双重申明）。

use std::sync::Arc;

use async_trait::async_trait;
use robit_agent::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use zimu_domain::bench::{align_bench, AlignBenchInput, BenchBar};
use zimu_domain::perf::{perf, PerfInput};
use zimu_market::error::MarketError;

use super::{ok_json, store_try};
use crate::AgentDeps;

/// 基准符号（03 §7 生产口径：sh000300 价格指数，未含分红——B11 如实标注义务）
const BENCH_SYMBOL: &str = "sh000300";
/// 净值快照取样上限（与前端 useEquity 的 limit=250 一致）
const SNAPSHOT_LIMIT: i64 = 250;
/// 四窗口（key, label, 尾部截断长度）——与诊断屏 WINDOWS + 全区间一致
const WINDOWS: [(&str, &str, usize); 4] =
    [("all", "全区间", usize::MAX), ("20", "近一月", 20), ("45", "近两月", 45), ("90", "近一季", 90)];

pub struct CalcDiagnosisTool {
    pub deps: Arc<AgentDeps>,
}

/// 行情错误 → 基准降级 reason 文本（market_err 三分类同款话术，但不早退——
/// 基准缺失不拒绝出报告，拼进 bench.reason 供模型如实转述）
fn bench_degrade_reason(e: &MarketError) -> String {
    match e {
        MarketError::Unavailable(m) => format!("行情 sidecar 不可达（{m}）"),
        MarketError::Upstream { status, code, message } => {
            format!("行情上游错误（HTTP {status}，{code}）：{message}")
        }
        MarketError::Contract(m) => format!("行情契约不符（sidecar 与本程序版本漂移）：{m}"),
    }
}

#[async_trait]
impl Tool for CalcDiagnosisTool {
    fn name(&self) -> &str {
        "calc_diagnosis"
    }

    fn description(&self) -> &str {
        "组合诊断报告取数（区间风险体检，与界面「组合诊断」屏同一口径；当日归因用 /复盘，单标的建议用 /建议）：净值快照（equity_snapshots 最近 250 条，date 升序）→ perf 十字段（03 §7）按四窗口尾部截断重跑（全区间/近20/45/90 交易日，与诊断屏分区间表一致）+ 最新快照行原值（总资产/仓位/HHI/前三行业占比等）+ 基准对比（sh000300 价格指数日线按 03 §7 基准对齐段 carry-forward 归一到区间起点；未含分红、非全收益口径，文案引用基准必须如实标注）+ 最近 5 条建议留档摘要。基准降级（sidecar 不可达/日线窗口空/对齐失败）时 bench.available=false 且 bench_* 全 0——此时禁止说「跑赢/跑输基准」。净值样本 <2 条时本工具如实报错拒绝出数（不得以 0 冒充安全）。文案中每个数字必须原样取自本工具返回；唯一允许的展示层运算 = 区间超额 cum − bench_cum（03 §7 B11）。"
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
        // ---- 净值快照（date 升序，store 侧已 reverse）----
        let rows = store_try!(crate::lock(&self.deps.store).equity_snapshots(SNAPSHOT_LIMIT));
        if rows.is_empty() {
            return Ok(ToolResult::error(
                "还没有净值快照——净值序列由每交易日 15:05 盘后链归档（03 §12）。如实告诉主人：现在没有可诊断的样本，录入第一笔并等盘后归档后才能生成报告；不要编造任何数字。",
            ));
        }
        if rows.len() < 2 {
            return Ok(ToolResult::error(
                "净值快照只有 1 条，区间指标无法计算（perf 需 ≥2 个采样点）。如实说明样本不足，不要给出任何指标数值（不得以 0 冒充安全）。",
            ));
        }
        let dates: Vec<String> = rows.iter().map(|r| r.date.clone()).collect();
        let nav: Vec<f64> = rows.iter().map(|r| r.total_assets).collect();

        // ---- 基准（降级不拒绝）：sh000300 日线 → align_bench 归一 ----
        let mut bench_available = false;
        let mut bench_degraded = false;
        let mut bench_source: Option<String> = None;
        let mut bench_reason: Option<String> = None;
        let mut aligned: Option<Vec<f64>> = None;
        match self.deps.market.kline(BENCH_SYMBOL, None, Some(&dates[0]), None).await {
            Ok(p) if p.kline.is_empty() => {
                bench_reason = Some("基准日线窗口为空（count=0）".into());
            }
            Ok(p) => {
                let bars: Vec<BenchBar> = p
                    .kline
                    .iter()
                    .map(|b| BenchBar { date: b.date.clone(), close: b.close })
                    .collect();
                match align_bench(&AlignBenchInput { dates: dates.clone(), bars, nav0: nav[0] }) {
                    Some(a) => {
                        aligned = Some(a);
                        bench_available = true;
                        bench_degraded = p.degraded;
                        bench_source = p.source.clone();
                    }
                    None => {
                        bench_reason = Some(
                            "首快照日早于日线窗口，对齐失败（03 §7 carry-forward null 语义）".into(),
                        );
                    }
                }
            }
            Err(e) => bench_reason = Some(bench_degrade_reason(&e)),
        }

        // ---- 四窗口尾部截断重跑 perf（与诊断屏 slice(-n) 同款）----
        let mut windows = Vec::new();
        for (key, label, w) in WINDOWS {
            let from = nav.len().saturating_sub(w);
            let m = perf(&PerfInput {
                nav: nav[from..].to_vec(),
                // bench None → 空数组 → bench_* 全 0（perf 既有降级形态，前端同款）
                bench: aligned.as_ref().map(|a| a[from..].to_vec()).unwrap_or_default(),
            });
            windows.push(json!({
                "key": key,
                "label": label,
                "samples": nav.len() - from,
                "range": { "start": dates[from], "end": dates[dates.len() - 1] },
                "has_bench": bench_available,
                "metrics": m,
            }));
        }

        // ---- 最近建议留档摘要（新在前；裁 dims/discipline_refs/risks 省 token）----
        let advice = store_try!(crate::lock(&self.deps.store).list_advice(None, 5));
        let recent_advice: Vec<Value> = advice
            .iter()
            .map(|a| {
                json!({
                    "id": a.id,
                    "code": a.code,
                    "action": a.action,
                    "score": a.score,
                    "conf": a.conf,
                    "position": a.position,
                    "stop": a.stop,
                    "target_lo": a.target_lo,
                    "target_hi": a.target_hi,
                    "generated_at": a.generated_at,
                    "data_version": a.data_version,
                    "reason": a.reason,
                })
            })
            .collect();

        let latest = rows.last().expect("rows 非空");
        Ok(ok_json(&json!({
            "samples": { "start": dates[0], "end": dates[dates.len() - 1], "count": rows.len() },
            "latest_snapshot": latest,
            "bench": {
                "symbol": BENCH_SYMBOL,
                "name": "沪深300",
                "index_kind": "价格指数（未含分红，非 03 §7 全收益理想口径——文案必须如实标注，B11）",
                "available": bench_available,
                "degraded": bench_degraded,
                "source": bench_source,
                "reason": bench_reason,
            },
            "windows": windows,
            "kpi_window": "90",
            "recent_advice": recent_advice,
            "notes": [
                "指标由 zimu-domain perf（03 §7，黄金向量校验）计算；窗口 = 尾部截断重跑，与界面诊断屏分区间表一致",
                "基准对齐 = 03 §7 基准对齐段（carry-forward + 归一到区间起点）；bench.available=false 时 bench_* 全 0，不得表述为跑赢/跑输",
                "sh000300 为价格指数（未含分红）；引用基准对比必须带此标注",
                "唯一允许的展示层运算 = 区间超额 cum − bench_cum（03 §7 B11）；其余数字原样引用",
            ],
        })))
    }
}
