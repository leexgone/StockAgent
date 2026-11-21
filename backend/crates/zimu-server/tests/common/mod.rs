//! 共享测试装置（P4-T4 集成测试 / P4-T5 契约测试共用）：
//! FakeApi（MarketApi 注入点）+ 临时端口起服务 + 确定性种子样本。
//! 全部离线：行情由 FakeApi 注入，真实 HTTP 打本机回环临时端口。
#![allow(dead_code)] // 消费方（market_api.rs / contract.rs）各用一部分

use std::sync::{Arc, Mutex};

use zimu_domain::{Kind, Side};
use zimu_market::clock::now_shanghai;
use zimu_market::error::{MarketError, MarketResult};
use zimu_market::models::*;
use zimu_market::MarketApi;
use zimu_server::holdings_api::SharedStore;
use zimu_server::{api_router, market_api};
use zimu_store::{AccountRow, NewHoldingMeta, Store, TradeRequest};

// ============================================================================
// FakeApi（MarketApi 注入点，默认全不可达 = sidecar 挂了）
// ============================================================================

type Shared<T> = Arc<Mutex<T>>;

#[derive(Clone)]
pub struct FakeApi {
    quote: Shared<MarketResult<QuotePayload>>,
    kline: Shared<MarketResult<KlinePayload>>,
    cb: Shared<MarketResult<CbListPayload>>,
    calendar: Shared<MarketResult<CalendarPayload>>,
    sector_map: Shared<MarketResult<SectorMapPayload>>,
    health: Shared<MarketResult<SidecarHealth>>,
    calls: Shared<Vec<String>>,
}

pub fn down<T>() -> MarketResult<T> {
    Err(MarketError::Unavailable("fake：sidecar 未启动".into()))
}

impl FakeApi {
    pub fn new() -> Self {
        FakeApi {
            quote: Arc::new(Mutex::new(down())),
            kline: Arc::new(Mutex::new(down())),
            cb: Arc::new(Mutex::new(down())),
            calendar: Arc::new(Mutex::new(down())),
            sector_map: Arc::new(Mutex::new(down())),
            health: Arc::new(Mutex::new(down())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub fn set_quote(&self, r: MarketResult<QuotePayload>) {
        *self.quote.lock().unwrap() = r;
    }
    pub fn set_kline(&self, r: MarketResult<KlinePayload>) {
        *self.kline.lock().unwrap() = r;
    }
    pub fn set_cb(&self, r: MarketResult<CbListPayload>) {
        *self.cb.lock().unwrap() = r;
    }
    pub fn set_calendar(&self, r: MarketResult<CalendarPayload>) {
        *self.calendar.lock().unwrap() = r;
    }
    pub fn set_sector_map(&self, r: MarketResult<SectorMapPayload>) {
        *self.sector_map.lock().unwrap() = r;
    }
    pub fn set_health(&self, r: MarketResult<SidecarHealth>) {
        *self.health.lock().unwrap() = r;
    }
    pub fn call_log(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn log(&self, name: &str) {
        self.calls.lock().unwrap().push(name.into());
    }
}

#[async_trait::async_trait]
impl MarketApi for FakeApi {
    fn base_url(&self) -> &str {
        "fake://sidecar"
    }
    async fn health(&self) -> MarketResult<SidecarHealth> {
        self.log("health");
        self.health.lock().unwrap().clone()
    }
    async fn quote(&self, symbols: &[String]) -> MarketResult<QuotePayload> {
        self.log(&format!("quote:{}", symbols.join(",")));
        self.quote.lock().unwrap().clone()
    }
    async fn kline(
        &self,
        symbol: &str,
        _kind: Option<&str>,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> MarketResult<KlinePayload> {
        self.log(&format!("kline:{symbol}"));
        self.kline.lock().unwrap().clone()
    }
    async fn cb_list(&self) -> MarketResult<CbListPayload> {
        self.log("cb_list");
        self.cb.lock().unwrap().clone()
    }
    async fn calendar(
        &self,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> MarketResult<CalendarPayload> {
        self.log("calendar");
        self.calendar.lock().unwrap().clone()
    }
    async fn sector_map(&self) -> MarketResult<SectorMapPayload> {
        self.log("sector_map");
        self.sector_map.lock().unwrap().clone()
    }
}

// ============================================================================
// 装配与样本构造
// ============================================================================

/// 缺省重置钩子：恒报 agent 离线（false）。chat reset 的在线行为在
/// tests/chat_api.rs 注入自己的钩子断言，其余测试不关心。
pub fn no_reset_hook() -> zimu_server::chat_api::ResetHook {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    Arc::new(move || -> Pin<Box<dyn Future<Output = bool> + Send>> {
        Box::pin(async { false })
    })
}

pub async fn spawn_api(fake: FakeApi, store: Store) -> String {
    spawn_api_with_pending(fake, store, Default::default()).await
}

/// 待绑定队列注入变体（契约测试种子 pending 记录用；ADR-0017 D3 补记）
pub async fn spawn_api_with_pending(
    fake: FakeApi,
    store: Store,
    pending: zimu_server::pending_bindings::PendingBindings,
) -> String {
    let shared: SharedStore = Arc::new(Mutex::new(store));
    let router = api_router(shared, market_api::new_market(Arc::new(fake)), no_reset_hook(), pending, Vec::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

pub fn qrow(
    symbol: &str,
    kind: &str,
    price: Option<f64>,
    prev: Option<f64>,
    status: QuoteStatus,
    date: Option<&str>,
) -> QuoteRow {
    QuoteRow {
        symbol: symbol.into(),
        code: symbol[2..].into(),
        mkt: symbol[..2].to_uppercase(),
        kind: kind.into(),
        name: Some(format!("样本{symbol}")),
        price,
        prev,
        open: price,
        high: price,
        low: price,
        volume: Some(1000.0),
        amount: Some(100000.0),
        chg_pct: None,
        date: date.map(String::from),
        time: Some("15:00:03".into()),
        status,
        source: Some("sina_hq".into()),
    }
}

pub fn qpayload(quotes: Vec<QuoteRow>) -> QuotePayload {
    let count = quotes.len();
    QuotePayload {
        as_of: "2026-10-28T15:40:01+08:00".into(),
        count,
        degraded: false,
        attempts: vec![],
        quotes,
    }
}

pub fn bar(date: &str, close: f64) -> KlineBar {
    KlineBar {
        date: date.into(),
        open: Some(close),
        high: Some(close),
        low: Some(close),
        close: Some(close),
        volume: Some(10000.0),
        amount: Some(1e7),
    }
}

/// 三标的持仓（现金充裕）：茅台 100@1500、300ETF 1000@4、立讯转债 10@120。
/// meta 全不带 prev/limit_pct——留给行情回写填充（P4-T4 链路）。
pub fn seed() -> Store {
    let s = Store::in_memory().unwrap();
    s.set_account(&AccountRow {
        cash: 300_000.0,
        ..Default::default()
    })
    .unwrap();
    let buy = |code: &str, name: &str, mkt: &str, kind: Kind, sector: &str, qty: i64, price: f64| {
        TradeRequest {
            code: code.into(),
            side: Side::Buy,
            qty,
            price,
            note: String::new(),
            meta: Some(NewHoldingMeta {
                name: name.into(),
                mkt: mkt.into(),
                kind,
                sector: sector.into(),
                prev: None,
                limit_pct: None,
            }),
        }
    };
    s.record_trade(&buy("600519", "贵州茅台", "SH", Kind::Stock, "白酒", 100, 1500.0))
        .unwrap();
    s.record_trade(&buy("510300", "沪深300ETF", "SH", Kind::Etf, "宽基指数", 1000, 4.0))
        .unwrap();
    s.record_trade(&buy("128136", "立讯转债", "SZ", Kind::Cb, "有色金属", 10, 120.0))
        .unwrap();
    s
}

pub fn today() -> String {
    now_shanghai().date_naive().format("%Y-%m-%d").to_string()
}

pub fn yesterday() -> String {
    (now_shanghai().date_naive() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string()
}

/// 全字段转债行 + 单元素列表（契约测试样本：13 个 redeem 键全有值）
pub fn cb_list_payload() -> CbListPayload {
    let item = CbItem {
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
        volume: None,
        amount: None,
        chg_pct: Some(1.145),
        ticktime: Some("2026-10-28 15:00:00".into()),
        status: QuoteStatus::Ok,
        spot_source: Some("sina-cb-spot".into()),
        stock_code: Some("002475".into()),
        stock_name: Some("立讯精密".into()),
        convert_price: Some(56.35),
        trigger_pct: Some(130.0),
        trigger_price: Some(73.26),
        stock_price: Some(70.0),
        redeem_price: Some(100.5),
        last_trade_date: Some("2026-11-20".into()),
        maturity_date: Some("2026-12-01".into()),
        scale: Some(30.0),
        scale_left: Some(2.1),
        redeem_status: Some("已公告强赎".into()),
        redeem_hint: Some("强赎 15/30".into()),
        redeem_source: Some("jsl_cb_redeem".into()),
    };
    CbListPayload {
        as_of: "2026-10-28T15:40:01+08:00".into(),
        count: 1,
        spot_source: Some("sina_cb_spot".into()),
        redeem_source: Some("jsl_cb_redeem".into()),
        degraded: false,
        attempts: vec![],
        items: vec![item],
    }
}

/// 单交易日日历（窗口字段与 sidecar 默认窗口同形）
pub fn calendar_payload(today: &str) -> CalendarPayload {
    CalendarPayload {
        start: "2026-04-30".into(),
        end: "2027-04-26".into(),
        count: 1,
        trade_dates: vec![today.to_string()],
        source: Some("sina_calendar".into()),
        degraded: false,
        attempts: vec![],
    }
}

/// 单 bar 日线负载（尾部 = 上一交易日：收盘后源侧滞后形态，当日 bar 需合成）
pub fn kline_payload(symbol: &str, kind: &str, tail_close: f64, today: &str) -> KlinePayload {
    KlinePayload {
        symbol: symbol.into(),
        kind: kind.into(),
        kind_routed: "explicit".into(),
        adjusted: "qfq".into(),
        start: "2026-10-01".into(),
        end: today.to_string(),
        count: 1,
        source: Some("sina_stock_daily".into()),
        degraded: false,
        attempts: vec![],
        kline: vec![bar(&yesterday(), tail_close)],
    }
}
