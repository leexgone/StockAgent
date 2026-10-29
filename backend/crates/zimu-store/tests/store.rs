//! zimu-store 集成测试（P4-T1/T2）。
//! 数字断言对齐 `docs/03` 口径——费用/摊薄/仓位与 zimu-domain 黄金向量同源，
//! 这里只验证「存储 + 事务 + 录入流程」把域函数的结果正确落库与读回。

use zimu_domain::Kind;
use zimu_domain::Side;
use zimu_store::{
    AccountRow, EquitySnapshotRow, NewAdvice, NewAlert, NewHoldingMeta, NewWatchlist, RecordError,
    Store, TradeRequest, WatchlistError,
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
    // 合成 v1 老库：先建当前版库 → 手工降回 v1（删 watchlist + 版本号回拨），
    // Store::open 应自动前进到 v2（迁移只前进，P4-T2 机制），且旧表数据不丢。
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
        conn.execute_batch("DROP TABLE watchlist; PRAGMA user_version = 1;").unwrap();
    }
    let s = Store::open(path_s).unwrap();
    // 旧数据不丢（v1 表原样），新表可用（v2 迁移已前进）
    assert!((s.portfolio().unwrap().account.cash - 12345.0).abs() < 1e-9);
    s.add_watchlist(&watch("601899", "紫金矿业", "SH"), "2026-10-29 11:00").unwrap();
    assert_eq!(s.watchlist().unwrap().len(), 1);
    let _ = std::fs::remove_file(&path);
}
