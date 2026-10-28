//! 市场时钟（03 §9 生产口径，P4-T4 定稿）——纯函数，无 I/O、无系统时钟读取。
//!
//! 口径要点：
//! - **时区恒 Asia/Shanghai**：UTC+8 固定偏移（1991 年后无夏令时），入参无论带
//!   什么时区都先转 +8 再判断——部署机时区设错口径也不漂。
//! - **交易日历是唯一依据**：`CalendarView.trade_dates` 有值时，「今天是否交易日 /
//!   下一交易日」全按日历（节假日、调休上班的周六都对）；None 时退工作日启发式
//!   并置 `calendar_degraded = true`（界面如实标注「未排除节假日」）。
//! - 时段边界（09:15/09:30/11:30/13:00/15:00/15:30）与状态文案同原型
//!   `data.js:marketClock()`——前端展示文案的唯一来源，永不降级。
//! - API 只给事实（`next_event`/`next_at`/`next_trade_day`），倒计时文案由前端
//!   每秒走针（与原型 `cd()` 同构）。

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Timelike, Weekday};
use serde::Serialize;

/// Asia/Shanghai = UTC+8 固定偏移（03 §9：不引时区数据库，产品范围仅沪深 A 股）
pub const SHANGHAI_OFFSET_SECS: i32 = 8 * 3600;

pub fn shanghai_offset() -> FixedOffset {
    FixedOffset::east_opt(SHANGHAI_OFFSET_SECS).expect("UTC+8 是合法固定偏移")
}

/// 当前上海时间（唯一的时钟读取点；`market_clock` 本身是纯函数）
pub fn now_shanghai() -> DateTime<FixedOffset> {
    chrono::Utc::now().with_timezone(&shanghai_offset())
}

/// 日历视图（时钟的输入侧）：`trade_dates` 为升序 ISO 日期串。
/// None = 日历不可用（sidecar 挂/全链 502）→ 工作日启发式降级。
#[derive(Debug, Clone, Default)]
pub struct CalendarView {
    pub trade_dates: Option<Vec<String>>,
    pub source: Option<String>,
    /// sidecar 自身的降级标注（备源日历顶上）——透传展示，不影响判定
    pub upstream_degraded: bool,
}

impl CalendarView {
    pub fn unavailable() -> Self {
        Self::default()
    }

    pub fn from_payload(p: &crate::models::CalendarPayload) -> Self {
        Self {
            trade_dates: Some(p.trade_dates.clone()),
            source: p.source.clone(),
            upstream_degraded: p.degraded,
        }
    }

    /// 今天是否交易日；第二返回值 = 是否在用启发式（降级标注）。
    pub fn is_trade_day(&self, d: NaiveDate) -> (bool, bool) {
        match &self.trade_dates {
            Some(dates) => (dates.iter().any(|s| s == &iso(d)), false),
            None => (is_weekday(d), true),
        }
    }

    /// 严格晚于 `d` 的下一交易日；日历缺失/窗口不够时为 None（启发式猜测由
    /// `market_clock` 负责，因为它要进 `next_trade_day` 字段并带降级标注）。
    pub fn next_trade_day(&self, d: NaiveDate) -> Option<String> {
        let dates = self.trade_dates.as_ref()?;
        let today = iso(d);
        dates.iter().find(|s| s.as_str() > today.as_str()).cloned()
    }
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn is_weekday(d: NaiveDate) -> bool {
    !matches!(d.weekday(), Weekday::Sat | Weekday::Sun)
}

/// 日历不可用时的启发式：跳过周末的下一个工作日（可能撞上节假日——所以必须
/// 带 `calendar_degraded` 标注，03 §9 降级条款）。
fn guess_next_weekday(d: NaiveDate) -> NaiveDate {
    let mut n = d + Duration::days(1);
    while !is_weekday(n) {
        n = n + Duration::days(1);
    }
    n
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketClock {
    /// 机器可读状态：pre_open / auction / am / lunch / pm / after_hours /
    /// closed / non_trading_day
    pub state: &'static str,
    /// 中文状态（与原型 marketClock 同款文案，前端展示唯一来源）
    pub state_text: &'static str,
    /// 原型圆点语义：live / lunch / closed
    pub dot: &'static str,
    /// 是否处于「行情会动」的时段（原型 tickPrices 的闸门语义）
    pub live: bool,
    /// 上海当前时刻
    pub date: String,
    pub time: String,
    /// 下一事件（机器可读）：auction / open / lunch / pm_open / close
    pub next_event: &'static str,
    /// 下一事件的上海时刻（ISO8601 +08:00）；跨日事件 = 下一交易日 09:15
    pub next_at: Option<String>,
    /// 下一交易日（YYYY-MM-DD）：休市/盘后定价/已收盘必给；日历缺失时给
    /// 启发式猜测值（配 calendar_degraded 判读）
    pub next_trade_day: Option<String>,
    /// true = 日历不可用，交易日判定退化为工作日启发式（03 §9 降级条款）
    pub calendar_degraded: bool,
    /// 日历源名（sina_calendar / baostock_calendar）；不可用时 null
    pub calendar_source: Option<String>,
    /// sidecar 日历自身降级（备源顶上）——如实透传
    pub calendar_upstream_degraded: bool,
}

fn at(d: NaiveDate, h: u32, m: u32) -> DateTime<FixedOffset> {
    d.and_hms_opt(h, m, 0)
        .expect("边界时刻合法")
        .and_local_timezone(shanghai_offset())
        .single()
        .expect("固定偏移无歧义时刻")
}

/// 市场时钟（03 §9）。`now` 任意时区，内部统一转上海。
pub fn market_clock<Tz: chrono::TimeZone>(
    now: DateTime<Tz>,
    cal: &CalendarView,
) -> MarketClock {
    let now = now.with_timezone(&shanghai_offset());
    let today = now.date_naive();
    let mins = now.hour() * 60 + now.minute();
    let (is_trade, heuristic) = cal.is_trade_day(today);

    // 下一交易日：日历优先；缺失/窗口不够 → 工作日启发式猜测。
    // 降级标注只跟「日历可用性」走（heuristic == trade_dates.is_none()）：
    // 有日历但窗口不够是运维问题（默认窗口 ±180 天，实际到不了边界），不谎报降级。
    let next_day = cal
        .next_trade_day(today)
        .unwrap_or_else(|| iso(guess_next_weekday(today)));
    let calendar_degraded = heuristic;

    let base = MarketClock {
        state: "",
        state_text: "",
        dot: "closed",
        live: false,
        date: iso(today),
        time: now.format("%H:%M:%S").to_string(),
        next_event: "auction",
        next_at: None,
        next_trade_day: Some(next_day.clone()),
        calendar_degraded,
        calendar_source: cal.source.clone(),
        calendar_upstream_degraded: cal.upstream_degraded,
    };
    let next_auction = NaiveDate::parse_from_str(&next_day, "%Y-%m-%d")
        .ok()
        .map(|d| at(d, 9, 15).to_rfc3339());

    if !is_trade {
        return MarketClock {
            state: "non_trading_day",
            state_text: "休市",
            next_at: next_auction,
            ..base
        };
    }
    let boundary = |h, m| at(today, h, m).to_rfc3339();
    if mins < 9 * 60 + 15 {
        return MarketClock {
            state: "pre_open",
            state_text: "盘前",
            next_at: Some(boundary(9, 15)),
            ..base
        };
    }
    if mins < 9 * 60 + 30 {
        return MarketClock {
            state: "auction",
            state_text: "集合竞价",
            dot: "live",
            live: true,
            next_event: "open",
            next_at: Some(boundary(9, 30)),
            ..base
        };
    }
    if mins < 11 * 60 + 30 {
        return MarketClock {
            state: "am",
            state_text: "交易中 · 早盘",
            dot: "live",
            live: true,
            next_event: "lunch",
            next_at: Some(boundary(11, 30)),
            ..base
        };
    }
    if mins < 13 * 60 {
        return MarketClock {
            state: "lunch",
            state_text: "午间休市",
            dot: "lunch",
            next_event: "pm_open",
            next_at: Some(boundary(13, 0)),
            ..base
        };
    }
    if mins < 15 * 60 {
        return MarketClock {
            state: "pm",
            state_text: "交易中 · 午盘",
            dot: "live",
            live: true,
            next_event: "close",
            next_at: Some(boundary(15, 0)),
            ..base
        };
    }
    if mins < 15 * 60 + 30 {
        return MarketClock {
            state: "after_hours",
            state_text: "盘后定价交易",
            dot: "lunch",
            next_at: next_auction,
            ..base
        };
    }
    MarketClock {
        state: "closed",
        state_text: "已收盘",
        next_at: next_auction,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<FixedOffset> {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
            .and_local_timezone(shanghai_offset())
            .single()
            .unwrap()
    }

    /// 2026-10-28 是周三；日历窗口含 10-28/29/30（10-31 周六、11-01 周日）
    fn cal() -> CalendarView {
        CalendarView {
            trade_dates: Some(vec![
                "2026-10-28".into(),
                "2026-10-29".into(),
                "2026-10-30".into(),
            ]),
            source: Some("sina_calendar".into()),
            upstream_degraded: false,
        }
    }

    #[test]
    fn session_boundaries_on_trade_day() {
        let c = cal();
        // (时刻, 期望 state, dot, live, next_event)
        for (h, m, state, dot, live, next_event) in [
            (8, 0, "pre_open", "closed", false, "auction"),
            (9, 14, "pre_open", "closed", false, "auction"),
            (9, 15, "auction", "live", true, "open"),
            (9, 29, "auction", "live", true, "open"),
            (9, 30, "am", "live", true, "lunch"),
            (11, 29, "am", "live", true, "lunch"),
            (11, 30, "lunch", "lunch", false, "pm_open"),
            (12, 59, "lunch", "lunch", false, "pm_open"),
            (13, 0, "pm", "live", true, "close"),
            (14, 59, "pm", "live", true, "close"),
            (15, 0, "after_hours", "lunch", false, "auction"),
            (15, 29, "after_hours", "lunch", false, "auction"),
            (15, 30, "closed", "closed", false, "auction"),
            (23, 59, "closed", "closed", false, "auction"),
        ] {
            let k = market_clock(sh(2026, 10, 28, h, m), &c);
            assert_eq!(k.state, state, "{h:02}:{m:02}");
            assert_eq!(k.dot, dot, "{h:02}:{m:02}");
            assert_eq!(k.live, live, "{h:02}:{m:02}");
            assert_eq!(k.next_event, next_event, "{h:02}:{m:02}");
            assert!(!k.calendar_degraded);
        }
    }

    #[test]
    fn boundary_next_at_values() {
        let c = cal();
        let k = market_clock(sh(2026, 10, 28, 9, 20), &c);
        assert_eq!(k.next_at.as_deref(), Some("2026-10-28T09:30:00+08:00"));
        // 收盘后：下一交易日 09:15（不是「明天」——按日历）
        let k = market_clock(sh(2026, 10, 28, 20, 0), &c);
        assert_eq!(k.next_trade_day.as_deref(), Some("2026-10-29"));
        assert_eq!(k.next_at.as_deref(), Some("2026-10-29T09:15:00+08:00"));
    }

    #[test]
    fn holiday_from_calendar() {
        // 周三不在日历里（节假日）→ 休市，下一交易日按日历
        let c = CalendarView {
            trade_dates: Some(vec!["2026-10-29".into(), "2026-10-30".into()]),
            source: Some("sina_calendar".into()),
            upstream_degraded: false,
        };
        let k = market_clock(sh(2026, 10, 28, 10, 0), &c);
        assert_eq!(k.state, "non_trading_day");
        assert_eq!(k.state_text, "休市");
        assert!(!k.live && !k.calendar_degraded);
        assert_eq!(k.next_trade_day.as_deref(), Some("2026-10-29"));
    }

    #[test]
    fn makeup_saturday_is_trade_day() {
        // 调休上班的周六在日历里就是交易日（03 §9：日历取代工作日启发式）
        let c = CalendarView {
            trade_dates: Some(vec!["2026-10-31".into(), "2026-11-02".into()]),
            source: Some("sina_calendar".into()),
            upstream_degraded: false,
        };
        let k = market_clock(sh(2026, 10, 31, 10, 0), &c); // 周六
        assert_eq!(k.state, "am");
        assert!(k.live && !k.calendar_degraded);
        assert_eq!(k.next_trade_day.as_deref(), Some("2026-11-02"));
    }

    #[test]
    fn weekend_without_calendar() {
        let c = CalendarView::unavailable();
        let k = market_clock(sh(2026, 10, 31, 10, 0), &c); // 周六
        assert_eq!(k.state, "non_trading_day");
        assert!(k.calendar_degraded);
        assert_eq!(k.next_trade_day.as_deref(), Some("2026-11-02")); // 周一（启发式）
        assert!(k.calendar_source.is_none());
    }

    #[test]
    fn degraded_weekday_heuristic() {
        // 周三 10:00，无日历 → 照常走时段（边界永不降级），但标注降级
        let c = CalendarView::unavailable();
        let k = market_clock(sh(2026, 10, 28, 10, 0), &c);
        assert_eq!(k.state, "am");
        assert!(k.live);
        assert!(k.calendar_degraded);
        assert_eq!(k.next_event, "lunch");
        assert_eq!(k.next_at.as_deref(), Some("2026-10-28T11:30:00+08:00"));
        // 交易日盘中的「下一交易日」= 启发式猜的周四（降级标注判读）
        assert_eq!(k.next_trade_day.as_deref(), Some("2026-10-29"));
    }

    #[test]
    fn timezone_input_normalized() {
        // UTC 01:30 = 上海 09:30：任意时区入参内部统一转 +8（部署机时区设错口径不漂）
        let c = cal();
        let utc: chrono::DateTime<chrono::Utc> = chrono::DateTime::from_naive_utc_and_offset(
            NaiveDate::from_ymd_opt(2026, 10, 28)
                .unwrap()
                .and_hms_opt(1, 30, 0)
                .unwrap(),
            chrono::Utc,
        );
        let k = market_clock(utc, &c);
        assert_eq!(k.state, "am");
        assert_eq!(k.date, "2026-10-28");
        assert_eq!(k.time, "09:30:00");
        // 同一物理时刻，+8 入参结果一致
        let k2 = market_clock(utc.with_timezone(&shanghai_offset()), &c);
        assert_eq!(k2.state, k.state);
        assert_eq!(k2.time, k.time);
    }
}
