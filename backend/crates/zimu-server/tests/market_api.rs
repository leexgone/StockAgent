//! zimu-server 集成测试（P4-T4）：`/api/market/*` + 持仓刷新链。
//!
//! 验收「API 数字与 03 口径一致」的两条腿：
//! 1. 手算具体数字（mv/dayChg/dayVal/limit_pct 推断/无行情降级）逐个断言；
//! 2. 等价性：API 的 per/agg 与 `zimu_domain::recalc`（黄金向量锁死的 03 实现）
//!    对同一输入逐位一致——证明 HTTP 链路上没有任何一处私改口径。
//!
//! 验收「sidecar 挂了主程序降级」：全端点 503/200 语义 + 时钟退工作日启发式。
//! 全部离线：FakeApi 注入行情（tests/common 共享装置），真实 HTTP 打本机回环
//! 临时端口。契约 schema 校验（P4-T5）在 tests/contract.rs。

mod common;

use common::*;
use serde_json::{json, Value};
use zimu_domain::recalc::{recalc, Holding, PortfolioInput};
use zimu_domain::Kind;
use zimu_domain::Side;
use zimu_market::clock::now_shanghai;
use zimu_market::error::MarketError;
use zimu_market::models::*;
use zimu_store::{AccountRow, NewHoldingMeta, Store, TradeRequest};

// ============================================================================
// 刷新链：行情回写 → recalc 重算 → API 数字 = 03 口径
// ============================================================================

#[tokio::test]
async fn refresh_updates_holdings_and_numbers_match_domain() {
    let fake = FakeApi::new();
    let today = today();
    fake.set_quote(Ok(qpayload(vec![
        qrow("sh600519", "stock", Some(1437.0), Some(1421.0), QuoteStatus::Ok, Some(&today)),
        qrow("sh510300", "etf", Some(4.1), Some(4.05), QuoteStatus::Ok, Some(&today)),
        qrow("sz128136", "cb", None, None, QuoteStatus::NoData, None),
    ])));
    let base = spawn_api(fake, seed()).await;
    let http = reqwest::Client::new();

    let r = http
        .post(format!("{base}/api/market/refresh"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["updated"], json!(2), "no_data 的转债不回写");
    let skipped = body["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["code"], json!("128136"));
    assert_eq!(body["degraded"], json!(false));

    let h: Value = http
        .get(format!("{base}/api/holdings"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = h["holdings"].as_array().unwrap();
    let find = |c: &str| rows.iter().find(|x| x["code"] == json!(c)).unwrap().clone();

    // 回写落库：价格/昨收/名称更新；limit_pct 推断（600 主板 → 10）；
    // ETF 无法推断（03 §2）→ 保持 null；skipped 行保持库存旧值
    assert_eq!(find("600519")["price"], json!(1437.0));
    assert_eq!(find("600519")["prev"], json!(1421.0));
    assert_eq!(find("600519")["limit_pct"], json!(10));
    assert_eq!(find("510300")["price"], json!(4.1));
    assert_eq!(find("510300")["limit_pct"], Value::Null);
    assert_eq!(find("128136")["price"], json!(120.0));
    assert_eq!(find("128136")["prev"], Value::Null);

    // 验收①：手算数字（03 §5 公式）
    let per = h["per"].as_array().unwrap();
    let per_of = |c: &str| per.iter().find(|x| x["code"] == json!(c)).unwrap().clone();
    let p1 = per_of("600519");
    assert!((p1["mv"].as_f64().unwrap() - 1437.0 * 100.0).abs() < 1e-9);
    // dayChg = (1437−1421)/1421×100；dayVal = 16×100 = 1600
    assert!((p1["day_chg"].as_f64().unwrap() - 16.0 / 1421.0 * 100.0).abs() < 1e-9);
    assert!((p1["day_val"].as_f64().unwrap() - 1600.0).abs() < 1e-9);
    // 无行情标的：dayChg/dayVal = 0（03 §5 无行情降级，不伪造盈亏）
    let p3 = per_of("128136");
    assert_eq!(p3["day_val"].as_f64().unwrap(), 0.0);
    assert_eq!(p3["day_chg"].as_f64().unwrap(), 0.0);
    // weight 分母 = 含现金总资产（03 §5：weight = mv/totalAssets）
    let total_assets = h["agg"]["total_assets"].as_f64().unwrap();
    assert!((p1["weight"].as_f64().unwrap() - 143700.0 / total_assets * 100.0).abs() < 1e-9);

    // 验收②：与 zimu-domain::recalc 等价（同一输入逐位一致，容差 1e-9）
    let inp = PortfolioInput {
        cash: h["account"]["cash"].as_f64().unwrap(),
        holdings: rows
            .iter()
            .map(|r| {
                let price = r["price"].as_f64().unwrap_or_else(|| r["cost"].as_f64().unwrap());
                Holding {
                    code: r["code"].as_str().unwrap().to_string(),
                    kind: match r["kind"].as_str().unwrap() {
                        "stock" => Kind::Stock,
                        "etf" => Kind::Etf,
                        _ => Kind::Cb,
                    },
                    sector: r["sector"].as_str().unwrap().to_string(),
                    shares: r["shares"].as_f64().unwrap(),
                    cost: r["cost"].as_f64().unwrap(),
                    price,
                    prev: r["prev"].as_f64().unwrap_or(price), // 03 §5 无行情降级同款约定
                }
            })
            .collect(),
    };
    let expect = recalc(&inp);
    assert_eq!(expect.per.len(), per.len());
    for (e, a) in expect.per.iter().zip(per) {
        assert_eq!(e.code, a["code"].as_str().unwrap());
        for (label, ev, av) in [
            ("day_chg", e.day_chg, &a["day_chg"]),
            ("day_val", e.day_val, &a["day_val"]),
            ("mv", e.mv, &a["mv"]),
            ("pnl", e.pnl, &a["pnl"]),
            ("pnl_pct", e.pnl_pct, &a["pnl_pct"]),
            ("weight", e.weight, &a["weight"]),
        ] {
            assert!(
                (ev - av.as_f64().unwrap()).abs() < 1e-9,
                "{}.{}: domain={ev} api={av}",
                e.code,
                label
            );
        }
    }
    let agg = &h["agg"];
    for (label, ev, av) in [
        ("cash", expect.agg.cash, &agg["cash"]),
        ("mv_stocks", expect.agg.mv_stocks, &agg["mv_stocks"]),
        ("mv_etf", expect.agg.mv_etf, &agg["mv_etf"]),
        ("mv_cb", expect.agg.mv_cb, &agg["mv_cb"]),
        ("total_mv", expect.agg.total_mv, &agg["total_mv"]),
        ("total_assets", expect.agg.total_assets, &agg["total_assets"]),
        ("day_pnl", expect.agg.day_pnl, &agg["day_pnl"]),
        ("day_pnl_pct", expect.agg.day_pnl_pct, &agg["day_pnl_pct"]),
        ("total_pnl", expect.agg.total_pnl, &agg["total_pnl"]),
        ("cost_total", expect.agg.cost_total, &agg["cost_total"]),
        ("total_pnl_pct", expect.agg.total_pnl_pct, &agg["total_pnl_pct"]),
        ("exposure", expect.agg.exposure, &agg["exposure"]),
        ("hhi", expect.agg.hhi, &agg["hhi"]),
        ("top3_sector", expect.agg.top3_sector, &agg["top3_sector"]),
    ] {
        assert!(
            (ev - av.as_f64().unwrap()).abs() < 1e-9,
            "agg.{label}: domain={ev} api={av}"
        );
    }
    let sectors = agg["sectors"].as_array().unwrap();
    assert_eq!(sectors.len(), expect.agg.sectors.len(), "行业数（按市值降序）");
    for (e, s) in expect.agg.sectors.iter().zip(sectors) {
        assert_eq!(e.name, s["name"].as_str().unwrap());
        assert!((e.v - s["v"].as_f64().unwrap()).abs() < 1e-9);
        assert!((e.w - s["w"].as_f64().unwrap()).abs() < 1e-9);
    }
}

#[tokio::test]
async fn refresh_with_empty_holdings_skips_sidecar() {
    let fake = FakeApi::new(); // 全不可达——若被调用即失败
    let base = spawn_api(fake.clone(), Store::in_memory().unwrap()).await;
    let http = reqwest::Client::new();
    let r = http
        .post(format!("{base}/api/market/refresh"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["updated"], json!(0));
    assert!(v["as_of"].is_null());
    assert!(fake.call_log().is_empty(), "空持仓不打 sidecar");
}

#[tokio::test]
async fn refresh_writes_limit_pct_only_when_unconfigured() {
    // 数据层已配置的 limit_pct（如 ETF 显式 10）永不被推断覆盖（03 §2 / 04 附录 B）
    let fake = FakeApi::new();
    let today = today();
    fake.set_quote(Ok(qpayload(vec![qrow(
        "sh510300",
        "etf",
        Some(4.1),
        Some(4.05),
        QuoteStatus::Ok,
        Some(&today),
    )])));
    let s = Store::in_memory().unwrap();
    s.set_account(&AccountRow {
        cash: 100_000.0,
        ..Default::default()
    })
    .unwrap();
    s.record_trade(&TradeRequest {
        code: "510300".into(),
        side: Side::Buy,
        qty: 1000,
        price: 4.0,
        note: String::new(),
        meta: Some(NewHoldingMeta {
            name: "沪深300ETF".into(),
            mkt: "SH".into(),
            kind: Kind::Etf,
            sector: "宽基指数".into(),
            prev: Some(4.0),
            limit_pct: Some(10), // 数据层配置
        }),
    })
    .unwrap();
    let base = spawn_api(fake, s).await;
    let http = reqwest::Client::new();
    assert_eq!(
        http.post(format!("{base}/api/market/refresh"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let h: Value = http
        .get(format!("{base}/api/holdings"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["holdings"][0]["limit_pct"], json!(10));
    assert_eq!(h["holdings"][0]["price"], json!(4.1));
}

// ============================================================================
// 降级：sidecar 挂了主程序不瘫
// ============================================================================

#[tokio::test]
async fn sidecar_down_degrades_not_crashes() {
    let fake = FakeApi::new(); // 默认全 Unavailable
    let base = spawn_api(fake, seed()).await;
    let http = reqwest::Client::new();

    // refresh → 503 契约错误（不写库）
    let r = http
        .post(format!("{base}/api/market/refresh"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 503);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["error"], json!("market_unavailable"));
    assert!(b["message"].as_str().unwrap().contains("sidecar"));

    // quote → 503
    let r = http
        .get(format!("{base}/api/market/quote?codes=sh600519"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 503);

    // 持仓/流水照常（库存陈旧数据，降级不断供）
    let r = http
        .get(format!("{base}/api/holdings"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let h: Value = r.json().await.unwrap();
    assert_eq!(h["holdings"].as_array().unwrap().len(), 3);
    assert!(h["agg"]["total_assets"].as_f64().unwrap() > 0.0);
    let r = http
        .get(format!("{base}/api/trades"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // status 永远 200：sidecar 离线如实标注，时钟退工作日启发式
    let r = http
        .get(format!("{base}/api/market/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let s: Value = r.json().await.unwrap();
    assert_eq!(s["sidecar"]["online"], json!(false));
    assert!(s["sidecar"]["error"].is_string());
    assert_eq!(s["calendar"]["available"], json!(false));
    assert_eq!(s["clock"]["calendar_degraded"], json!(true));
    assert!(!s["clock"]["state"].as_str().unwrap().is_empty());
    assert!(s["clock"]["state_text"].as_str().unwrap().len() > 0);
    assert!(s["clock"]["time"].as_str().unwrap().contains(':'));
    assert!(s["clock"]["next_trade_day"].is_string());
}

#[tokio::test]
async fn upstream_errors_pass_through_with_status() {
    let fake = FakeApi::new();
    let base = spawn_api(fake.clone(), seed()).await;
    let http = reqwest::Client::new();

    // sidecar 上游全链失败 → 502 + 错误码透传（本层不重定义语义）
    fake.set_quote(Err(MarketError::Upstream {
        status: 502,
        code: "UPSTREAM_ALL_FAILED".into(),
        message: "全部行情源失败".into(),
    }));
    let r = http
        .get(format!("{base}/api/market/quote?codes=sh600519"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 502);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["error"], json!("UPSTREAM_ALL_FAILED"));

    // 裸代码 → sidecar 400 BAD_SYMBOL 透传
    fake.set_quote(Err(MarketError::Upstream {
        status: 400,
        code: "BAD_SYMBOL".into(),
        message: "必须带显式市场前缀".into(),
    }));
    let r = http
        .get(format!("{base}/api/market/quote?codes=600519"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert_eq!(r.json::<Value>().await.unwrap()["error"], json!("BAD_SYMBOL"));

    // 空 codes 在本层就拦（不打 sidecar）
    let before = fake.call_log().len();
    let r = http
        .get(format!("{base}/api/market/quote?codes="))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"],
        json!("bad_params")
    );
    assert_eq!(fake.call_log().len(), before, "空 codes 不应触达 sidecar");
}

// ============================================================================
// 透传契约：quote / kline（当日 bar 合成）/ cb_list / calendar / status
// ============================================================================

#[tokio::test]
async fn quote_passthrough_keeps_degradation_fields() {
    let fake = FakeApi::new();
    let today = today();
    let mut qp = qpayload(vec![qrow(
        "sh600519",
        "stock",
        Some(1437.0),
        Some(1421.0),
        QuoteStatus::Ok,
        Some(&today),
    )]);
    qp.degraded = true;
    qp.attempts = vec![Attempt {
        source: "sina_hq".into(),
        reason: "ConnectTimeout: timed out".into(),
    }];
    fake.set_quote(Ok(qp));
    let base = spawn_api(fake, seed()).await;
    let http = reqwest::Client::new();
    let v: Value = http
        .get(format!("{base}/api/market/quote?codes=sh600519"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["degraded"], json!(true), "降级可观测性必须透传");
    assert_eq!(v["attempts"][0]["source"], json!("sina_hq"));
    assert_eq!(v["quotes"][0]["status"], json!("ok"));
    assert_eq!(v["quotes"][0]["price"], json!(1437.0));
    assert_eq!(v["quotes"][0]["date"], json!(today));
}

#[tokio::test]
async fn kline_with_today_synthesizes_from_live_quote() {
    let fake = FakeApi::new();
    let today = today();
    let yesterday = (now_shanghai().date_naive() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    fake.set_kline(Ok(KlinePayload {
        symbol: "sh600519".into(),
        kind: "stock".into(),
        kind_routed: "explicit".into(),
        adjusted: "qfq".into(),
        start: "2026-10-01".into(),
        end: today.clone(),
        count: 1,
        source: Some("sina_stock_daily".into()),
        degraded: false,
        attempts: vec![],
        kline: vec![bar(&yesterday, 1421.0)], // 日线尾部 = 上一交易日（收盘后源侧滞后形态）
    }));
    fake.set_quote(Ok(qpayload(vec![qrow(
        "sh600519",
        "stock",
        Some(1437.0),
        Some(1421.0),
        QuoteStatus::Ok,
        Some(&today),
    )])));
    let base = spawn_api(fake.clone(), seed()).await;
    let http = reqwest::Client::new();

    // with_today=true → 合成当日 bar（04 附录 B）
    let v: Value = http
        .get(format!(
            "{base}/api/market/kline/sh600519?with_today=true"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["today_synthesized"], json!(true));
    let kl = v["kline"].as_array().unwrap();
    assert_eq!(kl.len(), 2);
    assert_eq!(kl[1]["date"], json!(today));
    assert_eq!(kl[1]["close"], json!(1437.0), "close = 实时价");
    assert_eq!(v["count"], json!(2), "count 与合成后数组同步");

    // 不带 with_today → 不合成（默认诚实：日线是什么就是什么）
    let v2: Value = http
        .get(format!("{base}/api/market/kline/sh600519"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v2["today_synthesized"], json!(false));
    assert_eq!(v2["kline"].as_array().unwrap().len(), 1);

    // 实时源挂 → 日线照常供给，today_synthesized=false（降级不牵连）
    fake.set_quote(Err(MarketError::Unavailable("down".into())));
    let v3: Value = http
        .get(format!(
            "{base}/api/market/kline/sh600519?with_today=true"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v3["today_synthesized"], json!(false));
    assert_eq!(v3["kline"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn cb_list_and_calendar_passthrough() {
    let fake = FakeApi::new();
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
    fake.set_cb(Ok(CbListPayload {
        as_of: "2026-10-28T15:40:01+08:00".into(),
        count: 1,
        spot_source: Some("sina_cb_spot".into()),
        redeem_source: Some("jsl_cb_redeem".into()),
        degraded: false,
        attempts: vec![],
        items: vec![item],
    }));
    let today = today();
    fake.set_calendar(Ok(CalendarPayload {
        start: "2026-04-30".into(),
        end: "2027-04-26".into(),
        count: 1,
        trade_dates: vec![today.clone()],
        source: Some("sina_calendar".into()),
        degraded: false,
        attempts: vec![],
    }));
    fake.set_health(Ok(SidecarHealth {
        ok: true,
        version: Some("0.1.0".into()),
        rest: Default::default(),
    }));
    let base = spawn_api(fake, seed()).await;
    let http = reqwest::Client::new();

    let v: Value = http
        .get(format!("{base}/api/market/cb_list"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["count"], json!(1));
    assert_eq!(v["items"][0]["code"], json!("128136"));
    assert_eq!(v["items"][0]["redeem_status"], json!("已公告强赎"));

    let v: Value = http
        .get(format!("{base}/api/market/calendar"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["trade_dates"][0], json!(today));
    assert_eq!(v["source"], json!("sina_calendar"));

    // status：sidecar 在线 + 日历可用 → 时钟不降级
    let s: Value = http
        .get(format!("{base}/api/market/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s["sidecar"]["online"], json!(true));
    assert_eq!(s["sidecar"]["version"], json!("0.1.0"));
    assert_eq!(s["calendar"]["available"], json!(true));
    assert_eq!(s["calendar"]["source"], json!("sina_calendar"));
    assert_eq!(s["clock"]["calendar_degraded"], json!(false));
    assert_eq!(s["clock"]["calendar_source"], json!("sina_calendar"));
}
