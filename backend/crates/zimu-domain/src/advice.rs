//! 分析结论打分（03 §10 生产口径，P4-T6；ADR-0007：数字归规则，文字归模型）。
//!
//! `score / conf / dims[].s / action / horizon / target / stop / position` 全部由
//! [`score_advice`] 产出——模型文案里的数字只能来自这里的返回（经 agent 工具透传），
//! 每个数字可回溯到本文件与 03 §10 的公式（P4-T6 验收标准）。
//!
//! 口径要点（照抄 03 §10 生产口径，改动先改文档与 `fixtures/golden/advice.json`）：
//! - 信号票 v1..v4（技术）+ vv（量能），数据不足的票**不参与**并如实记降级；
//! - 无数据源维度（基本面/消息面）与量能缺失/平量的资金面记中性值 [`NEUTRAL_DIM`] = 60
//!   （中性带 55–69 的中值，保证全中性组合落「持有」带）；
//! - `score = (2t + m + f + n) / 5`，阈值 70/55/45 三带；
//! - `conf = 0.5 + |Σ票| / (2n票)`，可用票 < 3 → 0.5（样本内信号一致性，**不是胜率**，
//!   CLAUDE.md 安全边界：文案不得写成收益承诺）；
//! - 得分定 action；纪律只附参考（ADR-0023）：四条仓位纪律不拦截、不改写 action——
//!   越线事实以 [`DisciplineRef`] 附注随建议产出（仅偏多带、仅未过项、§6 表序），
//!   15% 目标带与 25% 上限锚仍是 `suggested` 数字来源（sizing 属仓位目标，不属纪律）；
//! - target/stop 锚 = 现价，half-up 舍入到 §1 小数位：`floor(v × 10^d + 0.5) / 10^d`
//!   （与 TS 镜像同一表达式，逐位一致）；
//! - `dims[].t` 观察文字是展示层模板（不进 fixture），事实数字全部取自本模块计算值。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::discipline::{LIMIT_CASH_RESERVE, LIMIT_CB_TOTAL, LIMIT_SECTOR, LIMIT_SINGLE_STOCK};
use crate::lot::price_decimals;
use crate::{Kind, THRESHOLD_EPS};

/// 无数据源/无信号维度的中性分（中性带 55–69 的中值，03 §10 生产口径）
pub const NEUTRAL_DIM: f64 = 60.0;
/// 偏多时的单票目标仓位 %（加仓/买入的建议仓位）
pub const TARGET_WEIGHT: f64 = 15.0;
/// score 带阈值：≥70 偏多 / ≥55 中性 / ≥45 减仓 / <45 卖出（03 §10）
pub const SCORE_BULL: f64 = 70.0;
pub const SCORE_NEUTRAL: f64 = 55.0;
pub const SCORE_TRIM: f64 = 45.0;
/// 每张技术票对维度分的步长：t = 50 + 12.5 × Σ票（4 票满格 → [0, 100]）
const VOTE_STEP: f64 = 12.5;
/// 20 日动量票的 ±5% 带
const MOM_BAND: f64 = 5.0;
/// 30 日区间位置票：> 0.9 追高保护 −1，< 0.2 低位 +1
const POS_HIGH: f64 = 0.9;
const POS_LOW: f64 = 0.2;
/// 量比放量阈值（近5日均量/近30日均量）
const VOL_SURGE: f64 = 1.2;
/// 可用票少于 3 张时 conf 归 0.5（信号样本不足）
const MIN_CONF_VOTES: i32 = 3;
/// 区间位置窗口（近 30 根收盘）
const RANGE_WINDOW: usize = 30;

/// 结论标签（03 §10 action 枚举；中文标签是展示层，由工具/前端映射）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Add,
    Buy,
    Hold,
    Trim,
    Sell,
    Dca,
}

/// 持有周期（03 §10 horizon；标签映射：short 短线 / short_mid 短中线 2–6 周 /
/// mid 中线 1–3 月 / long 长线 6 月+）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Horizon {
    Short,
    ShortMid,
    Mid,
    Long,
}

/// 建议仓位模式：target 到目标仓位 / maintain 维持 / dca 定投 / exit 清仓 / flat 未持仓观望
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PositionMode {
    Target,
    Maintain,
    Dca,
    Exit,
    Flat,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdviceInput {
    pub kind: Kind,
    /// 升序日收盘（末位最新；含当日合成 bar 时即当日价，04 附录 B）
    pub closes: Vec<f64>,
    /// 与 closes 同长的日成交量；元素 null = 该 bar 缺量
    pub volumes: Vec<Option<f64>>,
    /// 现价（target/stop 锚；≤0 视为缺失 → 双 null）
    pub price: f64,
    pub held: bool,
    /// 占总资产 %（recalc weight；未持仓 = 0）
    pub weight: f64,
    /// 纪律参考上下文（ADR-0023：只作参考判定，不参与 action 决策）。
    /// 可选——缺省 = 只出 single_stock ref；旧键 `cash_reserve_pass` 已退役。
    /// 工具层从 `discipline::check` 现算注入（sector_pct 为**该标的自身行业**占市值比，
    /// 与 §6 sector 同分子分母；cb_total_pct 仅转债标的用于参考）。
    #[serde(default)]
    pub discipline_ctx: Option<DisciplineCtx>,
}

/// 纪律参考上下文（百分数值，与 §6 同口径；缺项的规则不参与参考）
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct DisciplineCtx {
    /// 现金占总资产 %（cash_reserve.value）
    pub cash_pct: Option<f64>,
    /// 该标的所在行业占市值 %（分母 total_mv，与 §6 sector 同口径，非组合最大行业）
    pub sector_pct: Option<f64>,
    /// 转债合计占总资产 %（cb_total.value，仅 kind=cb 参与）
    pub cb_total_pct: Option<f64>,
}

/// 纪律参考项（ADR-0023）：仅未过项入列、仅偏多带生成、固定序 = §6 表序。
/// `rule/value/threshold` 进 fixture 比对（数字规则侧产出，ADR-0007 不变）；
/// `label/text` 展示层事实模板，不校验（`dims[].t` 同等待遇）。
/// 文案基调「参考线/不拦截/拍板在你」，禁「拦截/必须降至」字样。
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct DisciplineRef {
    /// 规则键（封闭词汇）："single_stock" | "sector" | "cb_total" | "cash_reserve"
    pub rule: String,
    /// 中文规则名（展示层）
    pub label: String,
    /// 当前值 %
    pub value: f64,
    /// 参考线 %（LIMIT_* 常量）
    pub threshold: f64,
    /// 一句话提示（展示层模板，数字内嵌）
    pub text: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdviceDim {
    /// 维度名（固定四维顺序：技术面/基本面/资金面/消息面，03 §10）
    pub k: String,
    /// 维度分 0–100
    pub s: f64,
    /// 观察文字（展示层模板，事实数字取自 stats；不进 fixture）
    pub t: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PositionAdvice {
    /// 当前仓位 %（计算值）
    pub current: f64,
    /// 建议仓位 %；maintain/dca 为 null（维持/定投无单点目标）
    pub suggested: Option<f64>,
    pub mode: PositionMode,
}

/// 中间量全量透出：复盘/建议文案里引用的每个数字都能在这里回溯
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdviceStats {
    pub n_bars: usize,
    pub ma5: Option<f64>,
    pub ma20: Option<f64>,
    /// (close/close[−6] − 1) × 100，%
    pub mom5: Option<f64>,
    /// (close/close[−21] − 1) × 100，%
    pub mom20: Option<f64>,
    /// (close − low30)/(high30 − low30)；high==low → 0.5；n < 5 → null
    pub pos30: Option<f64>,
    pub high30: Option<f64>,
    pub low30: Option<f64>,
    /// 近5日均量 / 近30日均量
    pub vol_ratio: Option<f64>,
    /// 可用票合计（技术 + 量能）
    pub vote_sum: i32,
    /// 可用票数
    pub vote_count: i32,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdviceOutput {
    pub action: Action,
    pub horizon: Horizon,
    pub score: f64,
    /// 样本内信号一致性 0–1（不是胜率）
    pub conf: f64,
    pub dims: Vec<AdviceDim>,
    pub target_lo: Option<f64>,
    pub target_hi: Option<f64>,
    pub stop: Option<f64>,
    pub position: PositionAdvice,
    /// 纪律参考（仅偏多带非空；序 = §6 表序，ADR-0023）
    pub discipline_refs: Vec<DisciplineRef>,
    pub stats: AdviceStats,
}

/// half-up 舍入到 d 位小数：`floor(v × 10^d + 0.5) / 10^d`（与 TS 镜像同表达式）。
/// 仅对 v > 0 的报价使用（target/stop 锚定正现价）。
fn round_dec(v: f64, d: u32) -> f64 {
    let scale = 10f64.powi(d as i32);
    (v * scale + 0.5).floor() / scale
}

/// 带 ε 守卫的符号（恰好相等 → 0，防浮点噪声翻票，03 附录比对规则第 5 条同款语义）
fn sign_eps(x: f64) -> i32 {
    if x > THRESHOLD_EPS {
        1
    } else if x < -THRESHOLD_EPS {
        -1
    } else {
        0
    }
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// 近 min(n,k) 根均量：窗口内任一 bar 缺量（null / 越界 / ≤0）→ None（不伪造量能信号）
fn vol_mean(volumes: &[Option<f64>], n: usize, k: usize) -> Option<f64> {
    let w = k.min(n);
    if w < 1 || volumes.len() < n {
        return None;
    }
    let seg = &volumes[n - w..n];
    let mut sum = 0.0;
    for v in seg {
        match v {
            Some(x) if *x > 0.0 => sum += x,
            _ => return None,
        }
    }
    Some(sum / w as f64)
}

/// 打分主函数（03 §10 生产口径；黄金向量 `fixtures/golden/advice.json`）。
pub fn score_advice(inp: &AdviceInput) -> AdviceOutput {
    let n = inp.closes.len();
    let last = inp.closes.last().copied();

    // ---- 中间量（每个都进 stats 供回溯）----
    let ma5 = if n >= 5 {
        Some(mean(&inp.closes[n - 5..]))
    } else {
        None
    };
    let ma20 = if n >= 20 {
        Some(mean(&inp.closes[n - 20..]))
    } else {
        None
    };
    let mom_of = |back: usize| -> Option<f64> {
        if n < back + 1 {
            return None;
        }
        let base = inp.closes[n - back - 1];
        if !(base > 0.0) {
            return None;
        }
        last.map(|l| (l / base - 1.0) * 100.0)
    };
    let mom5 = mom_of(5);
    let mom20 = mom_of(20);

    let (high30, low30, pos30) = if n >= 1 {
        let w = &inp.closes[n.saturating_sub(RANGE_WINDOW)..];
        let hi = w.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = w.iter().cloned().fold(f64::INFINITY, f64::min);
        // v4 最少样本 5 根（不足 → 票不参与，pos30 也不报——2 根窗口的「区间位置」是噪声）
        let pos = if n >= 5 {
            Some(if hi > lo {
                (last.unwrap_or(lo) - lo) / (hi - lo)
            } else {
                0.5 // high == low：全平窗口守卫
            })
        } else {
            None
        };
        (Some(hi), Some(lo), pos)
    } else {
        (None, None, None)
    };

    // ---- 信号票 ----
    let mut tech_votes: Vec<i32> = Vec::new();
    if let (Some(a), Some(b)) = (ma5, ma20) {
        tech_votes.push(sign_eps(a - b)); // v1 均线
    }
    if let (Some(l), Some(m)) = (last, ma20) {
        tech_votes.push(sign_eps(l - m)); // v2 价位
    }
    if let Some(mom) = mom20 {
        // v3 动量：±5% 带外才计票
        tech_votes.push(if mom > MOM_BAND + THRESHOLD_EPS {
            1
        } else if mom < -(MOM_BAND + THRESHOLD_EPS) {
            -1
        } else {
            0
        });
    }
    if let Some(p) = pos30 {
        // v4 区间位置：高位追高保护 / 低位关注
        tech_votes.push(if p > POS_HIGH + THRESHOLD_EPS {
            -1
        } else if p < POS_LOW - THRESHOLD_EPS {
            1
        } else {
            0
        });
    }
    let tech_sum: i32 = tech_votes.iter().sum();
    let t = (50.0 + VOTE_STEP * tech_sum as f64).clamp(0.0, 100.0);

    // 量能票：vol_ratio 与 mom5 都可算才参与（缺失 → 中性 60，不投票）
    let vol5 = vol_mean(&inp.volumes, n, 5);
    let vol30 = vol_mean(&inp.volumes, n, RANGE_WINDOW);
    let vol_ratio = match (vol5, vol30) {
        (Some(a), Some(b)) if b > 0.0 => Some(a / b),
        _ => None,
    };
    let (money, money_vote) = match (vol_ratio, mom5) {
        (Some(vr), Some(m5)) if vr >= VOL_SURGE - THRESHOLD_EPS => {
            if m5 > THRESHOLD_EPS {
                (70.0, Some(1)) // 放量上涨：资金流入迹象
            } else if m5 < -THRESHOLD_EPS {
                (30.0, Some(-1)) // 放量下跌：资金流出迹象
            } else {
                (NEUTRAL_DIM, Some(0))
            }
        }
        (Some(_), Some(_)) => (NEUTRAL_DIM, Some(0)), // 平量/缩量：无方向信号
        _ => (NEUTRAL_DIM, None),                      // 数据缺失：不参与
    };

    let vote_sum = tech_sum + money_vote.unwrap_or(0);
    let vote_count = tech_votes.len() as i32 + i32::from(money_vote.is_some());
    let conf = if vote_count < MIN_CONF_VOTES {
        0.5
    } else {
        0.5 + (vote_sum.abs() as f64) / (2.0 * vote_count as f64)
    };

    // 基本面/消息面：无数据源 → 中性（接入后先改 03 §10 与本函数）
    let score = (2.0 * t + money + NEUTRAL_DIM + NEUTRAL_DIM) / 5.0;

    // ---- action 决策表（得分定 action；纪律只附参考，ADR-0023，03 §10 生产口径）----
    let w = inp.weight;
    // over_cap 只决定 suggested 锚取 25 还是 15，不再改写 action（ADR-0023）
    let over_cap = inp.held && w > LIMIT_SINGLE_STOCK + THRESHOLD_EPS;
    let maintain = PositionAdvice {
        current: w,
        suggested: None,
        mode: PositionMode::Maintain,
    };
    let flat = PositionAdvice {
        current: w,
        suggested: Some(0.0),
        mode: PositionMode::Flat,
    };
    let to_target = |target: f64| PositionAdvice {
        current: w,
        suggested: Some(target),
        mode: PositionMode::Target,
    };

    let (action, position) = if score >= SCORE_BULL - THRESHOLD_EPS {
        if !inp.held {
            (Action::Buy, to_target(TARGET_WEIGHT))
        } else if over_cap {
            // 超上限偏多票：action 仍为加仓，suggested 锚上限 25%（上限锚读法「建议 ≤25%」，
            // 非降仓指令——减仓股数归 calc_discipline.reduce_hint 本家，ADR-0023 D2）
            (Action::Add, to_target(LIMIT_SINGLE_STOCK))
        } else if w < TARGET_WEIGHT - THRESHOLD_EPS {
            (Action::Add, to_target(TARGET_WEIGHT))
        } else {
            (Action::Hold, maintain) // 仓位已达目标带（sizing 锚，非纪律），不再加
        }
    } else if score >= SCORE_NEUTRAL - THRESHOLD_EPS {
        if inp.kind == Kind::Etf {
            (
                Action::Dca,
                PositionAdvice {
                    current: w,
                    suggested: None,
                    mode: PositionMode::Dca,
                },
            )
        } else if inp.held {
            (Action::Hold, maintain)
        } else {
            (Action::Hold, flat)
        }
    } else if score >= SCORE_TRIM - THRESHOLD_EPS {
        if inp.held {
            (Action::Trim, to_target(round_dec(w / 2.0, 1))) // 减半，1 位小数
        } else {
            (Action::Hold, flat)
        }
    } else if inp.held {
        (
            Action::Sell,
            PositionAdvice {
                current: w,
                suggested: Some(0.0),
                mode: PositionMode::Exit,
            },
        )
    } else {
        (Action::Hold, flat)
    };

    let horizon = match action {
        Action::Add | Action::Buy => Horizon::Mid,
        Action::Dca => Horizon::Long,
        Action::Hold => {
            if inp.kind == Kind::Etf {
                Horizon::Long
            } else {
                Horizon::Mid
            }
        }
        Action::Trim => Horizon::ShortMid,
        Action::Sell => Horizon::Short,
    };

    // ---- target / stop（锚 = 现价；转债无区间目标、止损 0.95；定投双 null）----
    let d = price_decimals(inp.kind);
    let priced = inp.price > 0.0;
    let stop_factor = if inp.kind == Kind::Cb { 0.95 } else { 0.92 };
    let (target_lo, target_hi) = match action {
        Action::Add | Action::Buy if priced && inp.kind != Kind::Cb => (
            Some(round_dec(inp.price * 1.08, d)),
            Some(round_dec(inp.price * 1.15, d)),
        ),
        _ => (None, None),
    };
    let stop = match action {
        // 买入/加仓必须有止损（03 §10）；持有/减仓/卖出给防线；定投与观望无单点止损
        Action::Add | Action::Buy if priced => Some(round_dec(inp.price * stop_factor, d)),
        Action::Hold | Action::Trim | Action::Sell if priced && inp.held => {
            Some(round_dec(inp.price * stop_factor, d))
        }
        _ => None,
    };

    // ---- dims 观察文字（展示层模板；事实数字全部来自上面算出的中间量）----
    let tech_obs = tech_observation(n, ma5, ma20, mom20, pos30, t);
    let money_obs = money_observation(vol_ratio, mom5, money_vote);
    let dims = vec![
        AdviceDim {
            k: "技术面".into(),
            s: t,
            t: tech_obs,
        },
        AdviceDim {
            k: "基本面".into(),
            s: NEUTRAL_DIM,
            t: "未接入基本面数据源，按中性处理".into(),
        },
        AdviceDim {
            k: "资金面".into(),
            s: money,
            t: money_obs,
        },
        AdviceDim {
            k: "消息面".into(),
            s: NEUTRAL_DIM,
            t: "未接入消息面数据源，按中性处理".into(),
        },
    ];

    // ---- 纪律参考（ADR-0023：仅偏多带生成、仅未过项入列、序 = §6 表序）----
    let discipline_refs = if score >= SCORE_BULL - THRESHOLD_EPS {
        build_discipline_refs(inp, w)
    } else {
        Vec::new()
    };

    AdviceOutput {
        action,
        horizon,
        score,
        conf,
        dims,
        target_lo,
        target_hi,
        stop,
        position,
        discipline_refs,
        stats: AdviceStats {
            n_bars: n,
            ma5,
            ma20,
            mom5,
            mom20,
            pos30,
            high30,
            low30,
            vol_ratio,
            vote_sum,
            vote_count,
        },
    }
}

/// 纪律参考构建（ADR-0023）：仅未过项入列（压线 = 过，THRESHOLD_EPS 复用），
/// 固定序 = §6 表序 single_stock → sector → cb_total → cash_reserve；
/// `text` 展示层事实模板（不进 fixture，`dims[].t` 同待遇），基调「参考线 / 拍板在你」。
fn build_discipline_refs(inp: &AdviceInput, w: f64) -> Vec<DisciplineRef> {
    let mut refs = Vec::new();
    if inp.held && w > LIMIT_SINGLE_STOCK + THRESHOLD_EPS {
        refs.push(DisciplineRef {
            rule: "single_stock".into(),
            label: "单票仓位参考线".into(),
            value: w,
            threshold: LIMIT_SINGLE_STOCK,
            text: format!("当前 {w:.1}% 已越 25% 单票参考线：建议锚为上限，是否执行由你拍板"),
        });
    }
    if let Some(ctx) = inp.discipline_ctx.as_ref() {
        if let Some(pct) = ctx.sector_pct {
            if pct > LIMIT_SECTOR + THRESHOLD_EPS {
                refs.push(DisciplineRef {
                    rule: "sector".into(),
                    label: "行业仓位参考线".into(),
                    value: pct,
                    threshold: LIMIT_SECTOR,
                    text: format!(
                        "所在行业占市值 {pct:.1}% 已越 35% 行业参考线：注意集中度风险，是否执行由你拍板"
                    ),
                });
            }
        }
        if inp.kind == Kind::Cb {
            if let Some(pct) = ctx.cb_total_pct {
                if pct > LIMIT_CB_TOTAL + THRESHOLD_EPS {
                    refs.push(DisciplineRef {
                        rule: "cb_total".into(),
                        label: "转债合计参考线".into(),
                        value: pct,
                        threshold: LIMIT_CB_TOTAL,
                        text: format!(
                            "组合转债合计 {pct:.1}% 已越 12% 转债参考线：仅供参考，是否执行由你拍板"
                        ),
                    });
                }
            }
        }
        if let Some(pct) = ctx.cash_pct {
            if pct < LIMIT_CASH_RESERVE - THRESHOLD_EPS {
                refs.push(DisciplineRef {
                    rule: "cash_reserve".into(),
                    label: "现金保留参考线".into(),
                    value: pct,
                    threshold: LIMIT_CASH_RESERVE,
                    text: format!(
                        "现金仅占 {pct:.1}%，低于 10% 现金参考线：仅供参考，是否执行由你拍板"
                    ),
                });
            }
        }
    }
    refs
}

/// 技术面观察：有什么数据说什么话，缺失如实标注（降级不伪装）
fn tech_observation(
    n: usize,
    ma5: Option<f64>,
    ma20: Option<f64>,
    mom20: Option<f64>,
    pos30: Option<f64>,
    t: f64,
) -> String {
    if n < 5 {
        return format!("日线仅 {n} 根（<5），技术信号样本不足，按中性处理");
    }
    let mut parts: Vec<String> = Vec::new();
    if let (Some(a), Some(b)) = (ma5, ma20) {
        let rel = if a > b { "上方" } else if a < b { "下方" } else { "持平" };
        parts.push(format!("MA5 在 MA20 {rel}"));
    } else {
        parts.push("MA20 样本不足（<20 根）".into());
    }
    match mom20 {
        Some(m) => parts.push(format!("20 日动量 {m:+.1}%")),
        None => parts.push("20 日动量样本不足（<21 根）".into()),
    }
    match pos30 {
        Some(p) => parts.push(format!("30 日区间位置 {:.0}%", p * 100.0)),
        None => {}
    }
    parts.push(format!("技术面得分 {t:.1}"));
    parts.join("；")
}

/// 资金面观察（量能是资金面的代理指标，如实说明）
fn money_observation(vol_ratio: Option<f64>, mom5: Option<f64>, vote: Option<i32>) -> String {
    let (Some(vr), Some(m5), Some(v)) = (vol_ratio, mom5, vote) else {
        return "量能数据缺失，按中性处理（资金面以量能为代理指标）".into();
    };
    let body = match v {
        1 => format!("近 5 日放量（量比 {vr:.2}）且 5 日动量 {m5:+.1}%，资金流入迹象"),
        -1 => format!("近 5 日放量（量比 {vr:.2}）但 5 日动量 {m5:+.1}%，资金流出迹象"),
        _ if vr < 0.8 => format!("近 5 日缩量（量比 {vr:.2}），观望情绪浓"),
        _ => format!("量能平稳（量比 {vr:.2}），无明确方向信号"),
    };
    format!("{body}；资金面以量能为代理指标，非真实资金流数据")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(n: usize) -> AdviceInput {
        AdviceInput {
            kind: Kind::Stock,
            closes: vec![100.0; n],
            volumes: vec![Some(1000.0); n],
            price: 100.0,
            held: true,
            weight: 10.0,
            discipline_ctx: None,
        }
    }

    /// 偏多带全票 +1 上升序列输入（score = 73，fixture av-bull-add 同款形态）
    fn bull_input() -> AdviceInput {
        let mut closes = vec![120.0];
        closes.extend(vec![100.0; 19]);
        closes.extend([104.0, 106.0, 108.0, 110.0, 112.0]);
        let mut volumes = vec![Some(5000.0)];
        volumes.extend(vec![Some(1000.0); 19]);
        volumes.extend(vec![Some(3000.0); 5]);
        AdviceInput {
            kind: Kind::Stock,
            closes,
            volumes,
            price: 112.0,
            held: true,
            weight: 12.0,
            discipline_ctx: None,
        }
    }

    #[test]
    fn empty_closes_is_neutral_hold() {
        let mut inp = flat(0);
        inp.volumes = vec![];
        let out = score_advice(&inp);
        assert_eq!(out.score, 56.0); // 全中性：(2×50+60+60+60)/5
        assert_eq!(out.conf, 0.5); // 可用票 0 < 3
        assert_eq!(out.action, Action::Hold);
        assert_eq!(out.stats.vote_count, 0);
        assert!(out.dims[0].t.contains("样本不足"));
    }

    #[test]
    fn round_dec_is_half_up() {
        assert_eq!(round_dec(120.96, 2), 120.96);
        assert_eq!(round_dec(1.0001, 2), 1.0);
        assert_eq!(round_dec(90.725, 3), 90.725);
        assert_eq!(round_dec(103.04, 2), 103.04);
        // 中点行为以「与 TS 镜像同一表达式」为准（floor(v×10^d+0.5)），不单独承诺十进制语义
    }

    #[test]
    fn discipline_refs_are_advisory_not_blocking() {
        // ADR-0023（fixture av-bull-overweight-cap 同款输入的浓缩版）：偏多 + 单票 26%
        // 越 25% 参考线 → action 仍为加仓、suggested 锚上限 25（mode target）；
        // 加仓带派生继承：target 区间 + stop 全量（旧拦截口径拿不到目标区间）；
        // refs 恰一条 single_stock。
        let mut inp = bull_input();
        inp.weight = 26.0;
        let out = score_advice(&inp);
        assert!(out.score >= SCORE_BULL);
        assert_eq!(out.action, Action::Add);
        assert_eq!(out.position.suggested, Some(LIMIT_SINGLE_STOCK));
        assert_eq!(out.position.mode, PositionMode::Target);
        assert!(out.target_lo.is_some() && out.target_hi.is_some());
        assert_eq!(out.stop, Some(round_dec(112.0 * 0.92, 2)));
        assert_eq!(out.discipline_refs.len(), 1);
        assert_eq!(out.discipline_refs[0].rule, "single_stock");
        assert_eq!(out.discipline_refs[0].value, 26.0);
        assert_eq!(out.discipline_refs[0].threshold, LIMIT_SINGLE_STOCK);
    }

    #[test]
    fn cash_below_ref_line_no_longer_blocks_buy() {
        // ADR-0023（fixture av-bull-notheld-cash-ref 浓缩版）：偏多未持仓 + 现金 6%
        // 低于 10% 参考线 → 照常买入（建议 15%、target/stop 全量），越线只进 refs
        let mut inp = bull_input();
        inp.held = false;
        inp.weight = 0.0;
        inp.discipline_ctx = Some(DisciplineCtx {
            cash_pct: Some(6.0),
            sector_pct: Some(20.0),
            cb_total_pct: Some(0.0),
        });
        let out = score_advice(&inp);
        assert!(out.score >= SCORE_BULL);
        assert_eq!(out.action, Action::Buy);
        assert_eq!(out.position.suggested, Some(TARGET_WEIGHT));
        assert!(out.target_lo.is_some() && out.stop.is_some());
        assert_eq!(out.discipline_refs.len(), 1);
        assert_eq!(out.discipline_refs[0].rule, "cash_reserve");
        assert_eq!(out.discipline_refs[0].value, 6.0);
    }

    #[test]
    fn refs_order_threshold_gates_and_band_gate() {
        // §6 表序 + 压线 = 过 + cb 仅转债 + 非偏多带恒空（fixture av-bull-multi-ref 扩展网）
        let mut inp = bull_input();
        inp.weight = 27.0;
        inp.discipline_ctx = Some(DisciplineCtx {
            cash_pct: Some(8.0),
            sector_pct: Some(38.0),
            cb_total_pct: Some(13.0), // stock 不参与转债参考
        });
        let out = score_advice(&inp);
        let rules: Vec<&str> = out.discipline_refs.iter().map(|r| r.rule.as_str()).collect();
        assert_eq!(rules, ["single_stock", "sector", "cash_reserve"]);

        // 压线 = 过（四条全部恰等于阈值 → refs 空）
        let mut inp2 = bull_input();
        inp2.weight = 25.0;
        inp2.discipline_ctx = Some(DisciplineCtx {
            cash_pct: Some(10.0),
            sector_pct: Some(35.0),
            cb_total_pct: Some(12.0),
        });
        assert!(score_advice(&inp2).discipline_refs.is_empty());

        // 转债标的：cb_total 参与参考
        let mut inp3 = bull_input();
        inp3.kind = Kind::Cb;
        inp3.discipline_ctx = Some(DisciplineCtx {
            cash_pct: Some(12.0),
            sector_pct: None,
            cb_total_pct: Some(13.0),
        });
        let out3 = score_advice(&inp3);
        let rules3: Vec<&str> = out3.discipline_refs.iter().map(|r| r.rule.as_str()).collect();
        assert_eq!(rules3, ["cb_total"]);

        // 非偏多带恒空：中性带 + ctx 越线 → refs 仍为 []（口径最小变更，ADR-0023 D3）
        let mut inp4 = flat(25);
        inp4.discipline_ctx = Some(DisciplineCtx {
            cash_pct: Some(5.0),
            sector_pct: Some(40.0),
            cb_total_pct: None,
        });
        assert!(score_advice(&inp4).discipline_refs.is_empty());
    }

    #[test]
    fn observation_texts_carry_facts() {
        let inp = flat(25);
        let out = score_advice(&inp);
        assert!(out.dims[0].t.contains("MA20"));
        assert!(out.dims[1].t.contains("未接入基本面数据源"));
        assert!(out.dims[2].t.contains("量比"));
        assert!(out.dims[3].t.contains("未接入消息面数据源"));
        // 四维顺序固定（03 §10）
        let ks: Vec<&str> = out.dims.iter().map(|d| d.k.as_str()).collect();
        assert_eq!(ks, ["技术面", "基本面", "资金面", "消息面"]);
    }

    #[test]
    fn missing_price_kills_target_and_stop() {
        let mut inp = flat(25);
        inp.price = 0.0;
        let out = score_advice(&inp);
        assert!(out.target_lo.is_none() && out.target_hi.is_none() && out.stop.is_none());
    }
}
