//! chat_api 集成测试（T3/ADR-0011）：Web 会话档案的读语义与重置三态。
//! 全离线：FakeApi 走 common 装置；档案行直接经 store 种子（recorder 链路自身的
//! 单测在 chat_record.rs 内联，端到端 SSE→档案 属于人工走查层）。

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::FakeApi;
use serde_json::{json, Value};
use zimu_server::chat_api::ResetHook;
use zimu_server::holdings_api::SharedStore;
use zimu_server::{api_router, market_api};
use zimu_store::Store;

fn tmp_store(tag: &str) -> Store {
    let dir = std::env::temp_dir().join(format!("zimu-chatapi-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("chat.db");
    Store::open(path.to_str().expect("临时路径是合法 UTF-8")).expect("临时档案库可开")
}

async fn serve(store: Store, reset: ResetHook) -> (String, SharedStore) {
    let shared: SharedStore = Arc::new(Mutex::new(store));
    let router = api_router(
        shared.clone(),
        market_api::new_market(Arc::new(FakeApi::new())),
        reset,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{addr}"), shared)
}

/// 记录调用次数的定值钩子（true=agent 在线假象 / false=离线）
fn counting_hook(val: bool, counter: Arc<AtomicUsize>) -> ResetHook {
    Arc::new(move || {
        let c = counter.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            val
        })
    })
}

async fn get_json(http: &reqwest::Client, url: &str) -> Value {
    let r = http.get(url).send().await.unwrap();
    assert_eq!(r.status(), 200);
    r.json::<Value>().await.unwrap()
}

#[tokio::test]
async fn history_returns_ascending_and_pages_with_before_id() {
    let s = tmp_store("hist");
    s.append_chat_message("web", "user", "第一问", None, "2026-11-07 09:00").unwrap();
    s.append_chat_message("web", "tool", "", Some(r#"{"name":"echo"}"#), "2026-11-07 09:00")
        .unwrap();
    s.append_chat_message("web", "assistant", "第一答", None, "2026-11-07 09:01").unwrap();
    s.append_chat_message("web", "user", "第二问", None, "2026-11-07 09:02").unwrap();
    // 其他会话的行不得混入（通道独立词汇表：'web' 才是网页档案）
    s.append_chat_message("qq", "user", "串台", None, "2026-11-07 09:03").unwrap();

    let (base, _shared) = serve(s, common::no_reset_hook()).await;
    let http = reqwest::Client::new();

    // 全量：时间升序（旧→新），tool 行在内、别的会话不在内
    let full = get_json(&http, &format!("{base}/api/chat/history")).await;
    let msgs = full["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 4, "只回 'web' 会话的行");
    assert_eq!(msgs[0]["text"], json!("第一问"));
    assert_eq!(msgs[1]["role"], json!("tool"));
    assert_eq!(msgs[3]["text"], json!("第二问"));
    assert_eq!(full["has_more"], json!(false));

    // 最近 2 条：升序返回 + has_more 报"还有更早"
    let page = get_json(&http, &format!("{base}/api/chat/history?limit=2")).await;
    let pm = page["messages"].as_array().unwrap();
    assert_eq!(pm.len(), 2);
    assert_eq!(pm[0]["text"], json!("第一答"));
    assert_eq!(page["has_more"], json!(true));

    // 游标向更早翻页：before_id = 本页最旧行
    let oldest = pm[0]["id"].as_i64().unwrap();
    let prev = get_json(
        &http,
        &format!("{base}/api/chat/history?limit=2&before_id={oldest}"),
    )
    .await;
    let pv = prev["messages"].as_array().unwrap();
    assert_eq!(pv.len(), 2, "第一问 + tool（升序）");
    assert_eq!(pv[0]["text"], json!("第一问"));
    assert_eq!(pv[1]["role"], json!("tool"));
    assert_eq!(prev["has_more"], json!(false), "已翻到页首");
}

#[tokio::test]
async fn reset_with_online_agent_clears_notifies_and_logs() {
    let s = tmp_store("reset-on");
    s.append_chat_message("web", "user", "要清掉", None, "2026-11-07 09:00").unwrap();
    s.append_chat_message("web", "assistant", "也要清掉", None, "2026-11-07 09:01").unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let (base, shared) = serve(s, counting_hook(true, hits.clone())).await;
    let http = reqwest::Client::new();

    let r = http
        .post(format!("{base}/api/chat/reset"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["cleared"], json!(2));
    assert_eq!(b["agent_signaled"], json!(true));
    assert_eq!(hits.load(Ordering::SeqCst), 1, "钩子恰被调一次");

    // 档案清空 + push_log 如实留痕（ok=agent_signaled）
    let full = get_json(&http, &format!("{base}/api/chat/history")).await;
    assert!(full["messages"].as_array().unwrap().is_empty());
    let logs = shared
        .lock()
        .unwrap()
        .push_logs(10)
        .unwrap()
        .into_iter()
        .find(|l| l.kind == "web_session_reset")
        .expect("重置必须留痕");
    assert!(logs.ok);
    assert_eq!(logs.channel, "agent");
    assert!(logs.detail.contains("2 条"));
}

#[tokio::test]
async fn reset_with_offline_agent_still_clears_and_logs_unok() {
    let s = tmp_store("reset-off");
    s.append_chat_message("web", "user", "离线也要清", None, "2026-11-07 09:00").unwrap();
    let (base, shared) = serve(s, common::no_reset_hook()).await;
    let http = reqwest::Client::new();

    let b: Value = http
        .post(format!("{base}/api/chat/reset"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(b["cleared"], json!(1));
    assert_eq!(b["agent_signaled"], json!(false), "离线如实报 false，不装成功");

    let logs = shared.lock().unwrap().push_logs(10).unwrap();
    let reset = logs
        .iter()
        .find(|l| l.kind == "web_session_reset")
        .expect("同样留痕");
    assert!(!reset.ok, "ok=false 记录 agent 未在线");
}
