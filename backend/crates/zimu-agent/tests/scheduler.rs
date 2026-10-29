//! 调度与预警引擎集成测试（P4-T7，全离线：FakeApi 注入 + in-memory Store +
//! **注入模拟时刻**——验收「模拟盘中可触发」，roadmap P4-T7）。
//!
//! 覆盖（口径断言锚 03 §12）：
//! - 盘中（live）轮询：刷新回写 → 判定 → 预警落库；同日 (code,type) 去重；
//!   stop/target_hi 来自最近一条 advice_history 留档；
//! - 非交易日空转：不打 sidecar；
//! - sidecar 挂：如实降级、不判定（不用旧值）、不瘫；
//! - 15:05 盘后链：equity_snapshots 写入（数字 = recalc agg 原值 + 手算锚点）、
//!   /复盘 注入回调、push_log 留痕；幂等（快照行存在 → 整链跳过）；
//! - agent 未接线：快照照写，push_log 如实记 ok=0；
//! - 转债期限类：cb_list 每日一次（第二轮不再打）；日线量窗口每日每 code 一次；
//!   放量与强赎各自触发。

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_json::json;
use zimu_agent::scheduler::{ReviewTrigger, SchedulerCore};
use zimu_agent::{AgentDeps, SharedStore};
use zimu_domain::Side;
use zimu_market::error::{MarketError, MarketResult};
use zimu_market::models::{
    CalendarPayload, CbItem, CbListPayload, KlineBar, KlinePayload, QuotePayload, QuoteRow,
    QuoteStatus, SidecarHealth,
};
use zimu_market::MarketApi;
use zimu_store::{AccountRow, NewAdvice, NewHoldingMeta, NewWatchlist, Store, TradeRequest};

// ============================================================================
// FakeApi（带调用计数——缓存语义断言用）
// ============================================================================

#[derive(Default)]
struct FakeApi {
    quote: Mutex<Option<QuotePayload>>,
    kline: Mutex<Option<KlinePayload>>,
    cb: Mutex<Option<CbListPayload>>,
    calendar: Mutex<Option<CalendarPayload>>,
    down: AtomicBool,
    quote_calls: AtomicUsize,
    kline_calls: AtomicUsize,
    cb_calls: AtomicUsize,
}

impl FakeApi {
    fn arc() -> Arc<Self> {
        Arc::new(Self::default())
    }
    fn down_err<T>(&self) -> MarketResult<T> {
        Err(MarketError::Unavailable(
            "sidecar 整机不可达（测试注入）".into(),
        ))
    }
}

#[async_trait]
impl MarketApi for FakeApi {
    fn base_url(&self) -> &str {
        "fake://scheduler-test"
    }
    async fn health(&self) -> MarketResult<SidecarHealth> {
        if self.down.load(Ordering::SeqCst) {
            return self.down_err();
        }
        Ok(SidecarHealth {
            ok: true,
            version: Some("test".into()),
            rest: serde_json::Map::new(),
        })
    }
    async fn quote(&self, _symbols: &[String]) -> MarketResult<QuotePayload> {
        self.quote_calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            return self.down_err();
        }
        self.quote
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MarketError::Unavailable("quote 未配置（测试）".into()))
    }
    async fn kline(
        &self,
        _symbol: &str,
        _kind: Option<&str>,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> MarketResult<KlinePayload> {
        self.kline_calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            return self.down_err();
        }
        self.kline
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MarketError::Unavailable("kline 未配置（测试）".into()))
    }
    async fn cb_list(&self) -> MarketResult<CbListPayload> {
        self.cb_calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            return self.down_err();
        }
        self.cb
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MarketError::Unavailable("cb_list 未配置（测试）".into()))
    }
    async fn calendar(
        &self,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> MarketResult<CalendarPayload> {
        if self.down.load(Ordering::SeqCst) {
            return self.down_err();
        }
        self.calendar
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MarketError::Unavailable("calendar 未配置（测试）".into()))
    }
}

// ============================================================================
// 构造器：时刻 / 日历 / 行情 / 日线 / 转债列表 / 种子库 / 复盘回调
// ============================================================================

/// 2026-10-29 是周三；10-31 周六（非交易日空转用例）
fn sh(day: u32, h: u32, m: u32) -> DateTime<FixedOffset> {
    NaiveDate::from_ymd_opt(2026, 10, day)
        .unwrap()
        .and_hms_opt(h, m, 0)
        .unwrap()
        .and_local_timezone(FixedOffset::east_opt(8 * 3600).unwrap())
        .single()
        .unwrap()
}

fn calendar() -> CalendarPayload {
    CalendarPayload {
        start: "2026-10-01".into(),
        end: "2026-12-31".into(),
        count: 3,
        trade_dates: vec!["2026-10-28".into(), "2026-10-29".into(), "2026-10-30".into()],
        source: Some("fake_calendar".into()),
        degraded: false,
        attempts: vec![],
    }
}

fn qrow(symbol: &str, code: &str, kind: &str, price: f64, prev: f64, vol: f64) -> QuoteRow {
    QuoteRow {
        symbol: symbol.into(),
        code: code.into(),
        mkt: symbol[..2].to_uppercase(),
        kind: kind.into(),
        name: None,
        price: Some(price),
        prev: Some(prev),
        open: Some(prev),
        high: Some(price.max(prev)),
        low: Some(price.min(prev)),
        volume: Some(vol),
        amount: None,
        chg_pct: Some((price - prev) / prev * 100.0),
        date: Some("2026-10-29".into()),
        time: Some("10:30:03".into()),
        status: QuoteStatus::Ok,
        source: Some("fake_hq".into()),
    }
}

fn quote_payload(rows: Vec<QuoteRow>) -> QuotePayload {
    let n = rows.len();
    QuotePayload {
        as_of: "2026-10-29T10:30:03+08:00".into(),
        count: n,
        degraded: false,
        attempts: vec![],
        quotes: rows,
    }
}

/// 20 根日线（2026-09-29 起连续 20 天，全部早于「今日」10-29），量恒 1000。
/// 放量判定基线：avg20 = 1000 → 当日量 ≥2000 触发（03 §12 序 6）。
fn kline_payload() -> KlinePayload {
    let start = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    let bars: Vec<KlineBar> = (0..20u64)
        .map(|i| KlineBar {
            date: (start + chrono::Duration::days(i as i64)).format("%Y-%m-%d").to_string(),
            open: Some(100.0),
            high: Some(101.0),
            low: Some(99.0),
            close: Some(100.0),
            volume: Some(1000.0),
            amount: None,
        })
        .collect();
    KlinePayload {
        symbol: "sh600519".into(),
        kind: "stock".into(),
        kind_routed: "explicit".into(),
        adjusted: "qfq".into(),
        start: bars.first().unwrap().date.clone(),
        end: bars.last().unwrap().date.clone(),
        count: bars.len(),
        source: Some("fake_daily".into()),
        degraded: false,
        attempts: vec![],
        kline: bars,
    }
}

fn cb_list_payload() -> CbListPayload {
    CbListPayload {
        as_of: "2026-10-29T10:30:00+08:00".into(),
        count: 1,
        spot_source: Some("fake-cb-spot".into()),
        redeem_source: Some("fake_redeem".into()),
        degraded: false,
        attempts: vec![],
        items: vec![CbItem {
            symbol: "sz128136".into(),
            code: "128136".into(),
            mkt: "SZ".into(),
            kind: "cb".into(),
            name: Some("立讯转债".into()),
            price: Some(132.5),
            prev: Some(131.0),
            open: None,
            high: None,
            low: None,
            volume: Some(100.0),
            amount: None,
            chg_pct: None,
            ticktime: None,
            status: QuoteStatus::Ok,
            spot_source: Some("fake-cb-spot".into()),
            stock_code: None,
            stock_name: None,
            convert_price: None,
            trigger_pct: None,
            trigger_price: None,
            stock_price: None,
            redeem_price: None,
            last_trade_date: Some("2026-11-20".into()), // +22 天：期限窗口外
            maturity_date: None,
            scale: None,
            scale_left: None,
            redeem_status: Some("已公告强赎".into()),
            redeem_hint: None,
            redeem_source: Some("fake_redeem".into()),
        }],
    }
}

/// 种子库：现金 300000，600519 100 股 @110（费用 5.11 → cost 110.0511，
/// 与 tools.rs 同款手算），SH stock。
fn seeded_store() -> SharedStore {
    let s = Store::in_memory().unwrap();
    s.set_account(&AccountRow {
        cash: 300_000.0,
        ..AccountRow::default()
    })
    .unwrap();
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

fn lock(store: &SharedStore) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// 记录注入文本的复盘回调（返回受理成功）
fn recording_trigger(log: Arc<Mutex<Vec<String>>>) -> ReviewTrigger {
    Arc::new(
        move |text: String| -> Pin<Box<dyn Future<Output = bool> + Send>> {
            log.lock().unwrap().push(text);
            Box::pin(async { true })
        },
    )
}

fn core_with(
    store: SharedStore,
    fake: Arc<FakeApi>,
    review: Option<ReviewTrigger>,
) -> Arc<SchedulerCore> {
    SchedulerCore::new(AgentDeps::new(store, fake), review)
}

fn standard_fake() -> Arc<FakeApi> {
    let fake = FakeApi::arc();
    *fake.calendar.lock().unwrap() = Some(calendar());
    *fake.kline.lock().unwrap() = Some(kline_payload());
    fake
}

// ============================================================================
// 盘中轮询：判定 → 落库 → 去重
// ============================================================================

#[tokio::test]
async fn simulated_intraday_triggers_alerts_and_dedups() {
    let store = seeded_store();
    // 最近留档：stop 106 / target_hi 130 → 现价 105 触发止损线，不触发止盈
    lock(&store)
        .insert_advice(&NewAdvice {
            code: "600519".into(),
            action: "持有".into(),
            score: 50.0,
            conf: 0.5,
            target_lo: Some(120.0),
            target_hi: Some(130.0),
            stop: Some(106.0),
            position: "当前 3.7% → 建议 维持".into(),
            dims: json!([]),
            data_version: "adv-v1|k=2026-10-28|q=none".into(),
            generated_at: "2026-10-28 15:05".into(),
        })
        .unwrap();

    let fake = standard_fake();
    // 现价 105 < cost 110.0511 → 跌破成本；≤ stop 106 → 触及止损线；
    // 量 1000 = avg20 → 无放量；105 > limit_down 99.0（110×0.9）→ 无跌停
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sh600519", "600519", "stock", 105.0, 110.0, 1000.0,
    )]));

    let core = core_with(store.clone(), fake.clone(), None);

    // 第一轮（模拟盘中 10:30）：两条预警按判定序落库
    let rep = core.tick(sh(29, 10, 30)).await;
    assert_eq!(rep.state, "am");
    assert!(rep.trade_day && rep.polled);
    assert_eq!(rep.quotes_updated, 1);
    let kinds: Vec<(&str, &str)> = rep
        .new_alerts
        .iter()
        .map(|a| (a.alert_type.as_str(), a.level.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![("跌破成本", "down"), ("触及止损线", "down")],
        "判定序 3 在序 4 前（03 §12）"
    );
    let rows = lock(&store).list_alerts(None, 50).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ts, "2026-10-29 10:30"); // 注入时刻，非真实时钟
    assert_eq!(rows[0].code, "600519");
    assert_eq!(rows[0].name, "贵州茅台");
    assert!(!rows[0].done);
    // text 数字只取输入（展示层模板，03 §12）
    let cost_alert = rows.iter().find(|r| r.alert_type == "跌破成本").unwrap();
    assert!(
        cost_alert.text.contains("105.00") && cost_alert.text.contains("110.05"),
        "text={}",
        cost_alert.text
    );

    // 第二轮（10:31）：条件仍成立但同日 (code,type) 去重 → 零新增
    let rep2 = core.tick(sh(29, 10, 31)).await;
    assert!(rep2.polled && rep2.new_alerts.is_empty());
    assert_eq!(lock(&store).list_alerts(None, 50).unwrap().len(), 2);
    assert_eq!(fake.quote_calls.load(Ordering::SeqCst), 2); // 每分钟一轮
    // 日线量窗口当日缓存：第二轮不再拉 kline
    assert_eq!(fake.kline_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn non_trading_day_idles_without_sidecar_calls() {
    let store = seeded_store();
    let fake = standard_fake();
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sh600519", "600519", "stock", 1.0, 110.0, 0.0,
    )]));
    let core = core_with(store.clone(), fake.clone(), None);

    // 2026-10-31 周六且不在日历 → 休市空转（哪怕价格「跌破」也不判定）
    let rep = core.tick(sh(31, 10, 30)).await;
    assert_eq!(rep.state, "non_trading_day");
    assert!(!rep.trade_day && !rep.polled && rep.notes.is_empty());
    assert_eq!(fake.quote_calls.load(Ordering::SeqCst), 0); // 不打 sidecar
    assert!(lock(&store).list_alerts(None, 50).unwrap().is_empty());

    // 交易日但午休（live=false）：同样不轮询
    let rep = core.tick(sh(29, 12, 0)).await;
    assert_eq!(rep.state, "lunch");
    assert!(!rep.polled);
    assert_eq!(fake.quote_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sidecar_down_degrades_without_alerts_or_panic() {
    let store = seeded_store();
    let fake = FakeApi::arc(); // 日历也未配置：全链不可用
    fake.down.store(true, Ordering::SeqCst);
    let core = core_with(store.clone(), fake.clone(), None);

    // 周三 10:30，日历挂 → 工作日启发式（calendar_degraded note）；行情挂 →
    // 本轮不判定（不用旧值，不伪造），服务不瘫
    let rep = core.tick(sh(29, 10, 30)).await;
    assert_eq!(rep.state, "am"); // 启发式仍认出周三盘中
    assert!(!rep.polled);
    assert!(
        rep.notes.iter().any(|n| n.contains("日历不可用")),
        "notes={:?}",
        rep.notes
    );
    assert!(
        rep.notes.iter().any(|n| n.contains("本轮跳过预警判定")),
        "notes={:?}",
        rep.notes
    );
    assert!(lock(&store).list_alerts(None, 50).unwrap().is_empty());

    // sidecar 恢复 → 下一轮自愈（价格 105 < cost → 跌破成本落库）
    fake.down.store(false, Ordering::SeqCst);
    *fake.calendar.lock().unwrap() = Some(calendar());
    *fake.kline.lock().unwrap() = Some(kline_payload());
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sh600519", "600519", "stock", 105.0, 110.0, 1000.0,
    )]));
    let rep = core.tick(sh(29, 10, 31)).await;
    assert!(rep.polled);
    assert_eq!(rep.new_alerts.len(), 1);
    assert_eq!(rep.new_alerts[0].alert_type, "跌破成本");
}

// ============================================================================
// 15:05 盘后链：快照 + 复盘触发 + push_log
// ============================================================================

#[tokio::test]
async fn after_close_writes_snapshot_triggers_review_once() {
    let store = seeded_store();
    let fake = standard_fake();
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sh600519", "600519", "stock", 112.0, 110.0, 3000.0,
    )]));
    let reviewed = Arc::new(Mutex::new(Vec::new()));
    let core = core_with(
        store.clone(),
        fake.clone(),
        Some(recording_trigger(reviewed.clone())),
    );

    // 15:04 未到 → 不触发
    let rep = core.tick(sh(29, 15, 4)).await;
    assert!(!rep.snapshot_written && rep.review_ok.is_none());
    assert!(lock(&store).equity_snapshots(10).unwrap().is_empty());

    // 15:05 首轮 → 快照 + 复盘 + push_log
    let rep = core.tick(sh(29, 15, 5)).await;
    assert_eq!(rep.state, "after_hours");
    assert!(rep.snapshot_written);
    assert_eq!(rep.review_ok, Some(true));
    assert_eq!(*reviewed.lock().unwrap(), vec!["/复盘".to_string()]);

    // 快照数字 = recalc agg 原值（逐位）+ 手算锚点（03 §12 / §5 / §3）：
    // 买入费用 5.11 → cash 288994.89、cost 110.0511；price 112 prev 110 →
    // total_mv 11200、total_assets 300194.89、day_pnl 200、total_pnl 194.89
    let snap = lock(&store).equity_snapshot("2026-10-29").unwrap().unwrap();
    assert_eq!(snap.ts, "2026-10-29 15:05");
    assert!((snap.cash - 288_994.89).abs() < 1e-6, "cash={}", snap.cash);
    assert!((snap.total_mv - 11_200.0).abs() < 1e-9);
    assert!(
        (snap.total_assets - 300_194.89).abs() < 1e-6,
        "total_assets={}",
        snap.total_assets
    );
    assert!((snap.day_pnl - 200.0).abs() < 1e-9);
    assert!(
        (snap.day_pnl_pct - 200.0 / 299_994.89 * 100.0).abs() < 1e-9,
        "day_pnl_pct={}",
        snap.day_pnl_pct
    );
    assert!((snap.total_pnl - 194.89).abs() < 1e-6);
    let view = lock(&store).portfolio().unwrap();
    assert_eq!(snap.total_assets, view.agg.total_assets);
    assert_eq!(snap.exposure, view.agg.exposure);
    assert_eq!(snap.hhi, view.agg.hhi);
    assert_eq!(snap.top3_sector, view.agg.top3_sector);

    // push_log：channel=agent kind=post_market_review ok=1
    let logs = lock(&store).push_logs(10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].channel, "agent");
    assert_eq!(logs[0].kind, "post_market_review");
    assert!(logs[0].ok && logs[0].detail.contains("复盘已注入"));

    // 15:06 第二轮 → 幂等：快照不重写、复盘不重触发（幂等闸 = 快照行存在）
    let rep2 = core.tick(sh(29, 15, 6)).await;
    assert!(!rep2.snapshot_written && rep2.review_ok.is_none());
    assert_eq!(lock(&store).equity_snapshots(10).unwrap().len(), 1);
    assert_eq!(reviewed.lock().unwrap().len(), 1);
    assert_eq!(lock(&store).push_logs(10).unwrap().len(), 1);

    // 当晚重启（新 core = 空日级缓存）→ 快照仍在，整链依旧跳过
    let core2 = core_with(store.clone(), fake.clone(), Some(recording_trigger(reviewed.clone())));
    let rep3 = core2.tick(sh(29, 20, 1)).await;
    assert!(!rep3.snapshot_written && rep3.review_ok.is_none());
    assert_eq!(reviewed.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn after_close_without_agent_records_push_log_failure() {
    let store = seeded_store();
    let fake = standard_fake();
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sh600519", "600519", "stock", 112.0, 110.0, 3000.0,
    )]));
    let core = core_with(store.clone(), fake.clone(), None); // agent 未接线

    let rep = core.tick(sh(29, 15, 5)).await;
    assert!(rep.snapshot_written); // 快照照写
    assert_eq!(rep.review_ok, Some(false));
    let logs = lock(&store).push_logs(10).unwrap();
    assert_eq!(logs.len(), 1);
    assert!(!logs[0].ok && logs[0].detail.contains("agent 未接线"));
}

#[tokio::test]
async fn after_close_defers_when_final_refresh_fails() {
    let store = seeded_store();
    let fake = standard_fake();
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sh600519", "600519", "stock", 112.0, 110.0, 3000.0,
    )]));
    let core = core_with(store.clone(), fake.clone(), None);

    // 15:05 但行情挂 → 整链顺延（宁缺不写旧数），快照/日志都不落
    fake.down.store(true, Ordering::SeqCst);
    let rep = core.tick(sh(29, 15, 5)).await;
    assert!(!rep.snapshot_written && rep.review_ok.is_none());
    assert!(rep.notes.iter().any(|n| n.contains("快照顺延")));
    assert!(lock(&store).equity_snapshots(10).unwrap().is_empty());

    // 恢复后下一轮补跑（含「白天宕机晚间重启」同款路径）
    fake.down.store(false, Ordering::SeqCst);
    let rep = core.tick(sh(29, 20, 30)).await;
    assert!(rep.snapshot_written);
    assert_eq!(
        lock(&store).equity_snapshot("2026-10-29").unwrap().unwrap().ts,
        "2026-10-29 20:30"
    );
}

// ============================================================================
// 转债期限类 + 量能窗口缓存
// ============================================================================

#[tokio::test]
async fn cb_redeem_and_vol_surge_with_daily_caches() {
    let store = seeded_store();
    // 加一笔转债：128136 10 张 @130（SZ cb；cost ≈ 130.05，现价 132.5 不触发跌破成本）
    lock(&store)
        .record_trade(&TradeRequest {
            code: "128136".into(),
            side: Side::Buy,
            qty: 10,
            price: 130.0,
            note: String::new(),
            meta: Some(NewHoldingMeta {
                name: "立讯转债".into(),
                mkt: "SZ".into(),
                kind: zimu_domain::Kind::Cb,
                sector: "转债".into(),
                prev: None,
                limit_pct: None,
            }),
        })
        .unwrap();

    let fake = standard_fake();
    *fake.cb.lock().unwrap() = Some(cb_list_payload());
    // 股票当日量 5000 = 5×avg20(1000) → 放量 up；转债量 100 → 缩量不触发
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![
        qrow("sh600519", "600519", "stock", 112.0, 110.0, 5000.0),
        qrow("sz128136", "128136", "cb", 132.5, 131.0, 100.0),
    ]));
    let core = core_with(store.clone(), fake.clone(), None);

    let rep = core.tick(sh(29, 10, 30)).await;
    let mut kinds: Vec<(String, String, String)> = rep
        .new_alerts
        .iter()
        .map(|a| (a.code.clone(), a.alert_type.clone(), a.level.clone()))
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        vec![
            ("128136".to_string(), "强赎公告".to_string(), "warn".to_string()),
            ("600519".to_string(), "放量".to_string(), "up".to_string()),
        ]
    );
    // 强赎公告 text 带状态与最后交易日（事实模板）
    let rows = lock(&store).list_alerts(None, 50).unwrap();
    let redeem = rows.iter().find(|r| r.alert_type == "强赎公告").unwrap();
    assert!(
        redeem.text.contains("已公告强赎") && redeem.text.contains("2026-11-20"),
        "text={}",
        redeem.text
    );
    // 11-20 距 10-29 有 22 天 > 5 → 不触发临近到期
    assert!(!rows.iter().any(|r| r.alert_type == "临近到期"));

    // 日级缓存：第二轮 cb_list 不再打（每日一次）、kline 不再拉（每 code 每日一次）
    let cb_calls_1 = fake.cb_calls.load(Ordering::SeqCst);
    let kline_calls_1 = fake.kline_calls.load(Ordering::SeqCst);
    assert_eq!(cb_calls_1, 1);
    assert_eq!(kline_calls_1, 2); // 两个持仓各一次
    let rep2 = core.tick(sh(29, 10, 31)).await;
    assert!(rep2.new_alerts.is_empty()); // 同日去重
    assert_eq!(fake.cb_calls.load(Ordering::SeqCst), cb_calls_1);
    assert_eq!(fake.kline_calls.load(Ordering::SeqCst), kline_calls_1);
}

// ============================================================================
// 自选（B13 定稿 2026-10-29：§12 范围扩展——无持仓形态判定、行情不回写）
// ============================================================================

#[tokio::test]
async fn watchlist_alerts_use_positionless_shape_and_dedup() {
    // 空持仓 + 自选一只（002415 SZ ±10%）：现价 9.0 = 跌停线（prev 10.0×0.9，
    // 含边界）；当日量 5000 = 5× 20 日均量 1000 → 放量（chg −10% < 0 → down）。
    // advice_history 虽有该标的留档（stop 9.2 / target_hi 9.5），自选一律按
    // 无持仓处理：cost/stop/target_hi 不喂 → 序 3/4/5 不参与（03 §12）
    let s = Store::in_memory().unwrap();
    s.set_account(&AccountRow {
        cash: 100_000.0,
        ..AccountRow::default()
    })
    .unwrap();
    s.add_watchlist(
        &NewWatchlist {
            code: "002415".into(),
            name: "海康威视".into(),
            mkt: "SZ".into(),
            kind: zimu_domain::Kind::Stock,
            limit_pct: Some(10),
            sector: "电子".into(),
            note: "自选测试".into(),
        },
        "2026-10-29 09:00",
    )
    .unwrap();
    s.insert_advice(&NewAdvice {
        code: "002415".into(),
        action: "持有".into(),
        score: 60.0,
        conf: 0.5,
        target_lo: None,
        target_hi: Some(9.5),
        stop: Some(9.2),
        position: "当前 0% → 建议 观察".into(),
        dims: json!([]),
        data_version: "adv-v1|k=none|q=none".into(),
        generated_at: "2026-10-28 15:05".into(),
    })
    .unwrap();
    let store: SharedStore = Arc::new(Mutex::new(s));

    let fake = standard_fake();
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sz002415", "002415", "stock", 9.0, 10.0, 5000.0,
    )]));

    let core = core_with(store.clone(), fake.clone(), None);
    let rep = core.tick(sh(29, 10, 30)).await;
    assert!(rep.polled);
    let kinds: Vec<(&str, &str)> = rep
        .new_alerts
        .iter()
        .map(|a| (a.alert_type.as_str(), a.level.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![("触及跌停", "down"), ("放量", "down")],
        "序 2 在序 6 前；止损线（stop 9.2 ≥ price 9.0）因自选无持仓形态不参与"
    );
    assert!(rep.new_alerts.iter().all(|a| a.code == "002415"));

    // 自选行情不回写：holdings 表保持空（holdings 表只存持仓，§12）
    assert_eq!(lock(&store).holdings().unwrap().len(), 0);
    // 空持仓 → 刷新链未打 sidecar；quote 仅自选合并请求一次
    assert_eq!(fake.quote_calls.load(Ordering::SeqCst), 1);
    // 日线量窗口与持仓共用同一日级缓存（每 code 每日一次）
    assert_eq!(fake.kline_calls.load(Ordering::SeqCst), 1);

    // 第二轮：同日 (code,type) 去重（alerts 表跨范围共用）；缓存不再打
    let rep2 = core.tick(sh(29, 10, 31)).await;
    assert!(rep2.new_alerts.is_empty());
    assert_eq!(fake.kline_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn held_code_in_watchlist_is_not_double_judged() {
    // 持有中的代码重复进自选 → 只按持仓形态判定一次（不重复拉行情、不双份预警）
    let store = seeded_store(); // 600519 100 股 @110（cost 110.0511）
    lock(&store)
        .add_watchlist(
            &NewWatchlist {
                code: "600519".into(),
                name: "贵州茅台".into(),
                mkt: "SH".into(),
                kind: zimu_domain::Kind::Stock,
                limit_pct: Some(10),
                sector: "白酒".into(),
                note: "重叠测试".into(),
            },
            "2026-10-29 09:00",
        )
        .unwrap();
    let fake = standard_fake();
    // 现价 105 < cost 110.0511 → 仅「跌破成本」一条（无留档 → 止损类不参与）
    *fake.quote.lock().unwrap() = Some(quote_payload(vec![qrow(
        "sh600519", "600519", "stock", 105.0, 110.0, 1000.0,
    )]));
    let core = core_with(store.clone(), fake.clone(), None);
    let rep = core.tick(sh(29, 10, 30)).await;
    let kinds: Vec<&str> = rep.new_alerts.iter().map(|a| a.alert_type.as_str()).collect();
    assert_eq!(kinds, vec!["跌破成本"], "持仓形态判定一次，自选侧不重复");
    assert_eq!(fake.quote_calls.load(Ordering::SeqCst), 1); // 仅刷新链一次，自选未单独拉
}
