//! 契约测试（P4-T5，验收「样例响应过 schema 校验」）：
//!
//! 1. `docs/api/openapi.json` 与代码生成的 `openapi_json()` 逐字节一致——
//!    契约文档永不手改，改结构体/标注后必须再生成；
//! 2. seeded 测试服务全端点的**实时响应**过其声明的响应 schema（jsonschema，
//!    draft 2020-12，$ref 解析自 components.schemas）；
//! 3. `fixtures/api/` 的**提交样例**同样过 schema 校验（样例永不腐烂，
//!    前端移植时可直接当 mock 数据用）。
//!
//! 工件再生成：
//! ```powershell
//! cargo test -p zimu-server --lib -- --ignored regenerate_openapi_json   # 契约文档
//! $env:ZIMU_REGEN_API_SAMPLES='1'; cargo test -p zimu-server --test contract  # 响应样例
//! ```
//!
//! 全部离线：FakeApi 注入行情（tests/common），真实 HTTP 打本机回环临时端口。

mod common;

use std::path::{Path, PathBuf};

use common::*;
use serde_json::{json, Value};
use zimu_market::error::MarketError;
use zimu_market::models::*;
use zimu_store::{EquitySnapshotRow, NewAlert};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn samples_dir() -> PathBuf {
    repo_root().join("fixtures/api")
}

fn regen_samples() -> bool {
    std::env::var("ZIMU_REGEN_API_SAMPLES").is_ok()
}

fn load_doc() -> Value {
    let p = repo_root().join("docs/api/openapi.json");
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!(
            "读取 {} 失败：{e}；先跑 cargo test -p zimu-server --lib -- --ignored regenerate_openapi_json",
            p.display()
        )
    });
    serde_json::from_str(&raw).expect("openapi.json 是合法 JSON")
}

// ============================================================================
// schema 校验装置（jsonschema，draft 2020-12）
// ============================================================================

/// JSON Pointer 段转义（路径段里有 `/` 与 `{}`）
fn esc(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// `#/components/schemas/X` → `#/$defs/X`：jsonschema crate 按根文档解析内部
/// $ref，把 OpenAPI 的 components.schemas 挂成 $defs 即可全文档解析。
fn rewrite_refs(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                if let Some(name) = r.strip_prefix("#/components/schemas/") {
                    map.insert("$ref".into(), Value::String(format!("#/$defs/{name}")));
                }
            }
            for val in map.values_mut() {
                rewrite_refs(val);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                rewrite_refs(val);
            }
        }
        _ => {}
    }
}

/// 以「schema + 全文档 components.schemas 作 $defs」建校验器
fn schema_validator(doc: &Value, mut schema: Value) -> jsonschema::Validator {
    let mut defs = doc
        .pointer("/components/schemas")
        .expect("契约文档必须有 components.schemas")
        .clone();
    rewrite_refs(&mut defs);
    rewrite_refs(&mut schema);
    let obj = schema.as_object_mut().expect("响应 schema 是对象");
    obj.insert("$defs".into(), defs);
    obj.insert(
        "$schema".into(),
        json!("https://json-schema.org/draft/2020-12/schema"),
    );
    jsonschema::validator_for(&schema).expect("schema 可编译")
}

/// 取文档里某端点某状态码声明的 application/json 响应 schema 建校验器
fn response_validator(doc: &Value, method: &str, path: &str, status: u16) -> jsonschema::Validator {
    let ptr = format!(
        "/paths/{}/{method}/responses/{status}/content/application~1json/schema",
        esc(path)
    );
    let schema = doc
        .pointer(&ptr)
        .unwrap_or_else(|| panic!("契约文档缺响应 schema：{ptr}"))
        .clone();
    schema_validator(doc, schema)
}

fn validate(label: &str, v: &jsonschema::Validator, body: &Value) {
    let errors: Vec<String> = v.iter_errors(body).map(|e| e.to_string()).collect();
    assert!(
        errors.is_empty(),
        "{label} 未过 schema 校验（{} 处）：\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// 实时响应过 schema；样例同步：regen 模式写 `fixtures/api/<name>.json`，
/// 常规模式校验已提交样例（样例与实时响应过同一个校验器）。
fn check(doc: &Value, name: &str, method: &str, path: &str, status: u16, body: &Value) {
    let v = response_validator(doc, method, path, status);
    validate(&format!("实时响应 {name}"), &v, body);
    let file = samples_dir().join(format!("{name}.json"));
    if regen_samples() {
        std::fs::create_dir_all(samples_dir()).expect("建 fixtures/api 目录");
        let pretty = serde_json::to_string_pretty(body).expect("响应可序列化") + "\n";
        std::fs::write(&file, pretty).expect("写样例文件");
    } else {
        let raw = std::fs::read_to_string(&file).unwrap_or_else(|e| {
            panic!(
                "样例 {} 缺失：{e}；先跑 ZIMU_REGEN_API_SAMPLES=1 cargo test -p zimu-server --test contract",
                file.display()
            )
        });
        let sample: Value = serde_json::from_str(&raw).expect("样例是合法 JSON");
        validate(&format!("样例 {name}"), &v, &sample);
    }
}

async fn get(http: &reqwest::Client, base: &str, path: &str) -> (u16, Value) {
    let r = http.get(format!("{base}{path}")).send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json::<Value>().await.unwrap())
}

async fn post(http: &reqwest::Client, base: &str, path: &str, body: Value) -> (u16, Value) {
    let r = http.post(format!("{base}{path}")).json(&body).send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json::<Value>().await.unwrap())
}

// ============================================================================
// 1. 契约文档新鲜度
// ============================================================================

#[test]
fn checked_in_openapi_json_is_up_to_date() {
    let expected = zimu_server::openapi::openapi_json();
    let p = repo_root().join("docs/api/openapi.json");
    let actual = std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!("读取 {} 失败：{e}", p.display())
    });
    assert_eq!(
        actual, expected,
        "docs/api/openapi.json 与代码不一致：改契约后跑 \
         cargo test -p zimu-server --lib -- --ignored regenerate_openapi_json"
    );
}

// ============================================================================
// 2+3. 全端点实时响应 + 提交样例过 schema
// ============================================================================

#[tokio::test]
async fn live_responses_and_samples_pass_schema() {
    let doc = load_doc();
    let fake = FakeApi::new();
    let today = today();
    fake.set_quote(Ok(qpayload(vec![
        qrow("sh600519", "stock", Some(1437.0), Some(1421.0), QuoteStatus::Ok, Some(&today)),
        qrow("sh510300", "etf", Some(4.1), Some(4.05), QuoteStatus::Ok, Some(&today)),
        qrow("sz128136", "cb", None, None, QuoteStatus::NoData, None),
    ])));
    fake.set_kline(Ok(kline_payload("sh600519", "stock", 1421.0, &today)));
    fake.set_cb(Ok(cb_list_payload()));
    fake.set_calendar(Ok(calendar_payload(&today)));
    fake.set_health(Ok(SidecarHealth {
        ok: true,
        version: Some("0.1.0".into()),
        rest: Default::default(),
    }));
    // alerts 面样例种子（P4-T7）：1 条待处理预警 + 1 行当日快照。
    // 快照数字 = 种子组合 recalc agg 原值（样例可直接当前端 mock）
    let store = seed();
    store
        .insert_alert(&NewAlert {
            ts: format!("{today} 10:30"),
            code: "600519".into(),
            name: "贵州茅台".into(),
            alert_type: "跌破成本".into(),
            level: "down".into(),
            text: "现价 1437.00 低于成本 1500.39，浮亏 -4.22%".into(),
        })
        .unwrap();
    let agg = store.portfolio().unwrap().agg;
    store
        .upsert_equity_snapshot(&EquitySnapshotRow::from_agg(
            &today,
            &format!("{today} 15:05"),
            &agg,
        ))
        .unwrap();
    let base = spawn_api(fake.clone(), store).await;
    let http = reqwest::Client::new();

    // ---- holdings 面（P4-T1）----
    let (s, b) = get(&http, &base, "/api/holdings").await;
    assert_eq!(s, 200);
    check(&doc, "holdings", "get", "/api/holdings", 200, &b);

    // 空 patch = 读当前账户（缺失字段保持原值语义）
    let (s, b) = post(&http, &base, "/api/account", json!({})).await;
    assert_eq!(s, 200);
    check(&doc, "account", "post", "/api/account", 200, &b);

    // ---- market 面（P4-T4）：refresh 回写后 600519 有 prev=1421 / limit_pct=10 ----
    let (s, b) = post(&http, &base, "/api/market/refresh", json!(null)).await;
    assert_eq!(s, 200);
    check(&doc, "refresh", "post", "/api/market/refresh", 200, &b);

    // 422：委托价 1600 > limit_up 1563.10（1421×1.1，03 §2 整数单元 half-up）
    let (s, b) = post(
        &http,
        &base,
        "/api/holdings/trade",
        json!({"code": "600519", "side": "buy", "qty": 100, "price": 1600.0}),
    )
    .await;
    assert_eq!(s, 422);
    assert_eq!(b["error"], json!("out_of_limit_range"));
    check(&doc, "error_out_of_limit", "post", "/api/holdings/trade", 422, &b);

    // 404：卖出未持有标的
    let (s, b) = post(
        &http,
        &base,
        "/api/holdings/trade",
        json!({"code": "600000", "side": "sell", "qty": 100, "price": 10.0}),
    )
    .await;
    assert_eq!(s, 404);
    check(&doc, "error_no_holding", "post", "/api/holdings/trade", 404, &b);

    // 201：加仓已持有 ETF（无 limit_pct 配置 → 跳过区间拦截）
    let (s, b) = post(
        &http,
        &base,
        "/api/holdings/trade",
        json!({"code": "510300", "side": "buy", "qty": 1000, "price": 4.1, "note": "契约样例：加仓"}),
    )
    .await;
    assert_eq!(s, 201);
    check(&doc, "trade_receipt", "post", "/api/holdings/trade", 201, &b);

    let (s, b) = get(&http, &base, "/api/trades?limit=50").await;
    assert_eq!(s, 200);
    check(&doc, "trades", "get", "/api/trades", 200, &b);

    let (s, b) = get(&http, &base, "/api/market/status").await;
    assert_eq!(s, 200);
    check(&doc, "market_status", "get", "/api/market/status", 200, &b);

    let (s, b) = get(&http, &base, "/api/market/quote?codes=sh600519,sh510300,sz128136").await;
    assert_eq!(s, 200);
    check(&doc, "quote", "get", "/api/market/quote", 200, &b);

    // with_today=true：日线尾部=昨天 + 实时行 ok → 合成当日 bar（04 附录 B）
    let (s, b) = get(&http, &base, "/api/market/kline/sh600519?with_today=true").await;
    assert_eq!(s, 200);
    assert_eq!(b["today_synthesized"], json!(true));
    check(&doc, "kline", "get", "/api/market/kline/{symbol}", 200, &b);

    let (s, b) = get(&http, &base, "/api/market/cb_list").await;
    assert_eq!(s, 200);
    check(&doc, "cb_list", "get", "/api/market/cb_list", 200, &b);

    let (s, b) = get(&http, &base, "/api/market/calendar").await;
    assert_eq!(s, 200);
    check(&doc, "calendar", "get", "/api/market/calendar", 200, &b);

    // ---- alerts 面（P4-T7）：读取 / 归档 / 净值快照 ----
    let (s, b) = get(&http, &base, "/api/alerts?limit=50").await;
    assert_eq!(s, 200);
    check(&doc, "alerts", "get", "/api/alerts", 200, &b);

    let (s, b) = post(&http, &base, "/api/alerts/1/done", json!(null)).await;
    assert_eq!(s, 200);
    assert_eq!(b["done"], json!(true));
    check(&doc, "alert_done", "post", "/api/alerts/{id}/done", 200, &b);

    let (s, b) = get(&http, &base, "/api/equity_snapshots?limit=120").await;
    assert_eq!(s, 200);
    check(&doc, "equity_snapshots", "get", "/api/equity_snapshots", 200, &b);

    // ---- 错误契约 ----
    // 归档不存在的预警 → 404 no_alert
    let (s, b) = post(&http, &base, "/api/alerts/999/done", json!(null)).await;
    assert_eq!(s, 404);
    assert_eq!(b["error"], json!("no_alert"));
    check(&doc, "error_no_alert", "post", "/api/alerts/{id}/done", 404, &b);

    let (s, b) = get(&http, &base, "/api/market/quote?codes=").await;
    assert_eq!(s, 400);
    assert_eq!(b["error"], json!("bad_params"));
    check(&doc, "error_bad_params", "get", "/api/market/quote", 400, &b);

    // sidecar 挂了：quote 503 / refresh 503（降级信号，04 附录 B 三分类）
    fake.set_quote(Err(MarketError::Unavailable("契约测试：模拟 sidecar 停止".into())));
    let (s, b) = get(&http, &base, "/api/market/quote?codes=sh600519").await;
    assert_eq!(s, 503);
    assert_eq!(b["error"], json!("market_unavailable"));
    check(&doc, "error_market_unavailable", "get", "/api/market/quote", 503, &b);
    let (s, b) = post(&http, &base, "/api/market/refresh", json!(null)).await;
    assert_eq!(s, 503);
    validate(
        "实时响应 refresh 503",
        &response_validator(&doc, "post", "/api/market/refresh", 503),
        &b,
    );

    // 契约文档自身也可在线取（与仓库文件同内容）
    let (s, b) = get(&http, &base, "/api/openapi.json").await;
    assert_eq!(s, 200);
    assert_eq!(b, doc, "运行时 /api/openapi.json 与仓库文件同源");
}

/// health 实现在 bin（main.rs），lib 测试服务器不含该路由——样例手写提交，
/// 这里过 Health schema；bin 侧另有键集一致单测（main.rs tests）守漂移。
#[test]
fn committed_health_sample_passes_schema() {
    let doc = load_doc();
    let v = schema_validator(&doc, json!({ "$ref": "#/components/schemas/Health" }));
    let raw = std::fs::read_to_string(samples_dir().join("health.json"))
        .expect("fixtures/api/health.json 必须提交在仓库");
    let sample: Value = serde_json::from_str(&raw).expect("样例是合法 JSON");
    validate("样例 health", &v, &sample);
}
