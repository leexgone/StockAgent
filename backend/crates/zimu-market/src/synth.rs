//! 盘中当日 bar 合成（docs/04 附录 B，P4-T4 定稿）。
//!
//! 背景：收盘后新浪/baostock 日线尾部是**上一交易日**（更新滞后数小时），
//! 盘中更拿不到当日定稿 bar。合成条件（全部满足）：
//! 1. 实时行 `symbol` 与日线同一标的；
//! 2. 实时行 `date` == 今天（上海）——实时行日期本身就是「今天在交易」的证据，
//!    日历不参与（节假日拿不到当日实时行）；
//! 3. `status == ok` 且 `price > 0`（no_data/停牌无价不合成）；
//! 4. 日线尾部日期 < 今天（为空也允许）。
//!
//! 合成 bar 的单位随实时行（股票/ETF 股/份；转债 张，与日线同源同单位）。
//! **盘中合成 bar 随实时行情变动，不是定稿 bar**；收盘后日线源更新到位，
//! 条件 4 自然失效。

use crate::models::{KlineBar, KlinePayload, QuoteRow, QuoteStatus};

/// 按 04 附录 B 规则合成当日 bar；不满足条件返回 None（绝不静默改数）。
pub fn today_bar(kline: &KlinePayload, quote: &QuoteRow, today: &str) -> Option<KlineBar> {
    if quote.symbol != kline.symbol {
        return None;
    }
    if quote.date.as_deref() != Some(today) {
        return None;
    }
    if quote.status != QuoteStatus::Ok {
        return None;
    }
    // 否定式是 NaN 守卫（同 zimu-domain 惯例）：`p > 0.0` 对 NaN 为 false
    let price = quote.price.filter(|p| *p > 0.0)?;
    let tail_before_today = kline
        .kline
        .last()
        .map(|b| b.date.as_str() < today)
        .unwrap_or(true);
    if !tail_before_today {
        return None; // 日线已含今天（源侧已更新）——不重复合成
    }
    Some(KlineBar {
        date: today.to_string(),
        open: quote.open.or(Some(price)),
        high: quote.high.or(Some(price)),
        low: quote.low.or(Some(price)),
        close: Some(price),
        volume: quote.volume,
        amount: quote.amount,
    })
}

/// 合成并追加：成功则 push 进尾部、count 同步，返回是否合成
///（server 据此在响应顶层标 `today_synthesized`）。
pub fn merge_today(kline: &mut KlinePayload, quote: &QuoteRow, today: &str) -> bool {
    match today_bar(kline, quote, today) {
        Some(bar) => {
            kline.kline.push(bar);
            kline.count = kline.kline.len();
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(date: &str, close: f64) -> KlineBar {
        KlineBar {
            date: date.into(),
            open: Some(close),
            high: Some(close),
            low: Some(close),
            close: Some(close),
            volume: Some(1000.0),
            amount: None,
        }
    }

    fn payload(bars: Vec<KlineBar>) -> KlinePayload {
        let count = bars.len();
        KlinePayload {
            symbol: "sh600519".into(),
            kind: "stock".into(),
            kind_routed: "explicit".into(),
            adjusted: "qfq".into(),
            start: "2026-10-01".into(),
            end: "2026-10-28".into(),
            count,
            source: Some("sina_stock_daily".into()),
            degraded: false,
            attempts: vec![],
            kline: bars,
        }
    }

    fn quote(date: Option<&str>, price: Option<f64>, status: QuoteStatus) -> QuoteRow {
        QuoteRow {
            symbol: "sh600519".into(),
            code: "600519".into(),
            mkt: "SH".into(),
            kind: "stock".into(),
            name: Some("贵州茅台".into()),
            price,
            prev: Some(1421.0),
            open: Some(1425.0),
            high: Some(1440.0),
            low: Some(1420.0),
            volume: Some(31234.0),
            amount: Some(4.48e9),
            chg_pct: Some(1.126),
            date: date.map(String::from),
            time: Some("15:00:03".into()),
            status,
            source: Some("sina_hq".into()),
        }
    }

    #[test]
    fn synthesizes_when_tail_is_yesterday() {
        let mut p = payload(vec![bar("2026-10-26", 1400.0), bar("2026-10-27", 1421.0)]);
        let merged = merge_today(&mut p, &quote(Some("2026-10-28"), Some(1437.0), QuoteStatus::Ok), "2026-10-28");
        assert!(merged);
        assert_eq!(p.count, 3);
        let last = p.kline.last().unwrap();
        assert_eq!(last.date, "2026-10-28");
        assert_eq!(last.close, Some(1437.0)); // close = 实时价
        assert_eq!(last.open, Some(1425.0)); // open/high/low 用实时行同名字段
        assert_eq!(last.high, Some(1440.0));
        assert_eq!(last.volume, Some(31234.0));
    }

    #[test]
    fn no_merge_when_daily_already_has_today() {
        let mut p = payload(vec![bar("2026-10-28", 1437.0)]);
        assert!(!merge_today(&mut p, &quote(Some("2026-10-28"), Some(1437.0), QuoteStatus::Ok), "2026-10-28"));
        assert_eq!(p.count, 1);
    }

    #[test]
    fn no_merge_for_stale_or_dead_quote() {
        let p = || payload(vec![bar("2026-10-27", 1421.0)]);
        // 实时行日期不是今天（停牌/源缓存陈旧）
        assert!(today_bar(&p(), &quote(Some("2026-10-27"), Some(1421.0), QuoteStatus::Ok), "2026-10-28").is_none());
        assert!(today_bar(&p(), &quote(None, Some(1421.0), QuoteStatus::Ok), "2026-10-28").is_none());
        // no_data / 退市 / 停牌无价
        assert!(today_bar(&p(), &quote(Some("2026-10-28"), None, QuoteStatus::NoData), "2026-10-28").is_none());
        assert!(today_bar(&p(), &quote(Some("2026-10-28"), None, QuoteStatus::HaltedOrDelisted), "2026-10-28").is_none());
        assert!(today_bar(&p(), &quote(Some("2026-10-28"), Some(0.0), QuoteStatus::Ok), "2026-10-28").is_none());
        assert!(today_bar(&p(), &quote(Some("2026-10-28"), Some(f64::NAN), QuoteStatus::Ok), "2026-10-28").is_none());
    }

    #[test]
    fn no_merge_for_other_symbol() {
        let p = payload(vec![bar("2026-10-27", 1421.0)]);
        let mut q = quote(Some("2026-10-28"), Some(100.0), QuoteStatus::Ok);
        q.symbol = "sh510300".into();
        assert!(today_bar(&p, &q, "2026-10-28").is_none());
    }

    #[test]
    fn empty_kline_allows_synth_and_ohl_fallback() {
        let mut p = payload(vec![]);
        let mut q = quote(Some("2026-10-28"), Some(100.0), QuoteStatus::Ok);
        q.open = None;
        q.high = None;
        q.low = None; // 开盘字段缺失 → 回落到 price
        assert!(merge_today(&mut p, &q, "2026-10-28"));
        let b = &p.kline[0];
        assert_eq!((b.open, b.high, b.low, b.close), (Some(100.0), Some(100.0), Some(100.0), Some(100.0)));
    }
}
