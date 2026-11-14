//! chat_upload 集成测试（ADR-0015）：Web 上传 multipart 接收 + 落盘 + 注入钩子。
//! 全离线：临时 media 目录 + counting hook（不依赖 AgentBridge/store），真实 HTTP
//! 打本机回环临时端口（reqwest multipart 客户端）。消毒/防覆盖/超限的纯函数单测
//! 在 chat_upload.rs 内联；这里验端到端 HTTP 语义与 hook 收到的附件形状。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use robit_agent::MediaAttachment;
use zimu_server::chat_upload::{UploadHook, UploadState};

/// hook 收到的调用记录：(text, attachments)
type Calls = Arc<Mutex<Vec<(String, Vec<MediaAttachment>)>>>;

fn tmp_media(tag: &str) -> PathBuf {
    // 不预建——receive_upload 惰性 create_dir_all（顺带验证建目录）
    std::env::temp_dir().join(format!("zimu-upload-it-{tag}-{}", uuid::Uuid::new_v4()))
}

fn ok_hook(calls: Calls) -> UploadHook {
    Arc::new(move |text, atts| {
        let c = calls.clone();
        Box::pin(async move {
            c.lock().unwrap().push((text, atts));
            Ok(())
        })
    })
}

fn err_hook() -> UploadHook {
    Arc::new(|_text, _atts| Box::pin(async { Err(anyhow::anyhow!("agent 离线")) }))
}

async fn serve(media_dir: PathBuf, max_bytes: u64, max_files: usize, hook: UploadHook) -> String {
    let state = UploadState { media_dir, max_bytes, max_files, hook };
    let router = zimu_server::chat_upload::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn file_part(bytes: Vec<u8>, name: &str, mime: &str) -> reqwest::multipart::Part {
    reqwest::multipart::Part::bytes(bytes)
        .file_name(name.to_string())
        .mime_str(mime)
        .unwrap()
}

#[tokio::test]
async fn text_only_accepted_and_hook_gets_empty_attachments() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let base = serve(tmp_media("textonly"), 20 * 1024 * 1024, 10, ok_hook(calls.clone())).await;
    let form = reqwest::multipart::Form::new().text("text", "今天该做什么");
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 202);
    let c = calls.lock().unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, "今天该做什么");
    assert!(c[0].1.is_empty());
}

#[tokio::test]
async fn file_saved_and_hook_gets_local_path_attachment() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let dir = tmp_media("file");
    let base = serve(dir.clone(), 20 * 1024 * 1024, 10, ok_hook(calls.clone())).await;
    let body = "code,qty\n600519,100\n".as_bytes().to_vec();
    let form = reqwest::multipart::Form::new()
        .text("text", "解析这个")
        .part("files", file_part(body.clone(), "positions.csv", "text/csv"));
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 202);
    // 落盘真实存在、内容一致
    let saved = dir.join("positions.csv");
    assert!(saved.exists(), "文件应落盘 media/positions.csv");
    assert_eq!(std::fs::read(&saved).unwrap(), body);
    // hook 收到的附件形状
    let c = calls.lock().unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, "解析这个");
    assert_eq!(c[0].1.len(), 1);
    let att = &c[0].1[0];
    assert_eq!(att.filename.as_deref(), Some("positions.csv"));
    assert_eq!(att.content_type, "text/csv");
    assert_eq!(att.size, Some(body.len() as u64));
    let lp = att.local_path.as_ref().expect("local_path 必填");
    assert!(lp.starts_with(&dir), "local_path 应在 media 目录内");
    assert!(att.url.is_empty(), "上传无远程 URL");
}

#[tokio::test]
async fn empty_text_with_file_allowed() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let base = serve(tmp_media("emptytext"), 20 * 1024 * 1024, 10, ok_hook(calls.clone())).await;
    let form = reqwest::multipart::Form::new()
        .text("text", "")
        .part("files", file_part(b"hello".to_vec(), "note.txt", "text/plain"));
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 202);
    assert_eq!(calls.lock().unwrap()[0].1.len(), 1);
}

#[tokio::test]
async fn empty_text_and_no_file_rejected_400() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let base = serve(tmp_media("emptyboth"), 20 * 1024 * 1024, 10, ok_hook(calls.clone())).await;
    let form = reqwest::multipart::Form::new().text("text", "   ");
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 400);
    assert!(calls.lock().unwrap().is_empty(), "拒绝时不应触达 hook");
}

#[tokio::test]
async fn oversized_file_rejected_413_and_half_removed() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let dir = tmp_media("toolarge");
    let base = serve(dir.clone(), 5, 10, ok_hook(calls.clone())).await; // 上限 5 字节
    let form = reqwest::multipart::Form::new()
        .text("text", "x")
        .part("files", file_part(vec![b'a'; 100], "big.csv", "text/csv"));
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 413);
    assert!(calls.lock().unwrap().is_empty());
    // 半截文件已删（目录存在但空）
    let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert!(entries.is_empty(), "超限不应留半截文件");
}

#[tokio::test]
async fn traversal_filename_sanitized_into_media_dir() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let dir = tmp_media("traversal");
    let base = serve(dir.clone(), 20 * 1024 * 1024, 10, ok_hook(calls.clone())).await;
    let form = reqwest::multipart::Form::new()
        .text("text", "x")
        .part("files", file_part(b"pwned".to_vec(), "../../evil.txt", "text/plain"));
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 202);
    assert!(dir.join("evil.txt").exists(), "消毒后应落在 media/evil.txt");
    let att = &calls.lock().unwrap()[0].1[0];
    assert_eq!(att.filename.as_deref(), Some("evil.txt"));
    assert!(att.local_path.as_ref().unwrap().starts_with(&dir), "不得逃出 media 目录");
}

#[tokio::test]
async fn same_filename_gets_numeric_suffix_no_overwrite() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let dir = tmp_media("collision");
    let base = serve(dir.clone(), 20 * 1024 * 1024, 10, ok_hook(calls.clone())).await;
    let http = reqwest::Client::new();
    for _ in 0..2 {
        let form = reqwest::multipart::Form::new()
            .text("text", "x")
            .part("files", file_part(b"data".to_vec(), "dup.csv", "text/csv"));
        let r = http.post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
        assert_eq!(r.status(), 202);
    }
    assert!(dir.join("dup.csv").exists());
    assert!(dir.join("dup-1.csv").exists(), "同名应加 -1 后缀而非覆盖");
    let c = calls.lock().unwrap();
    // filename 保留原始消毒名（QQ 同款标签），local_path 是防覆盖后的真实落盘名
    assert_eq!(c[1].1[0].filename.as_deref(), Some("dup.csv"));
    assert!(c[1].1[0].local_path.as_ref().unwrap().ends_with("dup-1.csv"));
}

#[tokio::test]
async fn too_many_files_rejected_400() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let base = serve(tmp_media("toomany"), 20 * 1024 * 1024, 1, ok_hook(calls.clone())).await; // 上限 1 个
    let form = reqwest::multipart::Form::new()
        .text("text", "x")
        .part("files", file_part(b"a".to_vec(), "a.txt", "text/plain"))
        .part("files", file_part(b"b".to_vec(), "b.txt", "text/plain"));
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 400);
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn empty_file_rejected_400() {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let base = serve(tmp_media("emptyfile"), 20 * 1024 * 1024, 10, ok_hook(calls.clone())).await;
    let form = reqwest::multipart::Form::new()
        .text("text", "x")
        .part("files", file_part(vec![], "empty.txt", "text/plain"));
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 400);
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn hook_error_maps_to_503() {
    let dir = tmp_media("hookerr");
    let base = serve(dir, 20 * 1024 * 1024, 10, err_hook()).await;
    let form = reqwest::multipart::Form::new()
        .text("text", "x")
        .part("files", file_part(b"data".to_vec(), "a.csv", "text/csv"));
    let r = reqwest::Client::new().post(format!("{base}/api/chat")).multipart(form).send().await.unwrap();
    assert_eq!(r.status(), 503);
}
