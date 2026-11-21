//! 调度与预警引擎（P4-T7，03 §12 生产口径）。
//!
//! 职责边界（照抄 §12）：
//! - **本模块只做编排**——何时轮询（市场时钟 §9）、取哪些事实（行情/日线/
//!   最近留档/转债强赎）、去重落库（同日 (code,type) 至多一条）、15:05 盘后链
//!   （快照 + 复盘触发）。**预警判定的数字口径全在 `zimu-domain::alert`**，
//!   这里不比较任何业务阈值。
//! - 判定范围 = 持仓 **+ 自选**（B13 定稿 2026-10-29）：自选按「无持仓形态」
//!   喂判定（cost/stop/target_hi = None → 序 3/4/5 不参与），行情单独合并一次
//!   拉取且**不回写库**；范围扩展全在本编排层，判定函数对两种范围零差别。
//! - 行情只经 `zimu-market`（sidecar 唯一消费入口）；刷新回写复用
//!   [`crate::service::refresh_portfolio_detailed`]（与 HTTP refresh 同一条链）。
//! - 降级如实：行情失败该轮**不判定**（不用库存旧价，不伪造）；日历不可用
//!   退工作日启发式并记 note；转债强赎源降级 → 期限类当日不参与。
//!
//! 可测试性：[`SchedulerCore::tick`] 接受注入的 `now`——「模拟盘中可触发」
//! 验收（roadmap P4-T7）靠它离线跑：FakeApi + 任意时刻，不需要真等交易时段。
//! [`run`] 只是 tick + sleep 的实时薄壳。
//!
//! 复盘触发经 [`ReviewTrigger`] 回调注入（server 侧接 `WebBridge::send_user_text`），
//! 本 crate 不依赖 robit-web——CLI/测试可传 None（快照照写，push_log 如实记未触发）。

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Days, FixedOffset, NaiveDate, Timelike};
use zimu_domain::alert::{self, AlertItem};
use zimu_domain::price_limit::{limit_pct, limit_prices};
use zimu_domain::Kind;
use zimu_market::clock::{market_clock, now_shanghai};
use zimu_market::models::{QuoteRow, QuoteStatus};
use zimu_market::symbols::symbol_of;
use zimu_store::{EquitySnapshotRow, HoldingRow, NewAlert, WatchlistRow};

use crate::service::{self, RefreshError};
use crate::{lock, AgentDeps};

/// 缺省轮询间隔（03 §12 常量表：60 秒；`04` 附录盘中频率结论支撑）
pub const POLL_INTERVAL: Duration = Duration::from_secs(60);
/// 盘后链时刻（03 §12）：交易日 ≥15:05 首轮触发，幂等 = 当日快照行存在
const REVIEW_AT_MINS: u32 = 15 * 60 + 5;
/// 日线回看窗口（自然日）：够 20 个交易日量 + 长假余量
const KLINE_LOOKBACK_DAYS: u64 = 45;
/// 量能窗口上限：近 20 根日线（不含今日，03 §12 序 6）
const VOL_WINDOW: usize = 20;
/// 复盘 Skill 触发词（skills/post-market-review）
const REVIEW_TRIGGER_TEXT: &str = "/复盘";

/// 复盘注入回调：返回是否被 agent 会话受理（受理 ≠ 产出完成，产出以
/// advice_history 留档与会话事件为准——03 §12 留档）。
pub type ReviewTrigger =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// 预警推送回调（T6/ADR-0011；ADR-0019 批面）：收到**本轮全部新预警**（≥1 条，
/// 空批不调用）的落库原值（name/alert_type/level/text），返回这批消息是否送达
/// 至少一个目标（合并后送达粒度是消息级，不存在条级计数）。**文案组装（N=1
/// 单条格式 / N≥2 合并一条）、多目标循环与 push_log 留痕全在宿主侧**——本
/// crate 只说「推这一批」，不认通道、不依赖 robit（scheduler.rs 头部纪律）。
pub type AlertPusher = Arc<
    dyn Fn(Vec<NewAlertInfo>) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync,
>;

/// 盘后摘要推送回调（T6）：携带当日 `equity_snapshots` 行原值（数字 = recalc
/// agg 原值，03 §12 不现编）；返回是否送达至少一个目标。同样：格式化与留痕归宿主。
pub type SummaryPusher = Arc<
    dyn Fn(zimu_store::EquitySnapshotRow) -> Pin<Box<dyn Future<Output = bool> + Send>>
        + Send
        + Sync,
>;

/// 单轮调度的事实汇报（测试断言面 + 日志素材；不落库）
#[derive(Debug, Default)]
pub struct TickReport {
    /// 市场时钟状态（§9：am/pm/lunch/closed/non_trading_day/…）
    pub state: String,
    pub trade_day: bool,
    /// 本轮是否完成了行情轮询（live 时段且刷新链成功）
    pub polled: bool,
    pub quotes_updated: usize,
    pub new_alerts: Vec<NewAlertInfo>,
    /// 预警推送送达的数量（T6；ADR-0019 批语义：整批一条消息，送达即全批
    /// 条数、失败即 0；未接推送恒 0）
    pub alerts_pushed: usize,
    pub snapshot_written: bool,
    /// None = 本轮未触发复盘链（时刻未到/当日快照已存在）
    pub review_ok: Option<bool>,
    /// 盘后摘要推送（T6）：None = 本轮未走盘后链或未接推送
    pub summary_pushed: Option<bool>,
    /// 降级与错误明细（如实上报，不吞）
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewAlertInfo {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub alert_type: String,
    pub level: String,
    /// 预警文案原值（zimu-domain::alert 判定产出，03 §12）——推送侧直接用，
    /// 不再二次加工数字
    pub text: String,
}

/// 转债期限类事实（cb_list 每日一次的缓存值）
#[derive(Debug, Clone)]
struct CbFacts {
    redeem_status: Option<String>,
    last_trade_date: Option<String>,
    maturity_date: Option<String>,
}

/// 日级缓存：跨日自动重置（tick 里对 date）
#[derive(Debug, Default)]
struct DailyState {
    date: String,
    /// code → 近 20 根日线量（不含今日）；None = 当日日线获取失败
    /// （放量类当日不参与判定——不伪造，也不逐分钟重试白耗请求）
    vol_hist: HashMap<String, Option<Vec<Option<f64>>>>,
    /// code → 强赎/期限字段（cb_list 每日一次）
    cb: HashMap<String, CbFacts>,
    /// cb_list 今日是否已成功拉过（失败不置位 → 下一轮重试）
    cb_checked: bool,
}

impl DailyState {
    fn new(date: String) -> Self {
        Self {
            date,
            ..Default::default()
        }
    }
}

pub struct SchedulerCore {
    deps: Arc<AgentDeps>,
    review: Option<ReviewTrigger>,
    /// 通道推送（T6）：None = 宿主未接（与 review 同构的降级语义）
    push_alert: Option<AlertPusher>,
    push_summary: Option<SummaryPusher>,
    daily: Mutex<DailyState>,
}

impl SchedulerCore {
    pub fn new(
        deps: Arc<AgentDeps>,
        review: Option<ReviewTrigger>,
        push_alert: Option<AlertPusher>,
        push_summary: Option<SummaryPusher>,
    ) -> Arc<Self> {
        Arc::new(Self {
            deps,
            review,
            push_alert,
            push_summary,
            daily: Mutex::new(DailyState::default()),
        })
    }

    fn daily(&self) -> MutexGuard<'_, DailyState> {
        self.daily.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 一轮调度（`now` 注入——测试用模拟时刻，生产用 [`now_shanghai`]）。
    /// 任何一步失败都不 panic：降级进 `notes`，下一轮自愈。
    pub async fn tick(&self, now: DateTime<FixedOffset>) -> TickReport {
        let mut rep = TickReport::default();

        // 时钟（§9 生产口径）：日历缓存自带降级链（实拉 → 旧缓存 → 启发式）
        let view = self.deps.calendar.view_for_clock(&*self.deps.market).await;
        let clock = market_clock(now, &view);
        rep.state = clock.state.to_string();
        if clock.calendar_degraded {
            rep.notes
                .push("日历不可用：交易日判定退化为工作日启发式（未排除节假日）".into());
        }
        if clock.state == "non_trading_day" {
            return rep; // 休市空转，不打 sidecar（03 §12）
        }
        rep.trade_day = true;
        let today = clock.date.clone();

        // 跨日重置日级缓存
        {
            let mut d = self.daily();
            if d.date != today {
                *d = DailyState::new(today.clone());
            }
        }

        // 盘中分钟级轮询（集合竞价/早盘/午盘 = clock.live）
        if clock.live {
            self.poll_once(&today, now, &mut rep).await;
            // 新预警推送（T6；ADR-0019 轮内合并）：落库与同日去重在 poll_once 已
            // 完成——本轮全部新预警作为**一批**交宿主（N=1 仍单条格式、N≥2 合并
            // 一条，零延迟）；空批不调用（避免「本轮 0 条」废消息）。批送达 =
            // 全批条数、批失败 = 0；送达失败不重试（下轮已被去重挡住，如实接受），
            // 宿主留痕。
            if let Some(pusher) = &self.push_alert {
                if !rep.new_alerts.is_empty() && pusher(rep.new_alerts.clone()).await {
                    rep.alerts_pushed = rep.new_alerts.len();
                }
            }
        }

        // 盘后链（≥15:05；幂等 = 当日快照行存在，含补跑语义）
        if now.hour() * 60 + now.minute() >= REVIEW_AT_MINS {
            self.after_close(&today, now, &mut rep).await;
        }
        rep
    }

    /// 盘中一轮：刷新链（回写 + recalc）→ 日级数据缓存 → 判定 → 去重落库
    async fn poll_once(&self, today: &str, now: DateTime<FixedOffset>, rep: &mut TickReport) {
        // 1) 行情刷新（与 HTTP refresh 同链）。失败 → 本轮不判定：
        //    不用库存旧价判——不伪造（03 §12）
        let (resp, quotes) =
            match service::refresh_portfolio_detailed(&self.deps.store, &*self.deps.market, &self.deps.sector_map).await {
                Ok(v) => v,
                Err(RefreshError::Market(e)) => {
                    rep.notes
                        .push(format!("行情刷新失败，本轮跳过预警判定（不用旧值判）：{e}"));
                    return;
                }
                Err(RefreshError::Store(m)) => {
                    rep.notes.push(format!("刷新链存储错误：{m}"));
                    return;
                }
            };
        rep.polled = true;
        rep.quotes_updated = resp.updated;
        for s in &resp.skipped {
            rep.notes
                .push(format!("{}：{}（本轮预警判定跳过该标的）", s.code, s.reason));
        }

        let holdings = match lock(&self.deps.store).holdings() {
            Ok(h) => h,
            Err(e) => {
                rep.notes.push(format!("读取持仓失败：{e}"));
                return;
            }
        };
        // 自选（B13 定稿 2026-10-29，03 §12 范围扩展）：与持仓重叠的代码走持仓
        // 形态（刷新链已含其行情行），不重复拉取——两种形态不会同时判定
        let watch: Vec<WatchlistRow> = match lock(&self.deps.store).watchlist() {
            Ok(w) => w,
            Err(e) => {
                rep.notes
                    .push(format!("读取自选失败（本轮自选不参与判定）：{e}"));
                Vec::new()
            }
        };
        let held: HashSet<&str> = holdings.iter().map(|h| h.code.as_str()).collect();
        let extra: Vec<WatchlistRow> = watch
            .into_iter()
            .filter(|w| !held.contains(w.code.as_str()))
            .collect();
        if holdings.is_empty() && extra.is_empty() {
            return;
        }

        // 2) 日级事实：转债强赎（每日一次，持仓**或自选**含转债才打）+
        //    各标的日线量窗口（每日每 code 一次，持仓与自选共用缓存）
        self.ensure_daily(today, &holdings, &extra, rep).await;

        // 3) 自选行情：单独一次合并请求（**不回写库**——holdings 表只存持仓，
        //    03 §12）。失败只降级自选侧：本轮自选不判定（不用旧值），持仓侧照常
        let mut w_quotes: Vec<QuoteRow> = Vec::new();
        if !extra.is_empty() {
            let syms: Vec<String> = extra
                .iter()
                .filter_map(|w| symbol_of(&w.mkt, &w.code))
                .collect();
            match self.deps.market.quote(&syms).await {
                Ok(p) => w_quotes = p.quotes,
                Err(e) => rep.notes.push(format!(
                    "自选行情获取失败，本轮自选跳过判定（不用旧值判）：{e}"
                )),
            }
        }

        // 4) 组装判定输入 → 规则函数出判定（数字口径只在 zimu-domain::alert）
        //    → 同日 (code,type) 去重 → 落库。本段无 await，daily 锁可安全持有。
        let ts = now.format("%Y-%m-%d %H:%M").to_string();
        let q_by_code: HashMap<&str, &QuoteRow> =
            quotes.iter().map(|q| (q.code.as_str(), q)).collect();
        let daily = self.daily();
        for h in &holdings {
            // 有效性过滤与回写链同语义（04 附录 B）：仅 status=ok 且 price>0 参与判定
            let Some(q) = q_by_code.get(h.code.as_str()) else {
                continue; // 响应缺行：skipped 已记 note
            };
            if q.status != QuoteStatus::Ok {
                continue;
            }
            let Some(price) = q.price.filter(|p| *p > 0.0) else {
                continue;
            };
            let (prev, limit_up, limit_down) =
                prev_and_limits(h.kind, &h.code, h.limit_pct, q, h.prev);

            // 最近一条建议留档的 stop/target_hi（无留档 → 不参与）
            let (stop, target_hi) = match lock(&self.deps.store).list_advice(Some(&h.code), 1) {
                Ok(mut v) => v.pop().map(|a| (a.stop, a.target_hi)).unwrap_or((None, None)),
                Err(e) => {
                    rep.notes.push(format!("读取 {} 建议留档失败：{e}", h.code));
                    (None, None)
                }
            };

            let cb = daily.cb.get(&h.code);
            let item = AlertItem {
                kind: h.kind,
                price,
                prev,
                limit_up,
                limit_down,
                cost: Some(h.cost),
                stop,
                target_hi,
                volume_today: q.volume,
                vol_hist: daily
                    .vol_hist
                    .get(&h.code)
                    .cloned()
                    .flatten()
                    .unwrap_or_default(),
                redeem_status: cb.and_then(|c| c.redeem_status.clone()),
                last_trade_date: cb.and_then(|c| c.last_trade_date.clone()),
                maturity_date: cb.and_then(|c| c.maturity_date.clone()),
            };
            self.eval_and_insert(&ts, today, &h.code, &h.name, item, rep);
        }

        // 自选按「无持仓形态」判定（03 §12）：cost/stop/target_hi 一律 None →
        // 序 3/4/5 不参与；1/2/6/7/8 与持仓同款。去重共用 alerts 表同日 (code,type)
        if !w_quotes.is_empty() {
            let wq_by_code: HashMap<&str, &QuoteRow> =
                w_quotes.iter().map(|q| (q.code.as_str(), q)).collect();
            for w in &extra {
                let Some(q) = wq_by_code.get(w.code.as_str()) else {
                    continue; // 响应缺行：源未收录（sidecar 补齐链已尽力）
                };
                if q.status != QuoteStatus::Ok {
                    continue;
                }
                let Some(price) = q.price.filter(|p| *p > 0.0) else {
                    continue;
                };
                let (prev, limit_up, limit_down) =
                    prev_and_limits(w.kind, &w.code, w.limit_pct, q, None);
                let cb = daily.cb.get(&w.code);
                let item = AlertItem {
                    kind: w.kind,
                    price,
                    prev,
                    limit_up,
                    limit_down,
                    cost: None,
                    stop: None,
                    target_hi: None,
                    volume_today: q.volume,
                    vol_hist: daily
                        .vol_hist
                        .get(&w.code)
                        .cloned()
                        .flatten()
                        .unwrap_or_default(),
                    redeem_status: cb.and_then(|c| c.redeem_status.clone()),
                    last_trade_date: cb.and_then(|c| c.last_trade_date.clone()),
                    maturity_date: cb.and_then(|c| c.maturity_date.clone()),
                };
                self.eval_and_insert(&ts, today, &w.code, &w.name, item, rep);
            }
        }
    }

    /// 判定 → 同日 (code,type) 去重 → 落库（持仓与自选共用；数字口径只在
    /// `zimu-domain::alert`，这里只搬运——03 §12）
    fn eval_and_insert(
        &self,
        ts: &str,
        today: &str,
        code: &str,
        name: &str,
        item: AlertItem,
        rep: &mut TickReport,
    ) {
        let out = alert::eval_alerts(&alert::AlertEvalInput {
            today: today.to_string(),
            item,
        });
        for d in out.alerts {
            match lock(&self.deps.store).alert_exists_on(code, &d.alert_type, today) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => {
                    rep.notes
                        .push(format!("{code} 预警去重查询失败（跳过该条）：{e}"));
                    continue;
                }
            }
            let na = NewAlert {
                ts: ts.to_string(),
                code: code.to_string(),
                name: name.to_string(),
                alert_type: d.alert_type.clone(),
                level: d.level.clone(),
                text: d.text,
            };
            match lock(&self.deps.store).insert_alert(&na) {
                Ok(id) => {
                    tracing::info!(
                        code = %na.code, name = %na.name,
                        r#type = %na.alert_type, level = %na.level,
                        "预警触发（03 §12）"
                    );
                    rep.new_alerts.push(NewAlertInfo {
                        id,
                        code: na.code,
                        name: na.name,
                        alert_type: na.alert_type,
                        level: na.level,
                        text: na.text,
                    });
                }
                Err(e) => rep.notes.push(format!("{code} 预警落库失败：{e}")),
            }
        }
    }

    /// 日级事实缓存：cb_list（持仓**或自选**有转债才打，每日一次）+
    /// 日线量窗口（每日每 code 一次，持仓与自选共用同一缓存）
    async fn ensure_daily(
        &self,
        today: &str,
        holdings: &[HoldingRow],
        watch: &[WatchlistRow],
        rep: &mut TickReport,
    ) {
        // a) 转债强赎/期限字段（cb_list 是全量在市列表的重请求——不进分钟级轮询）
        let cb_codes: Vec<String> = holdings
            .iter()
            .filter(|h| h.kind == Kind::Cb)
            .map(|h| h.code.clone())
            .chain(
                watch
                    .iter()
                    .filter(|w| w.kind == Kind::Cb)
                    .map(|w| w.code.clone()),
            )
            .collect();
        let need_cb = !cb_codes.is_empty() && !self.daily().cb_checked;
        if need_cb {
            match self.deps.market.cb_list().await {
                Ok(p) => {
                    if p.redeem_source.is_none() {
                        rep.notes.push(
                            "cb_list 强赎源降级：今日强赎/到期类判定跳过（价格类照常）".into(),
                        );
                    }
                    let mut d = self.daily();
                    for code in &cb_codes {
                        match p.items.iter().find(|i| i.code == *code) {
                            Some(it) => {
                                d.cb.insert(
                                    code.clone(),
                                    CbFacts {
                                        redeem_status: it.redeem_status.clone(),
                                        last_trade_date: it.last_trade_date.clone(),
                                        maturity_date: it.maturity_date.clone(),
                                    },
                                );
                            }
                            None => rep.notes.push(format!(
                                "{code} 未在 cb_list 在市列表中（强赎/到期类今日不参与）"
                            )),
                        }
                    }
                    d.cb_checked = true;
                }
                // 不置 cb_checked：下一轮重试（首轮多为 sidecar 冷启动）
                Err(e) => rep.notes.push(format!("cb_list 拉取失败（下一轮重试）：{e}")),
            }
        }

        // b) 日线量窗口：近 20 根（不含今日）。当日失败记 None → 放量类今日
        //    不参与判定（不伪造；也不逐分钟重试白耗请求——日线级故障等次日自愈）
        let missing: Vec<(String, String)> = {
            let d = self.daily();
            let h_iter = holdings
                .iter()
                .filter(|h| !d.vol_hist.contains_key(&h.code))
                .filter_map(|h| symbol_of(&h.mkt, &h.code).map(|s| (h.code.clone(), s)));
            let w_iter = watch
                .iter()
                .filter(|w| !d.vol_hist.contains_key(&w.code))
                .filter_map(|w| symbol_of(&w.mkt, &w.code).map(|s| (w.code.clone(), s)));
            h_iter.chain(w_iter).collect()
        };
        if missing.is_empty() {
            return;
        }
        let start = NaiveDate::parse_from_str(today, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.checked_sub_days(Days::new(KLINE_LOOKBACK_DAYS)))
            .map(|d| d.format("%Y-%m-%d").to_string());
        let mut fetched: Vec<(String, Option<Vec<Option<f64>>>)> = Vec::new();
        for (code, sym) in &missing {
            match self
                .deps
                .market
                .kline(sym, None, start.as_deref(), Some(today))
                .await
            {
                Ok(p) => {
                    // 排除今日 bar（sidecar 盘中可能带当日未完成 bar——量不是全日量）
                    let mut vols: Vec<Option<f64>> = p
                        .kline
                        .iter()
                        .filter(|b| b.date != *today)
                        .map(|b| b.volume)
                        .collect();
                    if vols.len() > VOL_WINDOW {
                        vols.drain(..vols.len() - VOL_WINDOW);
                    }
                    fetched.push((code.clone(), Some(vols)));
                }
                Err(e) => {
                    rep.notes
                        .push(format!("{code} 日线获取失败，放量类今日不参与判定：{e}"));
                    fetched.push((code.clone(), None));
                }
            }
        }
        let mut d = self.daily();
        for (code, v) in fetched {
            d.vol_hist.insert(code, v);
        }
    }

    /// 盘后链（03 §12）：最终刷新 → equity_snapshots（date 主键幂等）→ 复盘触发。
    /// 幂等闸 = 当日快照行存在；当日缺失且已过 15:05 → 任意一轮补跑（含重启后）。
    async fn after_close(&self, today: &str, now: DateTime<FixedOffset>, rep: &mut TickReport) {
        match lock(&self.deps.store).equity_snapshot(today) {
            Ok(Some(_)) => return, // 今日链已完成
            Ok(None) => {}
            Err(e) => {
                rep.notes.push(format!("读取净值快照失败：{e}"));
                return;
            }
        }
        // 最终刷新（15:00 后行情冻结为收盘价）；失败 → 整链顺延下一轮，宁缺不写旧数
        let view = match service::refresh_portfolio(&self.deps.store, &*self.deps.market, &self.deps.sector_map).await {
            Ok(r) => r.portfolio,
            Err(RefreshError::Market(e)) => {
                rep.notes
                    .push(format!("盘后最终刷新失败，快照顺延下一轮：{e}"));
                return;
            }
            Err(RefreshError::Store(m)) => {
                rep.notes.push(format!("盘后刷新链存储错误：{m}"));
                return;
            }
        };
        let ts = now.format("%Y-%m-%d %H:%M").to_string();
        let snap = EquitySnapshotRow::from_agg(today, &ts, &view.agg);
        if let Err(e) = lock(&self.deps.store).upsert_equity_snapshot(&snap) {
            rep.notes.push(format!("净值快照落库失败：{e}"));
            return;
        }
        rep.snapshot_written = true;
        tracing::info!(
            date = %today, total_assets = snap.total_assets,
            day_pnl = snap.day_pnl, day_pnl_pct = snap.day_pnl_pct,
            "净值快照已写入（15:05 盘后链，数字 = recalc agg 原值）"
        );

        // 触发盘后复盘（/复盘 → post-market-review Skill）；agent 离线如实记录，
        // 快照照写——ok=1 只表示会话受理，产出以 advice_history 为准（03 §12 留档）
        let (ok, detail) = match &self.review {
            Some(trigger) => {
                let accepted = trigger(REVIEW_TRIGGER_TEXT.to_string()).await;
                (
                    accepted,
                    if accepted {
                        "复盘已注入 agent 会话（/复盘 Skill）".to_string()
                    } else {
                        "会话注入失败（agent 循环已退出或通道关闭）".to_string()
                    },
                )
            }
            None => (
                false,
                "agent 未接线（缺 robit 配置或启动失败），复盘未触发".to_string(),
            ),
        };
        rep.review_ok = Some(ok);
        if let Err(e) = lock(&self.deps.store).insert_push_log(
            &ts,
            "agent",
            "post_market_review",
            ok,
            &detail,
        ) {
            rep.notes.push(format!("push_log 落库失败：{e}"));
        }
        tracing::info!(ok, detail = %detail, "盘后复盘触发（15:05 调度）");

        // 盘后摘要推送（T6）：把刚写入的快照行原值交给宿主（数字不现编，03 §12；
        // 格式化与 push_log 留痕归宿主侧）。复盘正文仍留在 Web 会话——这里只推
        // 数字摘要，不向 IM 会话注入触发回合（robit 缺注入 API，见 R8）。
        if let Some(pusher) = &self.push_summary {
            rep.summary_pushed = Some(pusher(snap.clone()).await);
        }
    }
}

/// prev 与涨跌停限价（§2）：prev 行情行优先（缺失/≤0 回退 `fallback_prev`——
/// 持仓为库存旧值，自选无库存传 None）；pct 数据层配置优先、未配置按板块推断
/// （推不出 → 涨跌停类不参与判定，不伪造）。持仓与自选两条路径共用。
fn prev_and_limits(
    kind: Kind,
    code: &str,
    configured: Option<i64>,
    q: &QuoteRow,
    fallback_prev: Option<f64>,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let prev = q.prev.filter(|p| *p > 0.0).or(fallback_prev);
    let pct = configured.or_else(|| limit_pct(kind, code, false));
    let (mut limit_up, mut limit_down) = (None, None);
    if let (Some(p), Some(pc)) = (prev, pct) {
        if p > 0.0 {
            let r = limit_prices(kind, p, pc);
            limit_up = r.limit_up;
            limit_down = r.limit_down;
        }
    }
    (prev, limit_up, limit_down)
}

/// 实时循环（生产入口）：tick + sleep 的薄壳——判定逻辑全在 [`SchedulerCore::tick`]，
/// 测试直接注入模拟时刻，不经过这里。
pub async fn run(core: Arc<SchedulerCore>, interval: Duration) {
    tracing::info!(
        "预警调度器已启动：盘中（live 时段）每 {}s 轮询 + 交易日 15:05 盘后链（03 §12）",
        interval.as_secs()
    );
    loop {
        let rep = core.tick(now_shanghai()).await;
        log_tick(&rep);
        tokio::time::sleep(interval).await;
    }
}

fn log_tick(rep: &TickReport) {
    let interesting = !rep.new_alerts.is_empty()
        || rep.snapshot_written
        || rep.review_ok.is_some()
        || !rep.notes.is_empty();
    if interesting {
        tracing::info!(
            state = %rep.state,
            polled = rep.polled,
            updated = rep.quotes_updated,
            alerts = rep.new_alerts.len(),
            snapshot = rep.snapshot_written,
            review = ?rep.review_ok,
            notes = %rep.notes.join("；"),
            "调度轮"
        );
    } else {
        // 无事件空转轮降 TRACE（2026-11-21 拍板）：非交易日每 60s 一条 DEBUG 刷屏，
        // 排查节奏问题时 log_level=trace 再看
        tracing::trace!(
            state = %rep.state,
            polled = rep.polled,
            updated = rep.quotes_updated,
            "调度轮（无事件）"
        );
    }
}
