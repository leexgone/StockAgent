//! 基准对齐（03 §7「基准对齐」段，P7-T10）：把基准指数日线按快照日 carry-forward
//! 对齐并归一到区间净值起点——`perf` 的 bench 输入形态（「已归一到同一起点」）的
//! 唯一生产实现。原前端单侧实现 `lib/bench.ts`（P5-T2）逐条上收，两侧跑同一份
//! 黄金向量 `fixtures/golden/bench.json`；TS 镜像 `frontend/src/domain/bench.ts`。
//!
//! 纯函数纪律：**不排序、不校验入参有序性**——前置条件 = dates/bars 均升序，
//! 调用方保证（store equity_snapshots 已 date 升序、sidecar kline 已升序）。
//! 日期比较是 `YYYY-MM-DD` 字符串字典序（= 时间序，ASCII 下 Rust 字节序与
//! TS UTF-16 序一致）。null 语义见 03 附录固定口径 6：整段不可对齐 ≠ 数值 0，
//! 调用方以空 bench 喂 perf 走 bench_* 全 0 降级形态。

use serde::{Deserialize, Serialize};

/// 基准日线 bar（只消费 date/close 两字段；close 无效值在对齐前过滤）
#[derive(Debug, Clone, Deserialize)]
pub struct BenchBar {
    /// 交易日 YYYY-MM-DD
    pub date: String,
    /// 收盘价；None / ≤0 / NaN = 无效 bar（对齐前过滤，不占指针位）
    pub close: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlignBenchInput {
    /// 快照日序列（升序，与 nav 同序）
    pub dates: Vec<String>,
    /// 基准日线（升序）
    pub bars: Vec<BenchBar>,
    /// 归一锚 = 区间首个净值点
    pub nav0: f64,
}

/// 黄金向量输出包装（expect 形如 `{"aligned": [...] | null}`）
#[derive(Debug, Default, Serialize)]
pub struct AlignBenchOutput {
    /// 归一后的基准序列（与 dates 等长）；任一 null 分支 → None（整段不画不伪装）
    pub aligned: Option<Vec<f64>>,
}

/// 基准对齐主函数（03 §7 基准对齐段，判定序 = 文档条目序）：
/// ① bars 空或 dates < 2 → None；② 无效收盘过滤后为空 → None；
/// ③ carry-forward（只向后不向前；首快照日无值 → None；同日多根取输入序靠后）；
/// ④ 归一 raw[i]/raw[0]×nav0（raw[0] ≤ 0 或 nav0 ≤ 0/NaN → None）。
pub fn align_bench(inp: &AlignBenchInput) -> Option<Vec<f64>> {
    // ① 退化分支
    if inp.bars.is_empty() || inp.dates.len() < 2 {
        return None;
    }
    // ② 无效收盘过滤（None / ≤0 / NaN——NaN 的 c > 0.0 为 false，自动出局）
    let closes: Vec<(&str, f64)> = inp
        .bars
        .iter()
        .filter_map(|b| b.close.filter(|c| *c > 0.0).map(|c| (b.date.as_str(), c)))
        .collect();
    if closes.is_empty() {
        return None;
    }
    // ③ 双指针 carry-forward：每个快照日取「当日或此前最近」有效收盘。
    //    首快照日就无值（区间起点早于日线窗口）→ None，整段不画不伪装。
    let mut raw: Vec<f64> = Vec::with_capacity(inp.dates.len());
    let mut last: Option<f64> = None;
    let mut j = 0usize;
    for d in &inp.dates {
        while j < closes.len() && closes[j].0 <= d.as_str() {
            last = Some(closes[j].1); // 同日多根：指针扫过全部 ≤d 的 bar，取输入序靠后者
            j += 1;
        }
        match last {
            Some(v) => raw.push(v),
            None => return None,
        }
    }
    // ④ 归一到区间净值起点（base/nav0 无效 → None；NaN 的 !(x>0) 判定同 TS）
    let base = raw[0];
    if !(base > 0.0) || !(inp.nav0 > 0.0) {
        return None;
    }
    Some(raw.iter().map(|v| v / base * inp.nav0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(date: &str, close: Option<f64>) -> BenchBar {
        BenchBar { date: date.into(), close }
    }

    /// 与黄金向量同款容差比对（归一除法带 1-ulp 级浮点噪声，不做精确相等）
    fn assert_close(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "长度不符：{actual:?} vs {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() <= 1e-9, "期望 {e} 实际 {a}");
        }
    }

    #[test]
    fn carry_forward_and_normalize() {
        let out = align_bench(&AlignBenchInput {
            dates: vec!["2026-01-09".into(), "2026-01-12".into(), "2026-01-13".into()],
            bars: vec![
                bar("2026-01-08", Some(3980.0)), // 早于首快照日：只喂指针不当 base
                bar("2026-01-09", Some(4000.0)),
                bar("2026-01-12", Some(4060.0)), // 01-13 无 bar → carry
            ],
            nav0: 100_000.0,
        })
        .unwrap();
        assert_close(&out, &[100_000.0, 101_500.0, 101_500.0]);
    }

    #[test]
    fn start_before_window_is_null_not_backfill() {
        // 首快照日早于日线窗口 → None（不以首日之后的值回填伪装）
        let out = align_bench(&AlignBenchInput {
            dates: vec!["2025-12-31".into(), "2026-01-06".into()],
            bars: vec![bar("2026-01-05", Some(4000.0)), bar("2026-01-06", Some(4020.0))],
            nav0: 100_000.0,
        });
        assert!(out.is_none());
    }

    #[test]
    fn invalid_closes_filtered_before_alignment() {
        // null/0/负数过滤；同日无效+有效并存取有效那根
        let out = align_bench(&AlignBenchInput {
            dates: vec!["2026-01-04".into(), "2026-01-06".into(), "2026-01-07".into()],
            bars: vec![
                bar("2026-01-04", Some(4000.0)),
                bar("2026-01-05", None),
                bar("2026-01-06", Some(0.0)),
                bar("2026-01-06", Some(4040.0)),
                bar("2026-01-07", Some(-5.0)),
            ],
            nav0: 100_000.0,
        })
        .unwrap();
        assert_close(&out, &[100_000.0, 101_000.0, 101_000.0]);
    }

    #[test]
    fn degenerate_branches_all_null() {
        let normal_bars = vec![bar("2026-01-05", Some(4000.0)), bar("2026-01-06", Some(4020.0))];
        let dates2 = vec!["2026-01-05".into(), "2026-01-06".into()];
        // bars 空
        assert!(align_bench(&AlignBenchInput { dates: dates2.clone(), bars: vec![], nav0: 1e5 }).is_none());
        // dates < 2
        assert!(align_bench(&AlignBenchInput {
            dates: vec!["2026-01-05".into()],
            bars: normal_bars.clone(),
            nav0: 1e5,
        })
        .is_none());
        // nav0 ≤ 0（负数与 NaN 同分支）
        assert!(align_bench(&AlignBenchInput { dates: dates2.clone(), bars: normal_bars.clone(), nav0: 0.0 }).is_none());
        assert!(align_bench(&AlignBenchInput { dates: dates2.clone(), bars: normal_bars.clone(), nav0: -1.0 }).is_none());
        assert!(align_bench(&AlignBenchInput { dates: dates2, bars: normal_bars, nav0: f64::NAN }).is_none());
    }

    #[test]
    fn single_bar_plateau() {
        // 仅一根有效 bar → 归一后恒定平台（perf ε 守卫把 beta/corr 归 0 的输入形态）
        let out = align_bench(&AlignBenchInput {
            dates: vec!["2026-01-05".into(), "2026-01-06".into(), "2026-01-07".into()],
            bars: vec![bar("2026-01-05", Some(4000.0))],
            nav0: 100_000.0,
        })
        .unwrap();
        assert_eq!(out, vec![100_000.0, 100_000.0, 100_000.0]);
    }
}
