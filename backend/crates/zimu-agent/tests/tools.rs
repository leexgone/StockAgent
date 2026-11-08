//! zimu-agent 领域工具集成测试（P4-T6，全离线：FakeApi 注入 + in-memory Store）。
//!
//! 验收锚点（roadmap P4-T6「复盘输出的每个数字可回溯到 P3 公式」的机器可验部分）：
//! - `calc_advice` 用**黄金向量 av-bull-add 同款输入**（日线 24 根 + 当日实时行合成
//!   第 25 根）端到端跑通：输出数字与 `fixtures/golden/advice.json` 期望逐位一致，
//!   且数字列已写入 advice_history；
//! - `finalize_advice` 只回填文案列，数字列不可改写；risks 空数组被拒（03 §10）；
//! - `get_quote` 的涨跌停与 `zimu-domain::price_limit` 同源（03 §2）；
//! - `calc_discipline` / `get_holdings` 数字与 domain 直接调用逐位等价（03 §5/§6）；
//! - `refresh_quotes` 与 HTTP refresh 同一条链（service::refresh_portfolio）；
//! - sidecar 降级：日线拿不到 → calc_advice 拒绝出建议；health 挂 → market_status
//!   如实报 offline，不瘫；
//! - Skill 模板可加载、触发词唯一、tools_required 全部已注册。

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Days;
use robit_agent::tool::async_runner::AsyncTaskRunner;
use robit_agent::{Frontend, TaskRegistry, Tool, ToolContext};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use zimu_agent::skills::load_zimu_skills;
use zimu_agent::tools::{build_registry, AdjustCashTool, CalcAdviceTool, CalcDisciplineTool, FinalizeAdviceTool, GetHoldingsTool, GetQuoteTool, GetTradesTool, MarketStatusTool, RecordTradeTool, RefreshQuotesTool, RemoveHoldingTool, SimulateTradeTool, UpsertHoldingTool};
use zimu_agent::{AgentDeps, SharedStore};
use zimu_market::clock::now_shanghai;
use zimu_market::error::{MarketError, MarketResult};
use zimu_market::models::{
    CalendarPayload, CbListPayload, KlineBar, KlinePayload, QuotePayload, QuoteRow, QuoteStatus,
    SidecarHealth,
};
use zimu_market::MarketApi;
use zimu_domain::Side;
use zimu_store::{AccountRow, NewHoldingMeta, Store, TradeRequest};

// ============================================================================
// FakeApi —— MarketApi 注入点（sidecar 替身）
// ============================================================================

#[derive(Default)]
struct FakeApi {
    quote: Mutex<Option<QuotePayload>>,
    kline: Mutex<Option<KlinePayload>>,
    calendar: Mutex<Option<CalendarPayload>>,
    /// sidecar 整机开关：true = 全部端点 Unavailable（模拟 sidecar 挂了）
    down: AtomicBool,
}

impl FakeApi {
    fn arc() -> Arc<Self> {
        Arc::new(Self::default())
    }
    fn set_quote(&self, p: QuotePayload) {
        *self.quote.lock().unwrap() = Some(p);
    }
    fn set_kline(&self, p: KlinePayload) {
        *self.kline.lock().unwrap() = Some(p);
    }
    fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }
    fn unavailable<T>(&self, what: &str) -> MarketResult<T> {
        if self.down.load(Ordering::SeqCst) {
            Err(MarketError::Unavailable("sidecar 整机不可达（测试注入）".into()))
        } else {
            Err(MarketError::Unavailable(format!("{what} 未配置（测试）")))
        }
    }
}

#[async_trait]
impl MarketApi for FakeApi {
    fn base_url(&self) -> &str {
        "fake://zimu-agent-test"
    }
    async fn health(&self) -> MarketResult<SidecarHealth> {
        if self.down.load(Ordering::SeqCst) {
            return Err(MarketError::Unavailable("sidecar 整机不可达（测试注入）".into()));
        }
        Ok(SidecarHealth {
            ok: true,
            version: Some("test".into()),
            rest: serde_json::Map::new(),
        })
    }
    async fn quote(&self, _symbols: &[String]) -> MarketResult<QuotePayload> {
        match self.quote.lock().unwrap().clone() {
            Some(p) if !self.down.load(Ordering::SeqCst) => Ok(p),
            _ => self.unavailable("quote"),
        }
    }
    async fn kline(
        &self,
        _symbol: &str,
        _kind: Option<&str>,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> MarketResult<KlinePayload> {
        match self.kline.lock().unwrap().clone() {
            Some(p) if !self.down.load(Ordering::SeqCst) => Ok(p),
            _ => self.unavailable("kline"),
        }
    }
    async fn cb_list(&self) -> MarketResult<CbListPayload> {
        self.unavailable("cb_list")
    }
    async fn calendar(
        &self,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> MarketResult<CalendarPayload> {
        match self.calendar.lock().unwrap().clone() {
            Some(p) if !self.down.load(Ordering::SeqCst) => Ok(p),
            _ => self.unavailable("calendar"),
        }
    }
}

// ============================================================================
// ToolContext 测试替身（robit 内部测试同款最小构造）
// ============================================================================

struct DummyFrontend;

#[async_trait]
impl Frontend for DummyFrontend {
    async fn on_event(&self, _event: robit_agent::AgentEvent) -> robit_agent::error::Result<()> {
        Ok(())
    }
    async fn request_tool_confirmation(
        &self,
        _info: &robit_agent::ToolCallInfo,
    ) -> robit_agent::error::Result<bool> {
        Ok(false)
    }
}

fn test_ctx() -> ToolContext {
    let (done_tx, _done_rx) = tokio::sync::mpsc::channel(1);
    ToolContext {
        working_dir: std::path::PathBuf::from("."),
        session_id: "test-session".into(),
        tool_call_id: "test-call".into(),
        frontend: Arc::new(DummyFrontend),
        extensions: HashMap::<String, Arc<dyn Any + Send + Sync>>::new(),
        supports_images: false,
        async_runner: AsyncTaskRunner::new(done_tx),
        cancel_token: CancellationToken::new(),
        task_registry: TaskRegistry::new(),
    }
}

async fn exec(tool: &impl Tool, args: Value) -> Value {
    let res = tool.execute(args, &test_ctx()).await.expect("execute 不应失败");
    assert!(!res.is_error, "工具意外报错：{}", res.content);
    serde_json::from_str(&res.content).expect("工具输出必须是 JSON")
}

async fn exec_err(tool: &impl Tool, args: Value) -> String {
    let res = tool.execute(args, &test_ctx()).await.expect("execute 不应失败");
    assert!(res.is_error, "工具本该报错却成功了：{}", res.content);
    res.content
}

/// 数值按容差、其余精确的递归比对（03 附录比对规则同款哲学）。
/// **不能直接 assert_eq! 两个 Value**：serde_json 的浮点解析 fast path 有
/// 1-ulp 双舍入误差（如 -0.046432975226967516 经 to_string→from_str 变
/// -0.04643297522696752），工具输出经过 JSON 文本往返后与内存值可差 1 ulp。
fn json_close(a: &Value, b: &Value, path: &str, tol: f64, errs: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, xv) in x {
                match y.get(k) {
                    Some(yv) => json_close(xv, yv, &format!("{path}.{k}"), tol, errs),
                    None => errs.push(format!("{path}.{k}: 右侧缺少该字段")),
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                errs.push(format!("{path}: 数组长度 {} != {}", x.len(), y.len()));
                return;
            }
            for (i, (xv, yv)) in x.iter().zip(y).enumerate() {
                json_close(xv, yv, &format!("{path}[{i}]"), tol, errs);
            }
        }
        (Value::Number(x), Value::Number(y)) => {
            let (xv, yv) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (xv - yv).abs() > tol {
                errs.push(format!("{path}: {xv} != {yv}（超出容差 {tol}）"));
            }
        }
        _ if a != b => errs.push(format!("{path}: {a} != {b}")),
        _ => {}
    }
}

fn assert_json_close(actual: &Value, expected: &Value, what: &str) {
    let mut errs = Vec::new();
    json_close(actual, expected, what, 1e-9, &mut errs);
    assert!(errs.is_empty(), "{what} 与 domain 直算不一致：\n{}", errs.join("\n"));
}

// ============================================================================
// 种子与行情构造
// ============================================================================

fn seeded_store() -> SharedStore {
    let s = Store::in_memory().unwrap();
    s.set_account(&AccountRow {
        cash: 300_000.0,
        ..AccountRow::default()
    })
    .unwrap();
    // 小仓位建 600519（100 股 @110 ≈ 占总资产 3.5%，落在「加仓」判定的 <15% 一侧）
    s.record_trade(&TradeRequest {
        code: "600519".into(),
        side: Side::Buy,
        qty: 100,
        price: 110.0,
        note: String::new(),
        meta: Some(NewHoldingMeta {
            name: "贵州茅台".into(),
            mkt: "SH".into(),
            kind: zimu_domain::Kind::Stock,
            sector: "白酒".into(),
            prev: None,
            limit_pct: None,
        }),
    })
    .unwrap();
    Arc::new(Mutex::new(s))
}

/// 黄金向量 av-bull-add 的日线部分：前 24 根（第 25 根由当日实时行合成）。
/// closes = [120, 100×19, 104, 106, 108, 110]，volumes = [5000, 1000×19, 3000×4]
fn bull_kline() -> KlinePayload {
    let mut closes = vec![120.0];
    closes.extend(vec![100.0; 19]);
    closes.extend([104.0, 106.0, 108.0, 110.0]);
    let mut volumes = vec![5000.0];
    volumes.extend(vec![1000.0; 19]);
    volumes.extend(vec![3000.0; 4]);

    let today = now_shanghai().date_naive();
    let n = closes.len();
    let bars: Vec<KlineBar> = closes
        .iter()
        .zip(&volumes)
        .enumerate()
        .map(|(i, (c, v))| KlineBar {
            // 末根 = 昨天（今天留给实时行合成，synth::merge_today 条件 4）
            date: (today - Days::new((n - i) as u64)).format("%Y-%m-%d").to_string(),
            open: Some(*c),
            high: Some(*c),
            low: Some(*c),
            close: Some(*c),
            volume: Some(*v),
            amount: None,
        })
        .collect();
    KlinePayload {
        symbol: "sh600519".into(),
        kind: "stock".into(),
        kind_routed: "explicit".into(),
        adjusted: "qfq".into(),
        start: bars.first().map(|b| b.date.clone()).unwrap_or_default(),
        end: bars.last().map(|b| b.date.clone()).unwrap_or_default(),
        count: bars.len(),
        source: Some("fake_daily".into()),
        degraded: false,
        attempts: vec![],
        kline: bars,
    }
}

/// 当日实时行：price 112（合成第 25 根收盘 = av-bull-add 的 112），volume 3000
fn bull_quote() -> QuotePayload {
    let today = now_shanghai().date_naive().format("%Y-%m-%d").to_string();
    QuotePayload {
        as_of: format!("{today}T15:00:03+08:00"),
        count: 1,
        degraded: false,
        attempts: vec![],
        quotes: vec![QuoteRow {
            symbol: "sh600519".into(),
            code: "600519".into(),
            mkt: "SH".into(),
            kind: "stock".into(),
            name: Some("贵州茅台".into()),
            price: Some(112.0),
            prev: Some(110.0),
            open: Some(110.5),
            high: Some(112.5),
            low: Some(110.2),
            volume: Some(3000.0),
            amount: None,
            chg_pct: Some(1.818),
            date: Some(today),
            time: Some("15:00:03".into()),
            status: QuoteStatus::Ok,
            source: Some("fake_hq".into()),
        }],
    }
}

fn deps_with(store: SharedStore, fake: Arc<FakeApi>) -> Arc<AgentDeps> {
    AgentDeps::new(store, fake)
}

// ============================================================================
// calc_advice —— 黄金向量端到端
// ============================================================================

#[tokio::test]
async fn calc_advice_matches_golden_vector_and_archives() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    fake.set_kline(bull_kline());
    fake.set_quote(bull_quote());
    let deps = deps_with(store.clone(), fake);
    let tool = CalcAdviceTool { deps };

    let out = exec(&tool, json!({ "code": "600519" })).await;

    // —— 与 fixtures/golden/advice.json av-bull-add 期望逐位一致（当日 bar 合成后
    //    输入 = 25 根 [120,100×19,104,106,108,110,112]）——
    assert_eq!(out["action"], "加仓");
    assert_eq!(out["action_code"], "add");
    assert_eq!(out["horizon"], "中线 1–3 月");
    let score = out["score"].as_f64().unwrap();
    assert!((score - 73.0).abs() < 1e-9, "score={score}");
    let conf = out["conf"].as_f64().unwrap();
    assert!((conf - 0.9).abs() < 1e-9, "conf={conf}");
    assert_eq!(out["target"], json!([120.96, 128.8]));
    assert_eq!(out["stop"].as_f64().unwrap(), 103.04);
    assert_eq!(out["dims"][0]["s"].as_f64().unwrap(), 87.5);
    assert_eq!(out["dims"][2]["s"].as_f64().unwrap(), 70.0);
    assert_eq!(out["stats"]["ma5"].as_f64().unwrap(), 108.0);
    assert_eq!(out["stats"]["vote_sum"], 4);
    assert_eq!(out["stats"]["vote_count"], 5);
    assert_eq!(out["kline"]["today_synthesized"], true);
    assert_eq!(out["kline"]["bars"], 25);
    assert_eq!(out["price_used"].as_f64().unwrap(), 112.0);
    assert_eq!(out["price_source"], "live_quote");
    // weight ≈ 3.5% < 15 → 建议到目标仓位；未超限不触发 sell hint
    assert_eq!(out["position"]["suggested"].as_f64().unwrap(), 15.0);
    assert_eq!(out["position"]["mode"], "target");
    assert!(out["position"]["sell_qty_hint"].is_null());

    // —— 数字列已留档（B9），文案列留空等 finalize ——
    let id = out["advice_id"].as_i64().unwrap();
    let row = zimu_agent::lock(&store).advice(id).unwrap().unwrap();
    assert_eq!(row.code, "600519");
    assert_eq!(row.action, "加仓");
    assert!((row.score - 73.0).abs() < 1e-9);
    assert_eq!(row.target_lo, Some(120.96));
    assert_eq!(row.stop, Some(103.04));
    assert_eq!(row.reason, "");
    assert!(row.risks.is_empty());
    assert!(row.data_version.starts_with("adv-v1|k="), "dv={}", row.data_version);
    assert!(row.data_version.contains("|q="), "dv={}", row.data_version);
    assert_eq!(row.dims.as_array().unwrap().len(), 4);
    assert_eq!(row.dims[0]["k"], "技术面");
}

#[tokio::test]
async fn calc_advice_refuses_without_kline() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    fake.set_quote(bull_quote()); // 有实时行但没日线 → 拒绝（不出全中性噪声结论）
    let tool = CalcAdviceTool { deps: deps_with(store, fake) };
    let err = exec_err(&tool, json!({ "code": "600519" })).await;
    assert!(err.contains("无法生成建议"), "{err}");
}

#[tokio::test]
async fn calc_advice_bare_unknown_code_rejected_b7() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let tool = CalcAdviceTool { deps: deps_with(store, fake) };
    let err = exec_err(&tool, json!({ "code": "600000" })).await;
    assert!(err.contains("前缀") && err.contains("B7"), "{err}");
}

// ============================================================================
// finalize_advice —— 文字归模型，数字不可改写
// ============================================================================

#[tokio::test]
async fn finalize_fills_text_only_and_requires_risks() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    fake.set_kline(bull_kline());
    fake.set_quote(bull_quote());
    let deps = deps_with(store.clone(), fake);
    let calc = CalcAdviceTool { deps: deps.clone() };
    let fin = FinalizeAdviceTool { deps };

    let out = exec(&calc, json!({ "code": "600519" })).await;
    let id = out["advice_id"].as_i64().unwrap();

    // risks 空 → 拒绝（03 §10：至少一条）
    let err = exec_err(&fin, json!({ "advice_id": id, "reason": "回踩不破 MA20", "risks": [] })).await;
    assert!(err.contains("至少一条"), "{err}");
    // reason 空 → 拒绝
    let err = exec_err(&fin, json!({ "advice_id": id, "reason": "  ", "risks": ["x"] })).await;
    assert!(err.contains("reason"), "{err}");
    // id 不存在 → 拒绝
    let err = exec_err(&fin, json!({ "advice_id": 424242, "reason": "x", "risks": ["y"] })).await;
    assert!(err.contains("不存在"), "{err}");

    // 正常回填：文案进库，数字列一字不动
    let done = exec(
        &fin,
        json!({ "advice_id": id, "reason": "回踩不破 MA20，量能配合；收盘跌破止损价 103.04 则结论失效。", "risks": ["放量下跌则资金面判断失效"] }),
    )
    .await;
    assert_eq!(done["archived"]["reason"].as_str().unwrap().contains("103.04"), true);
    let row = zimu_agent::lock(&store).advice(id).unwrap().unwrap();
    assert!((row.score - 73.0).abs() < 1e-9);
    assert_eq!(row.stop, Some(103.04));
    assert_eq!(row.risks, vec!["放量下跌则资金面判断失效".to_string()]);
}

// ============================================================================
// get_quote / calc_discipline / get_holdings —— 数字与 domain 同源
// ============================================================================

#[tokio::test]
async fn get_quote_attaches_domain_limit_prices() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    // 昨收 1421 + 数据层配置 limit_pct=10 → 限价与 03 §2 整数单元 half-up 同源
    let mut q = bull_quote();
    q.quotes[0].price = Some(1437.0);
    q.quotes[0].prev = Some(1421.0);
    fake.set_quote(q);
    {
        let s = zimu_agent::lock(&store);
        s.apply_quotes(&[zimu_store::QuoteUpdate {
            code: "600519".into(),
            price: Some(1437.0),
            prev: Some(1421.0),
            name: None,
            limit_pct: Some(10),
            ts: "2026-10-28 15:00".into(),
        }])
        .unwrap();
    }
    let tool = GetQuoteTool { deps: deps_with(store, fake) };
    let out = exec(&tool, json!({ "codes": "600519" })).await;
    let row = &out["quotes"][0];
    // 1421 × 1.1 = 1563.1，1421 × 0.9 = 1278.9（half-up 整数单元，与契约测试同锚）
    assert_eq!(row["limit_up"].as_f64().unwrap(), 1563.1);
    assert_eq!(row["limit_down"].as_f64().unwrap(), 1278.9);
    assert_eq!(row["limit_pct"], 10);
    assert_eq!(row["status"], "ok");

    // 非持仓裸代码 → B7 拒绝；前缀形态可查
    let err = exec_err(&tool, json!({ "codes": "600000" })).await;
    assert!(err.contains("B7"), "{err}");
}

#[tokio::test]
async fn discipline_and_holdings_match_domain_bitwise() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let deps = deps_with(store.clone(), fake);

    // domain 直接算（黄金向量同款函数）
    let inp = zimu_agent::lock(&store).portfolio_input().unwrap();
    let expected = zimu_domain::discipline::check(&inp);
    let expected_json = serde_json::to_value(&expected).unwrap();

    let d = exec(&CalcDisciplineTool { deps: deps.clone() }, json!({})).await;
    assert_json_close(&d["report"], &expected_json, "纪律四条");
    assert_eq!(d["reduce_hint"], Value::Null); // 单票 3.5% 未超限

    let h = exec(&GetHoldingsTool { deps }, json!({})).await;
    let view = zimu_agent::lock(&store).portfolio().unwrap();
    assert_json_close(&h["portfolio"], &serde_json::to_value(&view).unwrap(), "组合全景");
    assert_json_close(&h["discipline"], &expected_json, "get_holdings 纪律");
    // weight 分母口径提示必须在（03 §6：两个分母并存）
    assert!(h["note"].as_str().unwrap().contains("分母"));
}

// ============================================================================
// refresh_quotes / simulate_trade / market_status
// ============================================================================

#[tokio::test]
async fn refresh_quotes_writes_and_recalculates() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let mut q = bull_quote();
    q.quotes[0].price = Some(120.0);
    q.quotes[0].prev = Some(110.0);
    fake.set_quote(q);
    let tool = RefreshQuotesTool { deps: deps_with(store.clone(), fake) };

    let out = exec(&tool, json!({})).await;
    assert_eq!(out["updated"], 1);
    assert_eq!(out["skipped"], json!([]));
    // 回写后 recalc 重算：price 120、prev 110 → day_chg = 10/110×100
    let per = &out["portfolio"]["per"][0];
    let expected_chg = (120.0 - 110.0) / 110.0 * 100.0;
    assert!((per["day_chg"].as_f64().unwrap() - expected_chg).abs() < 1e-9);
    // limit_pct 未配置 → 推断 10（600 开头主板）
    assert_eq!(zimu_agent::lock(&store).holdings().unwrap()[0].limit_pct, Some(10));

    // sidecar 挂 → 如实报错，不写库
    let fake2 = FakeApi::arc();
    fake2.set_down(true);
    let tool2 = RefreshQuotesTool { deps: deps_with(store.clone(), fake2) };
    let err = exec_err(&tool2, json!({})).await;
    assert!(err.contains("不可达"), "{err}");
}

#[tokio::test]
async fn simulate_trade_previews_without_writing() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let deps = deps_with(store.clone(), fake);
    let tool = SimulateTradeTool { deps };

    // 正常试算：与 store.simulate_trade 逐位一致（同一函数）
    let out = exec(&tool, json!({ "code": "600519", "side": "buy", "qty": 100, "price": 112.0 })).await;
    let req = zimu_store::TradeRequest {
        code: "600519".into(),
        side: Side::Buy,
        qty: 100,
        price: 112.0,
        note: String::new(),
        meta: None,
    };
    let expected = zimu_agent::lock(&store).simulate_trade(&req).unwrap();
    assert_json_close(&out["preview"], &serde_json::to_value(&expected).unwrap(), "试算预览");
    // 试算不落库：持仓股数不变
    assert_eq!(zimu_agent::lock(&store).holdings().unwrap()[0].shares, 100);

    // 卖出无持仓标的 → 规则拒绝
    let err = exec_err(&tool, json!({ "code": "600000", "side": "sell", "qty": 100, "price": 10.0 })).await;
    assert!(err.contains("拒绝") && err.contains("600000"), "{err}");
}

#[tokio::test]
async fn market_status_survives_dead_sidecar() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    fake.set_down(true);
    let tool = MarketStatusTool { deps: deps_with(store, fake) };
    let out = exec(&tool, json!({})).await; // 永不失败：降级如实标注
    assert_eq!(out["sidecar_online"], false);
    assert!(out["sidecar_error"].is_string());
    assert_eq!(out["calendar_available"], false); // 无日历 → 工作日启发式
    assert!(out["clock"]["state"].is_string()); // 时钟照走针
}

// ============================================================================
// 写账本三件（ADR-0012：自动落账 + 读回，流水只增不改）
// ============================================================================

#[tokio::test]
async fn record_trade_writes_ledger_with_agent_note_mark() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let tool = RecordTradeTool { deps: deps_with(store.clone(), fake) };

    // 成功路径：再买 100 股 @112，用户 note 后追加来源标记
    let out = exec(&tool, json!({ "code": "600519", "side": "buy", "qty": 100, "price": 112.0, "note": "加仓" })).await;
    let receipt = &out["receipt"];
    assert_eq!(receipt["trade"]["qty"], 100);
    assert_eq!(receipt["trade"]["note"], "加仓（经智能体录入）");
    assert_eq!(receipt["removed"], false);
    // 读回指令内嵌在工具返回里（模型必然看到，ADR-0012 防线第二层）
    assert!(out["note"].as_str().unwrap().contains("宣读"));
    // 账本落地：持仓 200 股、流水 +1、现金联动（100×112 + 费用）
    let s = zimu_agent::lock(&store);
    assert_eq!(s.holdings().unwrap()[0].shares, 200);
    assert_eq!(s.trades(50).unwrap().len(), 2); // seeded 1 笔 + 本笔
    assert!(s.account().unwrap().cash < 300_000.0 - 11_200.0);
    drop(s);

    // note 为空 → 只落来源标记
    let out2 = exec(&tool, json!({ "code": "600519", "side": "buy", "qty": 100, "price": 110.0 })).await;
    assert_eq!(out2["receipt"]["trade"]["note"], "（经智能体录入）");
}

#[tokio::test]
async fn record_trade_rejects_and_leaves_ledger_untouched() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let tool = RecordTradeTool { deps: deps_with(store.clone(), fake) };
    let before_cash = zimu_agent::lock(&store).account().unwrap().cash;
    let before_trades = zimu_agent::lock(&store).trades(50).unwrap().len();

    // 非整手买入 → lot 校验拒绝（03 §1）
    let err = exec_err(&tool, json!({ "code": "600519", "side": "buy", "qty": 50, "price": 112.0 })).await;
    assert!(err.contains("拒绝"), "{err}");
    // 卖出超持有 → exceeds_held 拒绝
    let err = exec_err(&tool, json!({ "code": "600519", "side": "sell", "qty": 200, "price": 112.0 })).await;
    assert!(err.contains("拒绝"), "{err}");
    // 现金不足 → InsufficientCash（300k 买不起 10000 股 ×112）
    let err = exec_err(&tool, json!({ "code": "600519", "side": "buy", "qty": 10000, "price": 112.0 })).await;
    assert!(err.contains("现金不足"), "{err}");

    // 零副作用：流水行数与现金分毫不动（事务性 store 保证，此处断言钉死）
    let s = zimu_agent::lock(&store);
    assert_eq!(s.trades(50).unwrap().len(), before_trades);
    assert_eq!(s.account().unwrap().cash, before_cash);
}

#[tokio::test]
async fn adjust_cash_sets_absolute_value_with_delta() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let tool = AdjustCashTool { deps: deps_with(store.clone(), fake) };
    let before = zimu_agent::lock(&store).account().unwrap().cash;

    let out = exec(&tool, json!({ "cash": 250_000.0, "reason": "对账单校正" })).await;
    assert_eq!(out["before"].as_f64().unwrap(), before);
    assert_eq!(out["after"], 250_000.0);
    assert!((out["delta"].as_f64().unwrap() - (250_000.0 - before)).abs() < 1e-9);
    assert_eq!(out["reason"], "对账单校正");
    assert_eq!(zimu_agent::lock(&store).account().unwrap().cash, 250_000.0);

    // 负现金 → store BadInput 拒绝（现金下限是业务事实）
    let err = exec_err(&tool, json!({ "cash": -1.0 })).await;
    assert!(err.contains("现金不能为负"), "{err}");
    // 缺 cash → 参数错误
    let err = exec_err(&tool, json!({})).await;
    assert!(err.contains("cash"), "{err}");
}

#[tokio::test]
async fn get_trades_reads_ledger_newest_first_with_limit() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let deps = deps_with(store.clone(), fake);
    let rec = RecordTradeTool { deps: deps.clone() };
    for _ in 0..3 {
        exec(&rec, json!({ "code": "600519", "side": "buy", "qty": 100, "price": 110.0 })).await;
    }
    let tool = GetTradesTool { deps };

    let out = exec(&tool, json!({})).await; // 缺省 limit 20
    assert_eq!(out["count"], 4); // seeded 1 + 3
    assert_eq!(out["limit"], 20);
    // 最新在前：首条是工具写入的（带来源标记），seeded 那笔 note 为空
    assert!(out["trades"][0]["note"].as_str().unwrap().contains("经智能体录入"));

    let out = exec(&tool, json!({ "limit": 2 })).await;
    assert_eq!(out["count"], 2);
    let out = exec(&tool, json!({ "limit": 999 })).await; // 上限 50（对话侧省 context）
    assert_eq!(out["limit"], 50);
}

#[test]
fn ledger_tools_are_auto_commit_per_adr_0012() {
    // 把拍板钉进测试：写账本/流水三件全部免确认（自动落账 + 读回）。
    // 翻回逐笔确认 = 改三个 requires_confirmation() 为 true（ADR-0012 被否决项 B）。
    let store = seeded_store();
    let fake = FakeApi::arc();
    let deps = deps_with(store, fake);
    assert!(!RecordTradeTool { deps: deps.clone() }.requires_confirmation());
    assert!(!AdjustCashTool { deps: deps.clone() }.requires_confirmation());
    assert!(!GetTradesTool { deps }.requires_confirmation());
}

// ============================================================================
// 持仓快照校正两件（ADR-0013：初始化映射 / 定期校准 / 场外变动）
// ============================================================================

#[tokio::test]
async fn upsert_holding_creates_edits_with_audit_trail_and_three_untouched() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let tool = UpsertHoldingTool { deps: deps_with(store.clone(), fake) };

    // 新建：奇零股 150 合法（快照校正跳过 lot 校验，03 §13）
    let out = exec(&tool, json!({
        "code": "000001", "name": "平安银行", "mkt": "SZ", "kind": "stock",
        "sector": "银行", "shares": 150, "cost": 10.5, "note": "初始化搬入"
    })).await;
    assert_eq!(out["changed"], true);
    assert_eq!(out["holding"]["shares"], 150);
    assert!(out["note"].as_str().unwrap().contains("三不动"));

    // 留痕：op=create、note 用户备注 + 来源标记；三不动：现金/流水分毫不动
    let s = zimu_agent::lock(&store);
    let edits = s.holding_edits(10).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].op, zimu_store::EditOp::Create);
    assert!(edits[0].note.contains("初始化搬入"));
    assert!(edits[0].note.contains("经智能体录入"));
    assert_eq!(s.trades(50).unwrap().len(), 1); // seeded 那笔，校正不写流水
    // 三不动之现金：仍是 seeded 买入后的原值（11000 + 佣金 5 + 过户费 0.11）
    assert!((s.account().unwrap().cash - (300_000.0 - 11_005.11)).abs() < 1e-9);
    drop(s);

    // 编辑：整行覆盖改 shares，其余字段原样带回
    let out = exec(&tool, json!({
        "code": "000001", "name": "平安银行", "mkt": "SZ", "kind": "stock",
        "sector": "银行", "shares": 200, "cost": 10.5
    })).await;
    assert_eq!(out["changed"], true);
    assert_eq!(out["holding"]["shares"], 200);
    let s = zimu_agent::lock(&store);
    let edits = s.holding_edits(10).unwrap(); // 最新在前
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].op, zimu_store::EditOp::Update);
    assert_eq!(edits[0].before.as_ref().unwrap().shares, 150);
    drop(s);

    // 幂等：与现值逐位一致 → changed=false、不留痕
    let out = exec(&tool, json!({
        "code": "000001", "name": "平安银行", "mkt": "SZ", "kind": "stock",
        "sector": "银行", "shares": 200, "cost": 10.5
    })).await;
    assert_eq!(out["changed"], false);
    assert!(out["note"].as_str().unwrap().contains("幂等"));
    assert_eq!(zimu_agent::lock(&store).holding_edits(10).unwrap().len(), 2);
}

#[tokio::test]
async fn upsert_holding_rejects_bad_input_without_side_effects() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let tool = UpsertHoldingTool { deps: deps_with(store.clone(), fake) };

    // shares=0 → validate_upsert 拒绝（≥1，奇零股也至少 1 股）
    let err = exec_err(&tool, json!({
        "code": "000001", "name": "平安银行", "mkt": "SZ", "kind": "stock",
        "shares": 0, "cost": 10.5
    })).await;
    assert!(err.contains("被拒"), "{err}");
    // 零副作用：无留痕、无新持仓行
    let s = zimu_agent::lock(&store);
    assert!(s.holding_edits(10).unwrap().is_empty());
    assert_eq!(s.holdings().unwrap().len(), 1); // 只有 seeded 600519
}

#[tokio::test]
async fn remove_holding_deletes_row_keeps_trades_and_leaves_delete_trail() {
    let store = seeded_store();
    let fake = FakeApi::arc();
    let tool = RemoveHoldingTool { deps: deps_with(store.clone(), fake) };

    let out = exec(&tool, json!({ "code": "600519" })).await;
    assert_eq!(out["removed"]["code"], "600519");
    assert!(out["note"].as_str().unwrap().contains("流水保留"));
    let s = zimu_agent::lock(&store);
    assert!(s.holdings().unwrap().is_empty());
    assert_eq!(s.trades(50).unwrap().len(), 1); // 流水保留（holding_code 非外键）
    let edits = s.holding_edits(10).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].op, zimu_store::EditOp::Delete);
    assert!(edits[0].before.is_some() && edits[0].after.is_none()); // before 快照可恢复
    drop(s);

    // 再删同一标的 → 如实报错，不假装成功
    let err = exec_err(&tool, json!({ "code": "600519" })).await;
    assert!(err.contains("没有"), "{err}");
}

#[test]
fn snapshot_tools_are_auto_commit_per_adr_0013() {
    // ADR-0013 选项 A：快照校正沿用自动落账 + 读回（删除的框架硬门禁是被否决项 B，
    // 对话内两步确认走人格纪律）。翻回 = 改 requires_confirmation() 为 true。
    let store = seeded_store();
    let fake = FakeApi::arc();
    let deps = deps_with(store, fake);
    assert!(!UpsertHoldingTool { deps: deps.clone() }.requires_confirmation());
    assert!(!RemoveHoldingTool { deps }.requires_confirmation());
}

// ============================================================================
// Skill 模板（仓库根 skills/）
// ============================================================================

#[tokio::test]
async fn skills_load_with_unique_triggers_and_registered_tools() {
    let (skills, errors) = load_zimu_skills();
    assert!(errors.is_empty(), "Skill 加载错误：{errors:?}");
    let names: Vec<&str> = skills.iter().map(|s| s.frontmatter.name.as_str()).collect();
    for expect in ["post-market-review", "pre-market-watch", "generate-advice"] {
        assert!(names.contains(&expect), "缺 Skill {expect}，实有 {names:?}");
    }

    // 触发词唯一且覆盖中英文
    let mut triggers: Vec<&str> = Vec::new();
    for s in &skills {
        triggers.extend(s.frontmatter.triggers.iter().map(|t| t.as_str()));
    }
    for t in ["/review", "/复盘", "/watch", "/盘前", "/advice", "/建议"] {
        assert_eq!(triggers.iter().filter(|x| **x == t).count(), 1, "触发词 {t} 应恰好注册一次");
    }

    // tools_required 必须全部已注册（robit 只 warn，这里收紧成断言）
    let store = seeded_store();
    let registry = build_registry(AgentDeps::new(store, FakeApi::arc()));
    let registered: Vec<&str> = registry.tool_names();
    for s in &skills {
        for required in &s.frontmatter.tools_required {
            assert!(
                registered.contains(&required.as_str()),
                "Skill {} 依赖未注册工具 {required}",
                s.frontmatter.name
            );
        }
        // 每份模板必须内嵌免责与「数字来自工具返回」硬规则（P4-T6 验收项）
        assert!(s.content.contains("不构成投资建议"), "{} 缺免责声明", s.frontmatter.name);
        assert!(s.content.contains("工具返回"), "{} 缺数字来源硬规则", s.frontmatter.name);
    }
}
