//! 黄金测试向量 runner（P3-T3）。
//!
//! 读取仓库根 `fixtures/golden/*.json`，对 `zimu_domain` 的纯函数逐例断言。
//! 比对规则见 `docs/03-domain-rules.md` 附录：
//! - 数值：|实现 − 期望| ≤ tol（case 级 ?? 文件级）
//! - 布尔/字符串/枚举：精确相等（阈值压线的浮点噪声由实现内 THRESHOLD_EPS 消化）
//! - 数组：同长按序对齐；`null`：实现必须返回空；expect 未列字段不校验
//! - `status != "active"` 的组跳过（机制保留；price_limit 已随 P1-T2 定稿转 active，现全部 active）
//!
//! TS 共享层（P3-T4）跑同一份文件，规则一致。

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use zimu_domain::{
    advice, alert, bench, dilution, discipline, fees, lot, perf, price_limit, recalc, vol_synth,
};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("fixtures/golden")
}

fn compute(group: &str, case: &Value) -> Value {
    let fn_name = case["fn"].as_str().unwrap_or_default();
    let input = &case["input"];

    fn call<I: DeserializeOwned, O: Serialize>(input: &Value, f: impl FnOnce(I) -> O) -> Value {
        let inp: I = serde_json::from_value(input.clone()).expect("fixture input 与实现签名不符");
        serde_json::to_value(f(inp)).expect("输出序列化失败")
    }

    match (group, fn_name) {
        ("lot", "lot_size") => call(input, |i: lot::KindInput| lot::LotOutput {
            lot: lot::lot_size(i.kind),
        }),
        ("lot", "price_decimals") => call(input, |i: lot::KindInput| lot::DecimalsOutput {
            decimals: lot::price_decimals(i.kind),
        }),
        ("lot", "validate_qty") => call(input, |i: lot::ValidateQtyInput| {
            lot::validate_qty(i.kind, i.side, i.qty, i.held)
        }),
        ("lot", "quick_tiers") => call(input, |i: lot::QuickTiersInput| lot::TiersOutput {
            tiers: lot::quick_tiers(i.kind, i.side, i.held),
        }),
        ("fees", "fee") => call(input, fees::fee),
        ("dilution", "apply_trade") => call(input, |i: dilution::ApplyTradeInput| {
            dilution::apply_trade(&i)
        }),
        ("recalc", "recalc") => call(input, |i: recalc::PortfolioInput| recalc::recalc(&i)),
        ("discipline", "check") => call(input, |i: recalc::PortfolioInput| discipline::check(&i)),
        ("discipline", "sell_to") => call(input, |i: discipline::SellToInput| {
            discipline::sell_to(&i)
        }),
        ("perf", "perf") => call(input, |i: perf::PerfInput| perf::perf(&i)),
        ("bench", "align_bench") => call(input, |i: bench::AlignBenchInput| {
            bench::AlignBenchOutput { aligned: bench::align_bench(&i) }
        }),
        ("vol_synth", "vol_synth") => call(input, |i: vol_synth::VolSynthInput| {
            vol_synth::vol_synth(&i)
        }),
        ("price_limit", "limit_pct") => call(input, |i: price_limit::LimitPctInput| {
            price_limit::LimitPctOutput {
                pct: price_limit::limit_pct(i.kind, &i.code, i.st),
            }
        }),
        ("price_limit", "limit_prices") => call(input, |i: price_limit::LimitPricesInput| {
            price_limit::limit_prices(i.kind, i.prev, i.pct)
        }),
        ("price_limit", "validate_price") => call(input, |i: price_limit::ValidatePriceInput| {
            price_limit::validate_price(i.price, i.limit_up, i.limit_down)
        }),
        ("advice", "score_advice") => call(input, |i: advice::AdviceInput| {
            advice::score_advice(&i)
        }),
        ("alerts", "eval_alerts") => call(input, |i: alert::AlertEvalInput| {
            alert::eval_alerts(&i)
        }),
        (g, f) => panic!("未知 组/函数：{g}/{f}"),
    }
}

fn compare(path: &str, expected: &Value, computed: &Value, tol: f64, errs: &mut Vec<String>) {
    match (expected, computed) {
        (Value::Object(e), Value::Object(c)) => {
            for (k, ev) in e {
                match c.get(k) {
                    Some(cv) => compare(&format!("{path}.{k}"), ev, cv, tol, errs),
                    None => errs.push(format!("{path}.{k}: 实现输出缺少该字段（期望 {ev}）")),
                }
            }
        }
        (Value::Array(e), Value::Array(c)) => {
            if e.len() != c.len() {
                errs.push(format!(
                    "{path}: 数组长度不符 期望 {} 实际 {}",
                    e.len(),
                    c.len()
                ));
                return;
            }
            for (i, (ev, cv)) in e.iter().zip(c).enumerate() {
                compare(&format!("{path}[{i}]"), ev, cv, tol, errs);
            }
        }
        (Value::Number(e), Value::Number(c)) => {
            let (ev, cv) = (e.as_f64().unwrap(), c.as_f64().unwrap());
            if (ev - cv).abs() > tol {
                errs.push(format!(
                    "{path}: 期望 {ev} 实际 {cv}（差 {:.3e}，容差 {tol:e}）",
                    (ev - cv).abs()
                ));
            }
        }
        (Value::Null, Value::Null) => {}
        _ => {
            if expected != computed {
                errs.push(format!("{path}: 期望 {expected} 实际 {computed}"));
            }
        }
    }
}

#[test]
fn golden_vectors() {
    let dir = fixtures_dir();
    assert!(dir.is_dir(), "黄金向量目录不存在：{}", dir.display());

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("读取 fixtures/golden 失败")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "fixtures/golden 下没有 JSON 文件");

    let mut total = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for file in &files {
        let raw = fs::read_to_string(file).expect("读取 fixture 失败");
        let doc: Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{} 解析失败：{e}", file.display()));
        let group = doc["group"].as_str().expect("缺 group 字段");
        assert_eq!(
            Some(group),
            file.file_stem().and_then(|s| s.to_str()),
            "group 必须与文件名一致：{}",
            file.display()
        );

        if doc["status"].as_str() != Some("active") {
            skipped.push(format!(
                "{group}({})",
                doc["status"].as_str().unwrap_or("?")
            ));
            continue;
        }

        let file_tol = doc["tolerance"].as_f64().unwrap_or(1e-9);
        for case in doc["cases"].as_array().expect("缺 cases 数组") {
            total += 1;
            let id = case["id"].as_str().expect("case 缺 id");
            let tol = case["tolerance"].as_f64().unwrap_or(file_tol);
            let computed = compute(group, case);
            let mut errs = Vec::new();
            compare("expect", &case["expect"], &computed, tol, &mut errs);
            if !errs.is_empty() {
                failures.push(format!("[{group}/{id}]\n  {}", errs.join("\n  ")));
            }
        }
    }

    println!(
        "黄金向量：{total} 个用例全部比对；跳过 {} 组：{}",
        skipped.len(),
        if skipped.is_empty() {
            "无".to_string()
        } else {
            skipped.join(", ")
        }
    );
    assert!(
        failures.is_empty(),
        "黄金向量失败 {} 例：\n{}",
        failures.len(),
        failures.join("\n")
    );
}
