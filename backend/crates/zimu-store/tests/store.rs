//! zimu-store 集成测试（P4-T1/T2）。
//! 数字断言对齐 `docs/03` 口径——费用/摊薄/仓位与 zimu-domain 黄金向量同源，
//! 这里只验证「存储 + 事务 + 录入流程」把域函数的结果正确落库与读回。

use zimu_domain::Kind;
use zimu_domain::Side;
use zimu_store::{
    AccountRow, EditOp, EquitySnapshotRow, HoldingEditError, HoldingUpsert, NewAdvice, NewAlert,
    NewHoldingMeta, NewWatchlist, RecordError, Store, TradeRequest, WatchlistError,
};

fn meta(code_name: (&str, &str), kind: Kind, prev: Option<f64>, pct: Option<i64>) -> NewHoldingMeta {
    NewHoldingMeta {
        name: code_name.1.to_string(),
        mkt: "SH".into(),
        kind,
        sector: "测试".into(),
        prev,
        limit_pct: pct,
    }
}

fn buy(code: &str, qty: i64, price: f64, m: NewHoldingMeta) -> TradeRequest {
    TradeRequest {
        code: code.into(),
        side: Side::Buy,
        qty,
        price,
        note: String::new(),
        meta: Some(m),
    }
}

fn seed_store(cash: f64) -> Store {
    let s = Store::in_memory().unwrap();
    s.set_account(&AccountRow {
        cash,
        ..AccountRow::default()
    })
    .unwrap();
    s
}

#[test]
fn fresh_db_has_empty_portfolio_and_account_cash() {
    let s = seed_store(100_000.0);
    let p = s.portfolio().unwrap();
    assert_eq!(p.holdings.len(), 0);
    assert!((p.account.cash - 100_000.0).abs() < 1e-9);
    // 空持仓：总资产 = 现金，各比率为 0（A1/B1 除零防护口径）
    assert!((p.agg.total_assets - 100_000.0).abs() < 1e-9);
    assert_eq!(p.agg.exposure, 0.0);
    assert_eq!(p.agg.hhi, 0.0);
}

#[test]
fn buy_new_holding_creates_row_and_deducts_cash() {
    let s = seed_store(100_000.0);
    // 买 100 股 @10：成交额 1000，佣金 max(5, 1000×0.00025=0.25)=5，过户费 1000×0.00001=0.01，
    // 买入无印花税 → 费用 5.01；现金 100000 - 1000 - 5.01 = 98994.99
    let r = s.record_trade(&buy("600000", 100, 10.0, meta(("600000", "测试股"), Kind::Stock, None, None))).unwrap();
    assert_eq!(r.holding_after.as_ref().unwrap().shares, 100);
    let acc = s.account().unwrap();
    assert!((acc.cash - 98994.99).abs() < 1e-6, "cash_after={}", acc.cash);
    // 摊薄成本 = (成交额 + 费用) / 股数
    let h = s.holdings().unwrap().remove(0);
    assert!((h.cost - (1000.0 + 5.01) / 100.0).abs() < 1e-9);
}

#[test]
fn add_to_existing_dilutes_cost() {
    let s = seed_store(100_000.0);
    s.record_trade(&buy("600000", 100, 10.0, meta(("600000", "测试股"), Kind::Stock, None, None)))
        .unwrap();
    // 再加 100 股 @20：不传 meta 也应命中已有行
    let r = s
        .record_trade(&TradeRequest {
            code: "600000".into(),
            side: Side::Buy,
            qty: 100,
            price: 20.0,
            note: String::new(),
            meta: None,
        })
        .unwrap();
    assert_eq!(r.holding_after.as_ref().unwrap().shares, 200);
    // 摊薄成本 = (首次成本×100 + 2000 + 费用2)/200，与 03 §4 一致
    let outcome = &r.preview.outcome;
    assert!((outcome.after_shares - 200.0).abs() < 1e-9);
}

#[test]
fn sell_to_zero_removes_holding() {
    let s = seed_store(100_000.0);
    s.record_trade(&buy("510300", 1000, 4.0, meta(("510300", "ETF"), Kind::Etf, None, None)))
        .unwrap();
    let r = s
        .record_trade(&TradeRequest {
            code: "510300".into(),
            side: Side::Sell,
            qty: 1000,
            price: 4.2,
            note: String::new(),
            meta: None,
        })
        .unwrap();
    assert!(r.removed);
    assert!(r.holding_after.is_none());
    assert_eq!(s.holdings().unwrap().len(), 0);
    // ETF 卖出免印花税、免过户费，仅佣金
    assert!(r.preview.outcome.realized.is_some());
}

#[test]
fn rejects_odd_lot_sell() {
    let s = seed_store(100_000.0);
    s.record_trade(&buy("600000", 100, 10.0, meta(("600000", "测试股"), Kind::Stock, None, None)))
        .unwrap();
    // 股票整手 100；卖 50 应被整手校验拒绝（B12 定稿前与原型一致）
    let e = s.record_trade(&TradeRequest {
        code: "600000".into(),
        side: Side::Sell,
        qty: 50,
        price: 10.0,
        note: String::new(),
        meta: None,
    });
    assert!(matches!(e, Err(RecordError::BadInput(_))), "got {e:?}");
}

#[test]
fn rejects_sell_exceeding_held() {
    let s = seed_store(100_000.0);
    s.record_trade(&buy("600000", 100, 10.0, meta(("600000", "测试股"), Kind::Stock, None, None)))
        .unwrap();
    let e = s.record_trade(&TradeRequest {
        code: "600000".into(),
        side: Side::Sell,
        qty: 200,
        price: 10.0,
        note: String::new(),
        meta: None,
    });
    assert!(matches!(e, Err(RecordError::BadInput(_))));
}

#[test]
fn rejects_out_of_limit_price() {
    let s = seed_store(100_000.0);
    // 昨收 10、±10% → 涨停 11 / 跌停 9；买 12 越界
    let e = s.record_trade(&buy("600000", 100, 12.0, meta(("600000", "测试股"), Kind::Stock, Some(10.0), Some(10))));
    match e {
        Err(RecordError::OutOfLimitRange { lo, hi, .. }) => {
            assert!((lo - 9.0).abs() < 1e-6 && (hi - 11.0).abs() < 1e-6);
        }
        other => panic!("expected OutOfLimitRange, got {other:?}"),
    }
    assert_eq!(s.holdings().unwrap().len(), 0, "被拒的买入不应落库");
}

#[test]
fn limit_prices_skip_when_no_prev() {
    let s = seed_store(100_000.0);
    // prev/limit_pct 缺省 → 跳过区间拦截（03 §2 无行情语义），任意正价可成交
    let r = s.record_trade(&buy("600000", 100, 999.0, meta(("600000", "测试股"), Kind::Stock, None, None)));
    assert!(r.is_ok());
}

#[test]
fn rejects_insufficient_cash_and_rolls_back() {
    let s = seed_store(1_000.0);
    // 买 100 股 @50 = 5000 > 现金 1000 → 拒；持仓与现金都不应变化
    let e = s.record_trade(&buy("600000", 100, 50.0, meta(("600000", "测试股"), Kind::Stock, None, None)));
    assert!(matches!(e, Err(RecordError::InsufficientCash { .. })));
    assert_eq!(s.holdings().unwrap().len(), 0);
    assert!((s.account().unwrap().cash - 1_000.0).abs() < 1e-9);
}

#[test]
fn over_limit_is_warning_not_reject() {
    // 现金 100 万、单笔买 30 万市值 → 占组合 30% > 25% 上限：
    // 交易必须成功（原型语义：纪律提示不拦提交），warnings 带「25」
    let s = seed_store(1_000_000.0);
    let r = s
        .record_trade(&buy("600001", 30000, 10.0, meta(("600001", "重仓股"), Kind::Stock, None, None)))
        .unwrap();
    assert!(
        r.preview.warnings.iter().any(|w| w.contains("25")),
        "warnings={:?}",
        r.preview.warnings
    );
    assert!(r.preview.outcome.over_limit);
}

#[test]
fn sells_nonexistent_holding_rejected() {
    let s = seed_store(100_000.0);
    let e = s.record_trade(&TradeRequest {
        code: "000999".into(),
        side: Side::Sell,
        qty: 100,
        price: 10.0,
        note: String::new(),
        meta: None,
    });
    assert!(matches!(e, Err(RecordError::NoHolding { .. })));
}

#[test]
fn buy_new_without_meta_rejected() {
    let s = seed_store(100_000.0);
    let e = s.record_trade(&TradeRequest {
        code: "600000".into(),
        side: Side::Buy,
        qty: 100,
        price: 10.0,
        note: String::new(),
        meta: None,
    });
    assert!(matches!(e, Err(RecordError::NeedMeta { .. })));
}

#[test]
fn portfolio_recalc_matches_weights() {
    let s = seed_store(100_000.0);
    s.record_trade(&buy("600000", 1000, 10.0, meta(("600000", "A"), Kind::Stock, Some(10.0), Some(10))))
        .unwrap();
    // 手动设置现价，模拟行情填充（price 字段）
    let p = s.portfolio().unwrap();
    assert_eq!(p.per.len(), 1);
    // weight = mv / total_assets × 100
    let mv = p.holdings[0].price.unwrap() * p.holdings[0].shares as f64;
    let expect_w = mv / p.agg.total_assets * 100.0;
    assert!((p.per[0].weight - expect_w).abs() < 1e-6);
}

#[test]
fn trade_log_records_fee_breakdown_and_order() {
    let s = seed_store(100_000.0);
    s.record_trade(&buy("600000", 100, 10.0, meta(("600000", "A"), Kind::Stock, None, None)))
        .unwrap();
    s.record_trade(&TradeRequest {
        code: "600000".into(),
        side: Side::Sell,
        qty: 100,
        price: 12.0,
        note: "止盈".into(),
        meta: None,
    })
    .unwrap();
    let log = s.trades(10).unwrap();
    // 最新在前
    assert_eq!(log[0].side, Side::Sell);
    assert_eq!(log[0].note, "止盈");
    assert!(log[0].realized.is_some());
    assert_eq!(log[1].side, Side::Buy);
    // 卖出股票含印花税 1200×0.0005=0.6
    assert!(log[0].fee_stamp > 0.0);
    assert_eq!(log[1].fee_stamp, 0.0, "买入不计印花税");
}

#[test]
fn update_account_merges_and_guards_cash() {
    let s = Store::in_memory().unwrap();
    // 缺失字段保持原值（默认账户名不被覆盖）
    let acc = s.update_account(None, None, None, None, Some(50_000.0)).unwrap();
    assert!((acc.cash - 50_000.0).abs() < 1e-9);
    assert_eq!(acc.name, "我的账户");
    let acc = s.update_account(Some("短线账户".into()), None, None, None, None).unwrap();
    assert_eq!(acc.name, "短线账户");
    assert!((acc.cash - 50_000.0).abs() < 1e-9, "未提供 cash 不应重置");
    // 负现金拒绝
    assert!(matches!(
        s.update_account(None, None, None, None, Some(-1.0)),
        Err(RecordError::BadInput(_))
    ));
    // 录入流程因此可用：先配现金再记第一笔
    s.record_trade(&buy("600000", 100, 10.0, meta(("600000", "测试股"), Kind::Stock, None, None)))
        .unwrap();
    assert_eq!(s.holdings().unwrap().len(), 1);
}

#[test]
fn restart_does_not_lose_data() {
    // P4-T2 验收：重启不丢数据 —— 用临时文件跑一遍，重开读取一致
    let dir = std::env::temp_dir().join(format!("zimu-store-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.db");
    let _ = std::fs::remove_file(&path);

    let code;
    let cash_snapshot;
    {
        let s = Store::open(path.to_str().unwrap()).unwrap();
        s.set_account(&AccountRow {
            cash: 200_000.0,
            ..Default::default()
        })
        .unwrap();
        // 100 股 × 1500 = 15 万，昨收 1490 涨停 1639 内 ✓
        s.record_trade(&buy("600519", 100, 1500.0, meta(("600519", "贵州茅台"), Kind::Stock, Some(1490.0), Some(10))))
            .unwrap();
        let p = s.portfolio().unwrap();
        code = p.holdings[0].code.clone();
        cash_snapshot = p.account.cash;
        assert_eq!(s.version().unwrap(), zimu_store::CURRENT_VERSION);
    }
    // 重新打开同一文件
    {
        let s = Store::open(path.to_str().unwrap()).unwrap();
        let p = s.portfolio().unwrap();
        assert_eq!(p.holdings.len(), 1);
        assert_eq!(p.holdings[0].code, code);
        assert!((p.account.cash - cash_snapshot).abs() < 1e-6, "重启后现金应一致");
        assert_eq!(s.trades(10).unwrap().len(), 1);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ============================================================================
// 行情回写（P4-T4）
// ============================================================================

use zimu_store::QuoteUpdate;

fn qu(code: &str, price: Option<f64>, prev: Option<f64>) -> QuoteUpdate {
    QuoteUpdate {
        code: code.into(),
        price,
        prev,
        name: None,
        limit_pct: None,
        ts: "2026-10-28 15:05".into(),
    }
}

#[test]
fn apply_quotes_updates_price_prev_name_and_ts() {
    let s = seed_store(300_000.0);
    s.record_trade(&buy("600519", 100, 1500.0, meta(("600519", "贵州茅台"), Kind::Stock, Some(1490.0), Some(10))))
        .unwrap();
    let n = s
        .apply_quotes(&[QuoteUpdate {
            name: Some("贵州茅台A".into()),
            ..qu("600519", Some(1437.0), Some(1421.0))
        }])
        .unwrap();
    assert_eq!(n, 1);
    let h = &s.holdings().unwrap()[0];
    assert_eq!(h.price, Some(1437.0));
    assert_eq!(h.prev, Some(1421.0));
    assert_eq!(h.name, "贵州茅台A");
    assert_eq!(h.updated_at, "2026-10-28 15:05");
    // cost/shares 是录入事实，行情回写永不触碰
    assert_eq!(h.shares, 100);
    // portfolio 立即用新价重算（recalc 链）：mv = 143700，day_chg = 16/1421×100
    let p = s.portfolio().unwrap();
    assert!((p.per[0].mv - 143_700.0).abs() < 1e-9);
    assert!((p.per[0].day_chg - 16.0 / 1421.0 * 100.0).abs() < 1e-9);
}

#[test]
fn apply_quotes_coalesce_keeps_existing_on_none() {
    let s = seed_store(300_000.0);
    // meta 带数据层配置的 limit_pct（ETF 无法推断，03 §2）
    let mut m = meta(("510300", "沪深300ETF"), Kind::Etf, Some(4.0), Some(10));
    m.mkt = "SH".into();
    s.record_trade(&buy("510300", 1000, 4.0, m)).unwrap();
    // price=None（无价不覆盖）、limit_pct=None（数据层配置不被抹掉）
    let n = s
        .apply_quotes(&[QuoteUpdate {
            name: Some("300ETF".into()),
            ..qu("510300", None, Some(4.05))
        }])
        .unwrap();
    assert_eq!(n, 1);
    let h = &s.holdings().unwrap()[0];
    assert_eq!(h.price, Some(4.0), "None 不覆盖原值（COALESCE）");
    assert_eq!(h.prev, Some(4.05), "Some 正常更新");
    assert_eq!(h.limit_pct, Some(10), "数据层配置不被 None 覆盖");
    assert_eq!(h.name, "300ETF");
}

#[test]
fn missing_prev_degrades_to_zero_day_pnl() {
    // 03 §5 无行情降级（P4-T4 定稿）：prev 缺失视为等于 price → dayChg/dayVal = 0。
    // 回归：曾因 prev.unwrap_or(0.0) 把 dayVal 算成整笔市值（伪造当日盈利）。
    let s = seed_store(300_000.0);
    s.record_trade(&buy("600519", 100, 1500.0, meta(("600519", "贵州茅台"), Kind::Stock, None, None)))
        .unwrap();
    let p = s.portfolio().unwrap();
    assert_eq!(p.per[0].day_chg, 0.0);
    assert_eq!(p.per[0].day_val, 0.0);
    assert_eq!(p.agg.day_pnl, 0.0);
    assert_eq!(p.agg.day_pnl_pct, 0.0);
    // mv 照常按成交占位价算（录入时 price=成交价，03 §5 降级只影响当日口径）
    assert!((p.per[0].mv - 1500.0 * 100.0).abs() < 1e-9);
}

#[test]
fn apply_quotes_unknown_code_skipped_silently() {
    let s = seed_store(300_000.0); // 600519 @1500×100 = 15 万，现金要够建仓
    s.record_trade(&buy("600519", 100, 1500.0, meta(("600519", "贵州茅台"), Kind::Stock, Some(1490.0), Some(10))))
        .unwrap();
    let n = s
        .apply_quotes(&[
            qu("600519", Some(1437.0), None),
            qu("999999", Some(1.0), None), // 不在持仓里：跳过，不报错
        ])
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(s.holdings().unwrap().len(), 1);
}

// ============================================================================
// 行业补全（P7-T6/ADR-0016）
// ============================================================================

fn upsert_row(s: &Store, code: &str, sector: &str) {
    s.upsert_holding(
        &HoldingUpsert {
            code: code.into(),
            name: format!("标的{code}"),
            mkt: "SH".into(),
            kind: Kind::Stock,
            sector: sector.into(),
            shares: 100,
            cost: 10.0,
            limit_pct: None,
            note: String::new(),
        },
        "2026-11-15 10:00",
    )
    .unwrap();
}

#[test]
fn fill_sectors_fills_empty_only() {
    let s = seed_store(100_000.0);
    upsert_row(&s, "600519", ""); // 空串 → 应补
    upsert_row(&s, "601166", "手工行业"); // 手工非空 → 永不覆盖
    let n = s
        .fill_sectors(
            &[
                ("600519".into(), "酿酒行业".into()),
                ("601166".into(), "金融行业".into()),
            ],
            "2026-11-15 12:00",
        )
        .unwrap();
    assert_eq!(n, 1, "只补空串行");
    let hs = s.holdings().unwrap();
    let h519 = hs.iter().find(|h| h.code == "600519").unwrap();
    let h166 = hs.iter().find(|h| h.code == "601166").unwrap();
    assert_eq!(h519.sector, "酿酒行业");
    assert_eq!(h166.sector, "手工行业", "手工非空值不被自动覆盖");
}

#[test]
fn fill_sectors_unknown_code_skipped_silently() {
    let s = seed_store(100_000.0);
    upsert_row(&s, "600519", "");
    let n = s
        .fill_sectors(
            &[
                ("600519".into(), "酿酒行业".into()),
                ("999999".into(), "未知".into()),
            ],
            "2026-11-15 12:00",
        )
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(s.holdings().unwrap().len(), 1);
}

#[test]
fn fill_sectors_idempotent_second_call_zero() {
    let s = seed_store(100_000.0);
    upsert_row(&s, "600519", "");
    let fills = [("600519".into(), "酿酒行业".into())];
    assert_eq!(s.fill_sectors(&fills, "2026-11-15 12:00").unwrap(), 1);
    assert_eq!(
        s.fill_sectors(&fills, "2026-11-15 13:00").unwrap(),
        0,
        "二次幂等 0 行"
    );
}

#[test]
fn fill_sectors_touches_updated_at() {
    let s = seed_store(100_000.0);
    upsert_row(&s, "600519", "");
    s.fill_sectors(&[("600519".into(), "酿酒行业".into())], "2026-11-15 12:00")
        .unwrap();
    assert_eq!(s.holdings().unwrap()[0].updated_at, "2026-11-15 12:00");
}

#[test]
fn upsert_after_autofill_manual_wins() {
    let s = seed_store(100_000.0);
    upsert_row(&s, "600519", "");
    s.fill_sectors(&[("600519".into(), "酿酒行业".into())], "2026-11-15 12:00")
        .unwrap();
    // 手工 upsert 整行覆盖（带新 sector）→ 手工恒赢
    upsert_row(&s, "600519", "主人自定义");
    assert_eq!(s.holdings().unwrap()[0].sector, "主人自定义");
    // 此后自动补全不再碰（非空）
    let n = s
        .fill_sectors(&[("600519".into(), "酿酒行业".into())], "2026-11-15 14:00")
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(s.holdings().unwrap()[0].sector, "主人自定义");
}

#[test]
fn upsert_clearing_sector_reenables_autofill() {
    let s = seed_store(100_000.0);
    upsert_row(&s, "600519", "酿酒行业");
    // 用户把 sector 改回空串 = 重新请求自动分类（03 §13）
    upsert_row(&s, "600519", "");
    assert_eq!(s.holdings().unwrap()[0].sector, "");
    let n = s
        .fill_sectors(&[("600519".into(), "电力行业".into())], "2026-11-15 12:00")
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(s.holdings().unwrap()[0].sector, "电力行业");
}

// ============================================================================
// 建议留档（P4-T6：B9 正式解的写入侧，ADR-0007 数字归规则/文字归模型）
// ============================================================================

fn new_advice(code: &str, generated_at: &str) -> NewAdvice {
    NewAdvice {
        code: code.into(),
        action: "持有".into(),
        score: 56.0,
        conf: 0.5,
        target_lo: None,
        target_hi: None,
        stop: None,
        position: "当前 10.0% → 维持".into(),
        dims: serde_json::json!([
            { "k": "技术面", "s": 50.0, "t": "量能平稳" },
            { "k": "基本面", "s": 60.0, "t": "未接入基本面数据源，按中性处理" },
            { "k": "资金面", "s": 60.0, "t": "量能平稳" },
            { "k": "消息面", "s": 60.0, "t": "未接入消息面数据源，按中性处理" }
        ]),
        discipline_refs: serde_json::json!([
            { "rule": "cash_reserve", "label": "现金保留参考线", "value": 8.0, "threshold": 10.0, "text": "现金仅占 8.0%，低于 10% 现金参考线：仅供参考，是否执行由你拍板" }
        ]),
        data_version: "adv-v1|k=none|q=none".into(),
        generated_at: generated_at.into(),
    }
}

#[test]
fn advice_roundtrip_numbers_then_text() {
    let s = seed_store(300_000.0);
    let mut a = new_advice("600519", "2026-10-28 15:05");
    a.action = "加仓".into();
    a.score = 73.0;
    a.conf = 0.9;
    a.target_lo = Some(120.96);
    a.target_hi = Some(128.8);
    a.stop = Some(103.04);
    a.position = "当前 12.0% → 建议 15%".into();
    a.data_version = "adv-v1|k=2026-10-28|q=2026-10-28T15:00:03+08:00".into();
    let id = s.insert_advice(&a).unwrap();

    // 数字先留档：reason 空、risks 空（等模型文案回填）
    let row = s.advice(id).unwrap().unwrap();
    assert_eq!(row.reason, "");
    assert!(row.risks.is_empty());
    assert_eq!(row.score, 73.0);
    assert_eq!(row.dims[0]["k"], "技术面");
    assert_eq!(row.dims.as_array().unwrap().len(), 4);

    // 文案回填只动 reason/risks——数字列不可改写（ADR-0007 的结构性保证）
    assert!(s
        .finalize_advice(id, "回踩不破 MA20，量能配合。", &["动量转弱则结论失效".to_string()])
        .unwrap());
    let row = s.advice(id).unwrap().unwrap();
    assert_eq!(row.reason, "回踩不破 MA20，量能配合。");
    assert_eq!(row.risks, vec!["动量转弱则结论失效".to_string()]);
    assert_eq!(row.score, 73.0);
    assert_eq!(row.stop, Some(103.04));
    assert_eq!(row.generated_at, "2026-10-28 15:05");
}

#[test]
fn advice_finalize_missing_id_is_false() {
    let s = seed_store(1.0);
    assert!(!s.finalize_advice(999, "x", &["y".to_string()]).unwrap());
    assert!(s.advice(999).unwrap().is_none());
}

#[test]
fn advice_discipline_refs_roundtrip_and_null_fallback() {
    // ADR-0023：refs 列往返（new_advice 带一条 cash_reserve 参考）
    let s = seed_store(300_000.0);
    let id = s.insert_advice(&new_advice("600519", "2026-11-26 10:00")).unwrap();
    let row = s.advice(id).unwrap().unwrap();
    let refs = row.discipline_refs.as_array().unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0]["rule"], "cash_reserve");
    assert_eq!(refs[0]["value"], 8.0);
    assert_eq!(refs[0]["threshold"], 10.0);
    // list_advice 同款映射
    assert_eq!(s.list_advice(Some("600519"), 10).unwrap()[0].discipline_refs, row.discipline_refs);

    // 旧行语义：refs 列 NULL → 读回 []（存储层兜底，前端恒拿到数组）——文件库直改列模拟 v4 旧行
    let dir = std::env::temp_dir().join(format!("zimu-store-refs-null-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("refs-null.db");
    let _ = std::fs::remove_file(&path);
    let path_s = path.to_str().expect("临时路径是合法 UTF-8");
    {
        let s2 = Store::open(path_s).unwrap();
        s2.insert_advice(&new_advice("600519", "2026-11-26 10:01")).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("UPDATE advice_history SET discipline_refs = NULL;").unwrap();
    }
    let s3 = Store::open(path_s).unwrap();
    let old_row = s3.list_advice(None, 10).unwrap().remove(0);
    assert_eq!(old_row.discipline_refs, serde_json::json!([]), "NULL 旧行读回空数组");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn advice_list_orders_new_first_and_filters_code() {
    let s = seed_store(1.0);
    let a = s.insert_advice(&new_advice("600519", "2026-10-27 15:05")).unwrap();
    let b = s.insert_advice(&new_advice("600519", "2026-10-28 15:05")).unwrap();
    let c = s.insert_advice(&new_advice("510300", "2026-10-28 15:06")).unwrap();
    let all = s.list_advice(None, 50).unwrap();
    assert_eq!(all.iter().map(|r| r.id).collect::<Vec<_>>(), vec![c, b, a]);
    let only = s.list_advice(Some("600519"), 50).unwrap();
    assert_eq!(only.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b, a]);
    assert_eq!(s.list_advice(None, 2).unwrap().len(), 2);
}

// ============================================================================
// 预警 / 净值快照 / 推送日志（P4-T7）
// ============================================================================

fn new_alert(ts: &str, code: &str, t: &str) -> NewAlert {
    NewAlert {
        ts: ts.into(),
        code: code.into(),
        name: "测试股".into(),
        alert_type: t.into(),
        level: "down".into(),
        text: "测试文案".into(),
    }
}

#[test]
fn alerts_dedup_list_and_done() {
    let s = seed_store(1.0);
    let a = s.insert_alert(&new_alert("2026-10-29 09:35", "600000", "跌破成本")).unwrap();
    s.insert_alert(&new_alert("2026-10-29 10:32", "600000", "放量")).unwrap();
    s.insert_alert(&new_alert("2026-10-28 14:47", "600000", "跌破成本")).unwrap();

    // 去重依据（03 §12）：同日 (code,type) → true；跨日/异型/异码 → false
    assert!(s.alert_exists_on("600000", "跌破成本", "2026-10-29").unwrap());
    assert!(s.alert_exists_on("600000", "跌破成本", "2026-10-28").unwrap());
    assert!(!s.alert_exists_on("600000", "跌破成本", "2026-10-30").unwrap());
    assert!(!s.alert_exists_on("600000", "触及止盈线", "2026-10-29").unwrap());
    assert!(!s.alert_exists_on("600001", "跌破成本", "2026-10-29").unwrap());

    // 新在前（ts DESC, id DESC），未归档 done=false
    let all = s.list_alerts(None, 50).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].ts, "2026-10-29 10:32");
    assert_eq!(all[1].id, a); // 09:35 那条
    assert_eq!(all[2].ts, "2026-10-28 14:47"); // 跨日最旧
    assert!(all.iter().all(|r| !r.done));

    // 归档：返回行且 done=true；重复归档幂等；不存在 id → None
    let row = s.mark_alert_done(a).unwrap().unwrap();
    assert!(row.done && row.id == a);
    assert!(s.mark_alert_done(a).unwrap().unwrap().done);
    assert!(s.mark_alert_done(999).unwrap().is_none());
    assert_eq!(s.list_alerts(Some(false), 50).unwrap().len(), 2);
    assert_eq!(s.list_alerts(Some(true), 50).unwrap().len(), 1);
}

#[test]
fn equity_snapshot_upsert_idempotent() {
    let s = seed_store(100_000.0);
    let agg = s.portfolio().unwrap().agg; // 空持仓：total_assets = cash = 100000
    s.upsert_equity_snapshot(&EquitySnapshotRow::from_agg("2026-10-29", "2026-10-29 15:05", &agg))
        .unwrap();
    // 同日重跑盘后链：REPLACE 不产生第二行，后写覆盖（服务重启幂等）
    let mut snap2 = EquitySnapshotRow::from_agg("2026-10-29", "2026-10-29 15:06", &agg);
    snap2.total_assets = 100_001.0;
    s.upsert_equity_snapshot(&snap2).unwrap();
    s.upsert_equity_snapshot(&EquitySnapshotRow::from_agg("2026-10-28", "2026-10-28 15:05", &agg))
        .unwrap();

    let rows = s.equity_snapshots(50).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].date, "2026-10-28"); // date 升序（净值曲线输入形态）
    assert_eq!(rows[1].ts, "2026-10-29 15:06");
    assert!((rows[1].total_assets - 100_001.0).abs() < 1e-9);
    assert!((rows[1].cash - 100_000.0).abs() < 1e-9);
    // limit 取最近 N 条（截断后仍升序）
    let last1 = s.equity_snapshots(1).unwrap();
    assert_eq!(last1.len(), 1);
    assert_eq!(last1[0].date, "2026-10-29");
    assert!(s.equity_snapshot("2026-10-29").unwrap().is_some());
    assert!(s.equity_snapshot("2026-10-27").unwrap().is_none());
}

#[test]
fn push_log_roundtrip() {
    let s = seed_store(1.0);
    s.insert_push_log("2026-10-29 15:05", "agent", "post_market_review", true, "复盘已注入会话")
        .unwrap();
    s.insert_push_log("2026-10-30 15:05", "agent", "post_market_review", false, "agent 未在线")
        .unwrap();
    let rows = s.push_logs(50).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ts, "2026-10-30 15:05"); // 新在前
    assert!(!rows[0].ok && rows[0].detail == "agent 未在线");
    assert!(rows[1].ok && rows[1].channel == "agent");
}

fn watch(code: &str, name: &str, mkt: &str) -> NewWatchlist {
    NewWatchlist {
        code: code.into(),
        name: name.into(),
        mkt: mkt.into(),
        kind: Kind::Stock,
        limit_pct: Some(10),
        sector: "电子".into(),
        note: "观察：AI 视觉商业化".into(),
    }
}

#[test]
fn watchlist_upsert_remove_and_validation() {
    let s = seed_store(100_000.0);
    let r1 = s.add_watchlist(&watch("002415", "海康威视", "SZ"), "2026-10-29 10:00").unwrap();
    assert_eq!(r1.added_at, "2026-10-29 10:00");
    assert_eq!(r1.limit_pct, Some(10));

    // upsert：重复添加更新备注/元信息，added_at 保留首次值（幂等，不报错）
    let mut w2 = watch("002415", "海康威视", "SZ");
    w2.note = "备注更新".into();
    let r2 = s.add_watchlist(&w2, "2026-10-30 09:00").unwrap();
    assert_eq!(r2.note, "备注更新");
    assert_eq!(r2.added_at, "2026-10-29 10:00");
    assert_eq!(s.watchlist().unwrap().len(), 1);

    // 排序 = 加入时间序
    s.add_watchlist(&watch("601899", "紫金矿业", "SH"), "2026-10-29 11:00").unwrap();
    let all = s.watchlist().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].code, "002415"); // 10:00 先加入
    assert_eq!(all[1].code, "601899"); // 11:00 后加入

    // 校验：市场码非法 → BadInput（B7 显式 mkt，不按代码猜）
    assert!(matches!(
        s.add_watchlist(&watch("002415", "海康威视", "BJ"), "x"),
        Err(WatchlistError::BadInput(_))
    ));
    // 名称非空校验
    assert!(matches!(
        s.add_watchlist(&watch("002416", "  ", "SZ"), "x"),
        Err(WatchlistError::BadInput(_))
    ));

    // 删除：首次返回行，再次 None（幂等）
    assert!(s.remove_watchlist("002415").unwrap().is_some());
    assert!(s.remove_watchlist("002415").unwrap().is_none());
    assert_eq!(s.watchlist().unwrap().len(), 1);
}

#[test]
fn watchlist_migrates_v1_db_to_v2() {
    // 合成 v1 老库：先建当前版库 → 手工降回 v1（删 v1 之后新增的表 + 版本号回拨），
    // Store::open 应自动前进到当前版（迁移只前进，P4-T2 机制），且旧表数据不丢。
    let dir = std::env::temp_dir().join(format!("zimu-store-mig-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("mig-v1.db");
    let _ = std::fs::remove_file(&path);
    let path_s = path.to_str().expect("临时路径是合法 UTF-8");
    {
        let s = Store::open(path_s).unwrap();
        s.set_account(&AccountRow { cash: 12345.0, ..AccountRow::default() }).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        // v1 之后新增的表全删（迁移重放是 CREATE TABLE，表还在会撞 already exists）；
        // v5 起有 ALTER ADD COLUMN 迁移——重放前须 DROP COLUMN 还原（duplicate column）。
        // 加版迁移时记得在这里补 DROP——本行清单与 CURRENT_VERSION 同步演进。
        conn.execute_batch(
            "DROP TABLE watchlist; DROP TABLE holding_edits;
             DROP TABLE chat_messages; DROP TABLE channel_bindings;
             ALTER TABLE advice_history DROP COLUMN discipline_refs;
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let s = Store::open(path_s).unwrap();
    // 旧数据不丢（v1 表原样），新表可用（v2/v3 迁移已前进）
    assert_eq!(s.version().unwrap(), zimu_store::CURRENT_VERSION);
    assert!((s.portfolio().unwrap().account.cash - 12345.0).abs() < 1e-9);
    s.add_watchlist(&watch("601899", "紫金矿业", "SH"), "2026-10-29 11:00").unwrap();
    assert_eq!(s.watchlist().unwrap().len(), 1);
    s.upsert_holding(&up("600519", "贵州茅台", 100, 1400.0), "2026-11-01 10:00").unwrap();
    assert_eq!(s.holding_edits(10).unwrap().len(), 1);
    let _ = std::fs::remove_file(&path);
}

// ============================================================================
// 持仓直接维护（ADR-0009 / 03 §13：快照校正——不动现金、不写 trades、留编辑痕）
// ============================================================================

fn up(code: &str, name: &str, shares: i64, cost: f64) -> HoldingUpsert {
    HoldingUpsert {
        code: code.into(),
        name: name.into(),
        mkt: "SH".into(),
        kind: Kind::Stock,
        sector: "测试".into(),
        shares,
        cost,
        limit_pct: None,
        note: String::new(),
    }
}

#[test]
fn upsert_creates_odd_lot_holding_with_edit_trail() {
    let s = seed_store(100_000.0);
    // 奇零股 150 直接录入（红利零股真实存在；lot 校验只约束委托路径，03 §13）
    let (row, changed) = s.upsert_holding(&up("600519", "贵州茅台", 150, 1400.5), "2026-11-01 10:00").unwrap();
    assert!(changed);
    assert_eq!(row.shares, 150);
    assert!((row.cost - 1400.5).abs() < 1e-9);
    assert!(row.price.is_none() && row.prev.is_none() && row.limit_pct.is_none());

    // 快照校正三不动：现金、trades、已实现盈亏
    assert!((s.account().unwrap().cash - 100_000.0).abs() < 1e-9);
    assert_eq!(s.trades(10).unwrap().len(), 0);

    // 编辑痕：create，before 空、after 是行快照
    let edits = s.holding_edits(10).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].op, EditOp::Create);
    assert!(edits[0].before.is_none());
    let after = edits[0].after.as_ref().unwrap();
    assert_eq!(after.shares, 150);
    assert!((after.cost - 1400.5).abs() < 1e-9);
    assert_eq!(edits[0].ts, "2026-11-01 10:00");
}

#[test]
fn upsert_idempotent_same_fields_writes_nothing() {
    let s = seed_store(0.0);
    s.upsert_holding(&up("600519", "贵州茅台", 100, 1400.0), "2026-11-01 10:00").unwrap();
    // 同一表单原样重提交 → no-op（不写库、不留痕、不报错）
    let (row, changed) = s.upsert_holding(&up("600519", "贵州茅台", 100, 1400.0), "2026-11-01 10:05").unwrap();
    assert!(!changed);
    assert_eq!(row.updated_at, "2026-11-01 10:00", "幂等路径不刷新 updated_at");
    assert_eq!(s.holding_edits(10).unwrap().len(), 1);
}

#[test]
fn upsert_update_preserves_quotes_and_keeps_cash_out() {
    let s = seed_store(100_000.0);
    let mut req = up("600519", "贵州茅台", 100, 1400.0);
    req.limit_pct = Some(10);
    s.upsert_holding(&req, "2026-11-01 10:00").unwrap();
    // 刷新链回写行情（price/prev 归它管）
    s.apply_quotes(&[QuoteUpdate {
        code: "600519".into(),
        price: Some(1437.0),
        prev: Some(1421.0),
        name: None,
        limit_pct: None,
        ts: "2026-11-01 10:30".into(),
    }]).unwrap();

    // 校正：改 shares/cost，limit_pct 留空 = 保留原值（03 §13）
    let (row, changed) = s.upsert_holding(&up("600519", "贵州茅台", 200, 1350.0), "2026-11-01 11:00").unwrap();
    assert!(changed);
    assert_eq!(row.shares, 200);
    assert!((row.cost - 1350.0).abs() < 1e-9);
    assert_eq!(row.price, Some(1437.0), "price 是行情字段，校正不得清空");
    assert_eq!(row.prev, Some(1421.0));
    assert_eq!(row.limit_pct, Some(10), "limit_pct 留空保留原值");
    assert!((s.account().unwrap().cash - 100_000.0).abs() < 1e-9);
    assert_eq!(s.trades(10).unwrap().len(), 0, "校正不伪造流水");

    let edits = s.holding_edits(10).unwrap();
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].op, EditOp::Update);
    let (b, a) = (edits[0].before.as_ref().unwrap(), edits[0].after.as_ref().unwrap());
    assert_eq!((b.shares, a.shares), (100, 200));
    assert!((b.cost - 1400.0).abs() < 1e-9 && (a.cost - 1350.0).abs() < 1e-9);
}

#[test]
fn upsert_bad_inputs_rejected() {
    let s = seed_store(0.0);
    let cases: Vec<HoldingUpsert> = vec![
        up("60051", "五位代码", 100, 10.0),                    // 代码非 6 位
        up("60051a", "含字母", 100, 10.0),                      // 非数字
        { let mut r = up("600519", "北交所", 100, 10.0); r.mkt = "BJ".into(); r },
        up("600519", "  ", 100, 10.0),                          // 名称空
        up("600519", "零股数", 0, 10.0),                        // shares < 1
        up("600519", "负成本", 100, -1.0),                      // cost < 0
        up("600519", "NaN 成本", 100, f64::NAN),                // NaN 守卫
        { let mut r = up("600519", "零比例", 100, 10.0); r.limit_pct = Some(0); r },
    ];
    for req in cases {
        assert!(
            matches!(s.upsert_holding(&req, "x"), Err(HoldingEditError::BadInput(_))),
            "应拒：{} {}", req.code, req.name
        );
    }
    assert_eq!(s.holdings().unwrap().len(), 0, "拒绝路径不写库");
    assert_eq!(s.holding_edits(10).unwrap().len(), 0, "拒绝路径不留痕");
}

#[test]
fn remove_holding_keeps_trades_and_logs_delete() {
    let s = seed_store(200_000.0); // 100 股 @1400 = 14 万，现金要够
    // 走交易路径建仓（产生流水），再直接删除（场外变动语义）
    s.record_trade(&buy("600519", 100, 1400.0, meta(("600519", "贵州茅台"), Kind::Stock, None, Some(10)))).unwrap();
    let cash_before = s.account().unwrap().cash;

    let removed = s.remove_holding("600519", "2026-11-01 12:00").unwrap();
    assert!(removed.is_some());
    assert_eq!(s.holdings().unwrap().len(), 0);
    // 流水保留、现金不动、已实现不回滚（03 §13 删除语义）
    assert_eq!(s.trades(10).unwrap().len(), 1);
    assert!((s.account().unwrap().cash - cash_before).abs() < 1e-9);

    // 重复删 → None（HTTP 404），不再留痕
    assert!(s.remove_holding("600519", "2026-11-01 12:01").unwrap().is_none());
    let edits = s.holding_edits(10).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].op, EditOp::Delete);
    assert!(edits[0].after.is_none());
    let before = edits[0].before.as_ref().unwrap();
    assert_eq!(before.shares, 100);
    assert_eq!(edits[0].name, "贵州茅台", "留痕带操作时刻名称，删行后仍可展示");
}

#[test]
fn holding_edits_newest_first_with_limit() {
    let s = seed_store(0.0);
    for i in 0..5 {
        s.upsert_holding(&up(&format!("60051{i}"), "测试", 100 + i, 10.0), "ts").unwrap();
    }
    let edits = s.holding_edits(3).unwrap();
    assert_eq!(edits.len(), 3);
    // id 降序 = 最近在前
    assert!(edits[0].id > edits[1].id && edits[1].id > edits[2].id);
    assert_eq!(edits[0].code, "600514");
}

// ============================================================================
// 会话档案与通道绑定（schema v4，T2/ADR-0011）
// ============================================================================

#[test]
fn chat_messages_append_read_clear() {
    let dir = std::env::temp_dir().join(format!("zimu-chat-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("chat-v4.db");
    let _ = std::fs::remove_file(&path);
    let s = Store::open(path.to_str().unwrap()).unwrap();

    // 追加：user → tool 展示行 → assistant
    let a = s.append_chat_message("web", "user", "茅台什么价", None, "2026-11-07 10:00").unwrap();
    let b = s
        .append_chat_message("web", "tool", "", Some(r#"{"name":"get_quote"}"#), "2026-11-07 10:00")
        .unwrap();
    let c = s.append_chat_message("web", "assistant", "结论：现价 1400。", None, "2026-11-07 10:01").unwrap();
    assert!(a < b && b < c, "id 单调递增即时间序");

    // 读：新在前，tool 行带 tool_json
    let (rows, has_more) = s.chat_messages("web", 10, None).unwrap();
    assert!(!has_more);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].id, c);
    assert_eq!(rows[1].role, "tool");
    assert!(rows[1].tool_json.is_some() && rows[1].text == "");
    assert_eq!(rows[2].text, "茅台什么价");

    // 分页：limit=1 报 has_more；before_id 翻页拿到更早的
    let (p1, more1) = s.chat_messages("web", 1, None).unwrap();
    assert!(more1 && p1.len() == 1 && p1[0].id == c);
    let (p2, more2) = s.chat_messages("web", 1, Some(c)).unwrap();
    assert!(more2 && p2[0].id == b);
    let (p3, more3) = s.chat_messages("web", 1, Some(b)).unwrap();
    assert!(!more3 && p3[0].id == a);

    // 通道隔离：别的 session 看不见 web 的行
    s.append_chat_message("other", "user", "串台测试", None, "t").unwrap();
    let (rows, _) = s.chat_messages("web", 100, None).unwrap();
    assert_eq!(rows.len(), 3, "会话键隔离（通道独立：本表只归 Web）");

    // CHECK 约束：非法 role 拒收
    assert!(s.append_chat_message("web", "thinking", "不该入库", None, "t").is_err(),
        "thinking 段被词汇 CHECK 挡在档案外（框架语义：不写回 history）");

    // 清空：只清指定会话
    assert_eq!(s.clear_chat_messages("web").unwrap(), 3);
    let (rows, _) = s.chat_messages("web", 10, None).unwrap();
    assert!(rows.is_empty());
    let (rows, _) = s.chat_messages("other", 10, None).unwrap();
    assert_eq!(rows.len(), 1, "clear 不误伤其他会话");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn channel_bindings_bind_rebind_unbind_and_hot_check() {
    let dir = std::env::temp_dir().join(format!("zimu-bind-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("bind-v4.db");
    let _ = std::fs::remove_file(&path);
    let s = Store::open(path.to_str().unwrap()).unwrap();

    s.bind_channel("qq", "private:ABC", "private", "本人", "2026-11-07 09:00").unwrap();
    s.bind_channel("qq", "group:XYZ", "group", "家庭群", "2026-11-07 09:01").unwrap();
    assert!(s.is_channel_bound("qq", "private:ABC").unwrap());
    assert!(!s.is_channel_bound("qq", "private:OTHER").unwrap());
    // 跨通道同 id 不互认（chat_id 命名空间撞名时白名单不越权）
    assert!(!s.is_channel_bound("wechat", "private:ABC").unwrap());

    // unbind 留痕：判定转 false，行还在（active=0）
    assert!(s.unbind_channel("qq", "group:XYZ").unwrap());
    assert!(!s.is_channel_bound("qq", "group:XYZ").unwrap());
    assert!(!s.unbind_channel("qq", "group:XYZ").unwrap(), "重复解绑不命中活跃行");
    let all = s.channel_bindings(None).unwrap();
    assert_eq!(all.len(), 1, "列表只报活跃绑定");

    // 重绑幂等 upsert：刷新 label/bound_at、拉回 active，不产生双行
    s.bind_channel("qq", "group:XYZ", "group", "家人群改名", "2026-11-07 15:00").unwrap();
    let all = s.channel_bindings(Some("qq")).unwrap();
    assert_eq!(all.len(), 2);
    let g = all.iter().find(|b| b.chat_id == "group:XYZ").unwrap();
    assert_eq!(g.label, "家人群改名");
    assert_eq!(g.bound_at, "2026-11-07 15:00");
    // bound_at 升序：先绑在前
    assert_eq!(all[0].chat_id, "private:ABC");
    let _ = std::fs::remove_file(&path);
}

/// 真实库**副本**升版冒烟（P4-T8 双库纪律：原件永不打开——ZIMU_SMOKE_DB 指副本！）。
/// 手工跑法（有真实库时）：
///   Copy-Item <真实 zimu.db> $env:TEMP\zimu-v4-smoke.db
///   $env:ZIMU_SMOKE_DB="$env:TEMP\zimu-v4-smoke.db"; cargo test -p zimu-store --test store -- --ignored
/// 副本应被 Store::open 自动前进到 CURRENT_VERSION，旧数据读数完好、v4 新表可用。
#[test]
#[ignore]
fn upgrade_copy_of_real_db_to_current() {
    let path = std::env::var("ZIMU_SMOKE_DB")
        .expect("ZIMU_SMOKE_DB 必须指向真实库的副本路径（绝不指原件）");
    let s = Store::open(&path).unwrap();
    assert_eq!(s.version().unwrap(), zimu_store::CURRENT_VERSION);
    // 升版后基本读数完好（不断言具体数字——真实库内容不属于测试假设）
    let _ = s.portfolio().unwrap();
    let _ = s.push_logs(1).unwrap();
    // v4 新表读写可用（探针用独立 session/channel 键，不污染 'web' 档案词汇）
    s.append_chat_message("smoke", "user", "升版探针", None, "2026-11-07 00:00").unwrap();
    s.bind_channel("smoke", "probe", "private", "升版探针", "2026-11-07 00:00").unwrap();
    assert!(s.is_channel_bound("smoke", "probe").unwrap());
    let (rows, more) = s.chat_messages("smoke", 10, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!more);
    s.clear_chat_messages("smoke").unwrap();
    s.unbind_channel("smoke", "probe").unwrap();
}

#[test]
fn migrates_v3_db_to_current() {
    // 合成 v3 老库 → Store::open 自动前进到 CURRENT_VERSION（迁移只前进），旧表数据不丢、新表可用。
    let dir = std::env::temp_dir().join(format!("zimu-store-mig4-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("mig-v3.db");
    let _ = std::fs::remove_file(&path);
    let path_s = path.to_str().expect("临时路径是合法 UTF-8");
    {
        let s = Store::open(path_s).unwrap();
        s.set_account(&AccountRow { cash: 23456.0, ..AccountRow::default() }).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        // v5 是 ALTER ADD COLUMN——重放前还原列，否则 duplicate column（同 v1 重放测试）
        conn.execute_batch(
            "DROP TABLE chat_messages; DROP TABLE channel_bindings;
             ALTER TABLE advice_history DROP COLUMN discipline_refs;
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }
    let s = Store::open(path_s).unwrap();
    assert_eq!(s.version().unwrap(), zimu_store::CURRENT_VERSION);
    assert!((s.portfolio().unwrap().account.cash - 23456.0).abs() < 1e-9, "v3 旧数据不丢");
    // v4 新表读写可用
    s.append_chat_message("web", "user", "迁移后第一句", None, "2026-11-07 10:00").unwrap();
    s.bind_channel("qq", "private:MIG", "private", "迁移", "2026-11-07 10:00").unwrap();
    assert!(s.is_channel_bound("qq", "private:MIG").unwrap());
    let (rows, _) = s.chat_messages("web", 10, None).unwrap();
    assert_eq!(rows.len(), 1);
    let _ = std::fs::remove_file(&path);
}
