//! zimu-store 集成测试（P4-T1/T2）。
//! 数字断言对齐 `docs/03` 口径——费用/摊薄/仓位与 zimu-domain 黄金向量同源，
//! 这里只验证「存储 + 事务 + 录入流程」把域函数的结果正确落库与读回。

use zimu_domain::Kind;
use zimu_domain::Side;
use zimu_store::{AccountRow, NewHoldingMeta, RecordError, Store, TradeRequest};

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
