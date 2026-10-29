//! 预警判定（03 §12 生产口径，P4-T7 定稿 2026-10-29）——纯函数，无 I/O、
//! 无时钟、无状态。
//!
//! 边界：
//! - 本模块只做「事实 → 触发哪些预警」的判定；**调度**（何时轮询、合并行情
//!   请求、同日 (code,type) 去重、落库、15:05 复盘链）在 `zimu-agent::scheduler`
//!   应用层，那里不定义任何数字口径。
//! - 输入全部是调用方 assembled 的事实：限价按 §2 由 `price_limit` 拼装传入、
//!   stop/target_hi 取最近一条 `advice_history` 留档、vol_hist 是近 20 根日线
//!   （不含今日）的量序列。数据缺失 → 对应判定序**不参与**，不伪造。
//! - `text` 是展示层模板（与 §10 `dims[].t` 同约定）：数字只取输入值，
//!   价格按 §1 小数位、百分比带符号两位小数；不进 fixture，两边单测只断言
//!   关键事实存在。
//! - 输出数组序 = §12 判定序（1 涨停 → 8 临近到期），同标的多条全部产出。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::lot::price_decimals;
use crate::Kind;
use crate::THRESHOLD_EPS;

/// 放量倍数：当日累计量 ≥ 2.0 × 近 20 日均量（03 §12 常量表）
pub const VOL_SURGE_MULT: f64 = 2.0;
/// 均量最少有效样本（null / ≤0 不计入）；不足 → 放量不参与判定
pub const VOL_SURGE_MIN_SAMPLES: usize = 5;
/// 转债期限窗口（自然日）：0 ≤ last_trade_date − today ≤ 5
pub const CB_DEADLINE_DAYS: i64 = 5;

// type 封闭词汇（03 §12 判定表；落 alerts.type 列，前端直接展示）
pub const T_LIMIT_UP: &str = "触及涨停";
pub const T_LIMIT_DOWN: &str = "触及跌停";
pub const T_BELOW_COST: &str = "跌破成本";
pub const T_STOP_LOSS: &str = "触及止损线";
pub const T_TAKE_PROFIT: &str = "触及止盈线";
pub const T_VOL_SURGE: &str = "放量";
pub const T_CB_REDEEM: &str = "强赎公告";
pub const T_CB_DEADLINE: &str = "临近到期";

// level 词汇（§11：up/down 只用于价格方向；warn 要动作类；引擎不产出 flat）
pub const L_UP: &str = "up";
pub const L_DOWN: &str = "down";
pub const L_WARN: &str = "warn";

/// 单标的判定输入（字段与 fixture `alerts.json` 键一一对应）
#[derive(Debug, Clone, Deserialize)]
pub struct AlertItem {
    pub kind: Kind,
    /// 现价（本轮实时行情）；≤0 → 整行跳过不伪造
    pub price: f64,
    /// 昨收（放量方向判定用）；缺失 → 放量 level 降为 warn
    pub prev: Option<f64>,
    /// 当日涨停价（调用方按 §2 拼装；None → 序 1 不参与）
    pub limit_up: Option<f64>,
    /// 当日跌停价（同上；None → 序 2 不参与）
    pub limit_down: Option<f64>,
    /// 持仓摊薄成本（§4）；None / ≤0 → 序 3 不参与
    pub cost: Option<f64>,
    /// 最近一条建议留档的止损价；None → 序 4 不参与
    pub stop: Option<f64>,
    /// 最近一条建议留档的目标区间上沿；None → 序 5 不参与
    pub target_hi: Option<f64>,
    /// 当日累计量（股/份/张，与日线 volume 同单位）
    pub volume_today: Option<f64>,
    /// 近 20 根日线量（**不含今日**，升序，调用方截窗；null 不计入样本）
    #[serde(default)]
    pub vol_hist: Vec<Option<f64>>,
    /// 转债强赎状态（集思录；源降级 → None，序 7 不参与）
    pub redeem_status: Option<String>,
    /// 转债最后交易日 YYYY-MM-DD（序 8 首选）
    pub last_trade_date: Option<String>,
    /// 转债到期日 YYYY-MM-DD（last_trade_date 缺失时的回退）
    pub maturity_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlertEvalInput {
    /// 上海日期 YYYY-MM-DD（转债期限的自然日差基准）
    pub today: String,
    pub item: AlertItem,
}

/// 触发的预警（type/level 进 fixture；text 展示层模板不校验）
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AlertDraft {
    #[serde(rename = "type")]
    pub alert_type: String,
    pub level: String,
    pub text: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AlertEvalOutput {
    /// 按 §12 判定序排列；空数组 = 本轮无预警
    pub alerts: Vec<AlertDraft>,
}

/// 预警判定（03 §12）。纯函数：同一输入永远同一输出，去重/落库归调度器。
pub fn eval_alerts(inp: &AlertEvalInput) -> AlertEvalOutput {
    let it = &inp.item;
    let mut alerts: Vec<AlertDraft> = Vec::new();
    // 无有效价整行跳过（调用方本应以行情有效性过滤，这里再守一道——不伪造）
    if !(it.price > 0.0) {
        return AlertEvalOutput { alerts };
    }
    let d = price_decimals(it.kind);
    let p = fmt_price(it.price, d);
    let mut push = |t: &str, level: &str, text: String| {
        alerts.push(AlertDraft {
            alert_type: t.to_string(),
            level: level.to_string(),
            text,
        })
    };

    // 序 1/2：涨跌停（含边界，§2 限价由调用方拼装）
    if let Some(lu) = it.limit_up {
        if it.price >= lu - THRESHOLD_EPS {
            push(
                T_LIMIT_UP,
                L_UP,
                format!("现价 {p} 已触及涨停价 {}", fmt_price(lu, d)),
            );
        }
    }
    if let Some(ld) = it.limit_down {
        if it.price <= ld + THRESHOLD_EPS {
            push(
                T_LIMIT_DOWN,
                L_DOWN,
                format!("现价 {p} 已触及跌停价 {}", fmt_price(ld, d)),
            );
        }
    }

    // 序 3：跌破成本（相等不算——压线算安全，附录规则 4）
    if let Some(c) = it.cost {
        if c > 0.0 && it.price < c - THRESHOLD_EPS {
            let pnl_pct = (it.price - c) / c * 100.0;
            push(
                T_BELOW_COST,
                L_DOWN,
                format!(
                    "现价 {p} 低于成本 {}，浮亏 {pnl_pct:+.2}%",
                    fmt_price(c, d)
                ),
            );
        }
    }

    // 序 4/5：最近留档的止损/止盈线（含边界；无留档 → 不参与）
    if let Some(s) = it.stop {
        if it.price <= s + THRESHOLD_EPS {
            push(
                T_STOP_LOSS,
                L_DOWN,
                format!("现价 {p} 已触及最近建议的止损线 {}", fmt_price(s, d)),
            );
        }
    }
    if let Some(t) = it.target_hi {
        if it.price >= t - THRESHOLD_EPS {
            push(
                T_TAKE_PROFIT,
                L_WARN,
                format!(
                    "现价 {p} 达到最近建议的止盈线 {}",
                    fmt_price(t, d)
                ),
            );
        }
    }

    // 序 6：放量（当日累计量 ≥ 2.0 × 近 20 日均量；有效样本 <5 不参与）
    let valid: Vec<f64> = it
        .vol_hist
        .iter()
        .flatten()
        .filter(|x| **x > 0.0)
        .copied()
        .collect();
    if valid.len() >= VOL_SURGE_MIN_SAMPLES {
        let avg = valid.iter().sum::<f64>() / valid.len() as f64;
        if let Some(vt) = it.volume_today {
            if vt > 0.0 && avg > 0.0 {
                let r = vt / avg;
                if r >= VOL_SURGE_MULT - THRESHOLD_EPS {
                    // 方向 = (price−prev)/prev；prev 缺失/≤0 → 方向不可判，warn
                    let (level, dir) = match it.prev {
                        Some(prev) if prev > 0.0 => {
                            let chg = (it.price - prev) / prev * 100.0;
                            let lv = if chg >= -THRESHOLD_EPS { L_UP } else { L_DOWN };
                            (lv, format!("，涨跌幅 {chg:+.2}%"))
                        }
                        _ => (L_WARN, "，昨收缺失、方向不可判".to_string()),
                    };
                    push(
                        T_VOL_SURGE,
                        level,
                        format!(
                            "当日累计量 {vt:.0} 为近 20 日均量 {avg:.0} 的 {r:.1} 倍{dir}"
                        ),
                    );
                }
            }
        }
    }

    // 序 7/8：转债强赎与期限（kind 门槛 + 集思录字段；源降级 → 字段 None 不参与）
    if it.kind == Kind::Cb {
        if let Some(rs) = it.redeem_status.as_deref().filter(|s| !s.is_empty()) {
            // 封闭判据：含「强赎」且不以「不」开头（「不强赎」不触发）
            if rs.contains("强赎") && !rs.starts_with('不') {
                let tail = match it.last_trade_date.as_deref().filter(|s| !s.is_empty()) {
                    Some(ltd) => format!("，最后交易日 {ltd}"),
                    None => String::new(),
                };
                push(
                    T_CB_REDEEM,
                    L_WARN,
                    format!("正股触发有条件赎回（{rs}）{tail}"),
                );
            }
        }
        let dated = it
            .last_trade_date
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| ("最后交易日", s))
            .or_else(|| {
                it.maturity_date
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|s| ("到期日", s))
            });
        if let Some((label, ds)) = dated {
            // 已过期（差 <0）不参与：数据异常，行情侧自会以停牌/退市跳过
            if let Some(days) = iso_date_diff(&inp.today, ds) {
                if (0..=CB_DEADLINE_DAYS).contains(&days) {
                    push(
                        T_CB_DEADLINE,
                        L_WARN,
                        format!("距{label} {ds} 还有 {days} 天"),
                    );
                }
            }
        }
    }

    AlertEvalOutput { alerts }
}

// ============================================================================
// 展示层模板辅助（不进 fixture）
// ============================================================================

fn fmt_price(v: f64, decimals: u32) -> String {
    format!("{v:.prec$}", prec = decimals as usize)
}

// ============================================================================
// 自然日差（不引 chrono：领域库保持零依赖，算法 = Howard Hinnant
// days_from_civil；TS 镜像用 Date.UTC 同值）
// ============================================================================

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 解析 ISO 日期（YYYY-MM-DD）为 civil 天数；非法格式/越界 → None（不参与判定）
fn parse_iso_date(s: &str) -> Option<i64> {
    let mut p = s.split('-');
    let y = p.next()?.parse::<i64>().ok()?;
    let m = p.next()?.parse::<u32>().ok()?;
    let d = p.next()?.parse::<u32>().ok()?;
    if p.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// to − from 的自然日差；任一解析失败 → None
fn iso_date_diff(from: &str, to: &str) -> Option<i64> {
    Some(parse_iso_date(to)? - parse_iso_date(from)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> AlertItem {
        AlertItem {
            kind: Kind::Stock,
            price: 10.0,
            prev: Some(10.0),
            limit_up: None,
            limit_down: None,
            cost: None,
            stop: None,
            target_hi: None,
            volume_today: None,
            vol_hist: Vec::new(),
            redeem_status: None,
            last_trade_date: None,
            maturity_date: None,
        }
    }

    fn eval(item: AlertItem) -> Vec<(String, String)> {
        let out = eval_alerts(&AlertEvalInput {
            today: "2026-10-29".into(),
            item,
        });
        out.alerts
            .into_iter()
            .map(|a| (a.alert_type, a.level))
            .collect()
    }

    #[test]
    fn quiet_input_yields_nothing() {
        assert!(eval(base()).is_empty());
    }

    #[test]
    fn zero_price_skips_row_even_with_breaches() {
        let mut it = base();
        it.price = 0.0;
        it.cost = Some(10.0); // 若判定会触发跌破成本——但无价不伪造
        it.stop = Some(11.0);
        assert!(eval(it).is_empty());
    }

    #[test]
    fn multiple_alerts_keep_judgement_order() {
        // 涨停（序 1）+ 跌破成本不可能同现；用 跌停 + 跌破成本 + 止损 验证排序
        let mut it = base();
        it.price = 9.0;
        it.limit_down = Some(9.0);
        it.cost = Some(10.0);
        it.stop = Some(9.5);
        let v = eval(it);
        assert_eq!(
            v,
            vec![
                (T_LIMIT_DOWN.to_string(), L_DOWN.to_string()),
                (T_BELOW_COST.to_string(), L_DOWN.to_string()),
                (T_STOP_LOSS.to_string(), L_DOWN.to_string()),
            ]
        );
    }

    #[test]
    fn redeem_prefix_guard() {
        let mut it = base();
        it.kind = Kind::Cb;
        it.redeem_status = Some("不强赎".into());
        assert!(eval(it.clone()).is_empty());
        it.redeem_status = Some("已公告强赎".into());
        assert_eq!(eval(it), vec![(T_CB_REDEEM.to_string(), L_WARN.to_string())]);
    }

    #[test]
    fn non_cb_ignores_redeem_fields() {
        // kind 门槛：股票的 redeem 字段（数据错配）不触发转债类预警
        let mut it = base();
        it.redeem_status = Some("已公告强赎".into());
        it.last_trade_date = Some("2026-10-30".into());
        assert!(eval(it).is_empty());
    }

    #[test]
    fn deadline_window_edges() {
        let mut it = base();
        it.kind = Kind::Cb;
        for (date, fires) in [
            ("2026-10-28", false), // −1：已过期不参与
            ("2026-10-29", true),  // 0：当天含边界
            ("2026-11-03", true),  // +5：压线含边界
            ("2026-11-04", false), // +6：窗口外
        ] {
            it.last_trade_date = Some(date.into());
            let v = eval(it.clone());
            assert_eq!(
                v.iter().any(|a| a.0 == T_CB_DEADLINE),
                fires,
                "last_trade_date={date}"
            );
        }
    }

    #[test]
    fn vol_surge_needs_five_valid_samples() {
        let mut it = base();
        it.volume_today = Some(99999.0);
        it.vol_hist = vec![Some(1000.0); 4];
        assert!(eval(it.clone()).is_empty(), "4 个有效样本不参与");
        it.vol_hist.push(None); // null 不计入样本
        assert!(eval(it.clone()).is_empty());
        it.vol_hist.push(Some(1000.0)); // 第 5 个有效样本 → 参与
        assert_eq!(eval(it), vec![(T_VOL_SURGE.to_string(), L_UP.to_string())]);
    }

    #[test]
    fn bad_dates_do_not_panic() {
        let mut it = base();
        it.kind = Kind::Cb;
        it.last_trade_date = Some("2026-13-45".into()); // 月份越界 → 不参与
        assert!(eval(it.clone()).is_empty());
        it.last_trade_date = Some("not-a-date".into());
        assert!(eval(it).is_empty());
    }

    #[test]
    fn text_carries_facts() {
        let mut it = base();
        it.price = 9.5;
        it.cost = Some(10.0);
        let out = eval_alerts(&AlertEvalInput {
            today: "2026-10-29".into(),
            item: it,
        });
        let t = &out.alerts[0].text;
        assert!(t.contains("9.50") && t.contains("10.00"), "{t}");
        assert!(t.contains("-5.00%"), "{t}"); // 浮亏 (9.5−10)/10 = −5.00%
    }

    #[test]
    fn civil_date_diff_known_pairs() {
        assert_eq!(iso_date_diff("2026-10-29", "2026-11-03"), Some(5));
        assert_eq!(iso_date_diff("2026-10-29", "2026-11-20"), Some(22));
        assert_eq!(iso_date_diff("2026-10-29", "2026-10-01"), Some(-28));
        assert_eq!(iso_date_diff("2024-02-28", "2024-03-01"), Some(2)); // 闰年
        assert_eq!(iso_date_diff("2026-12-31", "2027-01-01"), Some(1)); // 跨年
        assert_eq!(iso_date_diff("2026-10-29", "2026-10-29"), Some(0));
    }
}
