//! Web 对话框文件上传（ADR-0015）：`POST /api/chat` 的 multipart 接收 + 落盘 + 注入。
//!
//! 链路（与 QQ 接收语义同构，robit 三 crate 零改动）：
//! ChatPanel `FormData(text + files)` → 本模块流式落盘 `media/`（消毒 + 防覆盖 +
//! `max_media_bytes` 守卫）→ 构造 `MediaAttachment{local_path}` → `UploadHook`
//! （bin 接 `AgentBridge::submit_user_input`：档案存标注版 text、模型收原文 +
//! attachments）→ `WebBridge::send_message(UserInput)` → Agent 自动分流（图像
//! 本地优先 base64 vision；非图像追加路径标注行，标准工具 read/bash 自动解析）。
//!
//! 落盘卫生规则**复刻** robit-agent `media.rs` 的私有 fn（`sanitize_filename` /
//! `extension_from_content_type` / `resolve_collision`，:125/:149/:174）——公开导出
//! 只有 download/encode 系，宿主上传无 URL 可下载，只能自写同等规则（R12 已诉求
//! 上游公开这些助手）。限额复用 robit 官方键 `[app.context] max_media_bytes`
//! （缺省 20MB，0=不限），不自创键。
//!
//! 可测结构：`UploadHook` 照 `chat_api::ResetHook` 注入范式——lib 不依赖 bin 的
//! AgentBridge，集成测试注入 counting fake + 临时 media 目录即可打真实 multipart HTTP。
//!
//! 信任边界（ADR-0015 明示）：端点无鉴权（与本服务一致，监听 localhost），只向
//! `media/` 写消毒后文件、**上传本身不执行任何东西**；解析由 Agent 按 ADR-0014
//! 人格纪律自动执行。文件名消毒防路径穿越；MIME 信任客户端（robit 同款）。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::multipart::Field;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use robit_agent::MediaAttachment;
use tokio::io::AsyncWriteExt;

/// 单条消息附件数量上限（宿主政策，ADR-0015；文档注明，非 robit 键）。
pub const DEFAULT_MAX_FILES: usize = 10;

/// 上传注入钩子（形状照 `chat_api::ResetHook`）：bin 接
/// `AgentBridge::submit_user_input`，lib 测试注入 fake。
/// `Err` → 503（agent 离线 / 档案写入失败）；落盘已完成，附件路径在档案标注里留存。
pub type UploadHook = Arc<
    dyn Fn(String, Vec<MediaAttachment>) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct UploadState {
    /// 落盘目录（= working_dir/media，与 QQ 同根；惰性创建）
    pub media_dir: PathBuf,
    /// 单文件字节上限（robit `[app.context] max_media_bytes`，0=不限）
    pub max_bytes: u64,
    /// 单条消息附件数量上限
    pub max_files: usize,
    pub hook: UploadHook,
}

/// `POST /api/chat` 路由（bin  merge 进主路由；自带独立状态，与 AppState 解耦）。
pub fn router(state: UploadState) -> Router {
    Router::new()
        .route("/api/chat", post(handle_chat))
        .with_state(state)
}

async fn handle_chat(State(st): State<UploadState>, multipart: Multipart) -> Response {
    let (text, attachments) = match receive_upload(multipart, &st).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };
    // 空 text 允许（仅发附件）；两者皆空才拒（放宽旧纯文本契约的空串 400）
    if text.trim().is_empty() && attachments.is_empty() {
        return (StatusCode::BAD_REQUEST, "消息为空（无文本且无附件）").into_response();
    }
    match (st.hook)(text, attachments).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => {
            tracing::warn!("/api/chat 受理失败：{e:#}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "会话受理失败：agent 已退出或档案写入失败（详情见服务日志）",
            )
                .into_response()
        }
    }
}

/// 解析 multipart：`text` 字段取文本，`files` 字段逐个流式落盘。未知字段忽略
/// （前进兼容）。Multipart 流式提取不受 axum DefaultBodyLimit（2MB）约束，
/// 大小上限由 `save_field` 自守卫（复刻 robit media.rs:79-98）。
async fn receive_upload(
    mut multipart: Multipart,
    st: &UploadState,
) -> Result<(String, Vec<MediaAttachment>), UploadError> {
    tokio::fs::create_dir_all(&st.media_dir)
        .await
        .map_err(UploadError::Io)?;
    let mut text = String::new();
    let mut attachments: Vec<MediaAttachment> = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| UploadError::BadForm(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "text" {
            let t = field
                .text()
                .await
                .map_err(|e| UploadError::BadForm(e.to_string()))?;
            if text.is_empty() {
                text = t;
            }
        } else if name == "files" {
            if attachments.len() >= st.max_files {
                return Err(UploadError::TooManyFiles(st.max_files));
            }
            let filename = field.file_name().map(|s| s.to_string());
            let content_type = field
                .content_type()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());
            let saved = save_field(
                field,
                &st.media_dir,
                &content_type,
                filename.as_deref(),
                st.max_bytes,
            )
            .await?;
            attachments.push(MediaAttachment {
                content_type,
                // 上传无远程 URL：local_path 优先（Agent 编码本地图像直接用本地副本）
                url: String::new(),
                filename: Some(saved.filename),
                size: Some(saved.size),
                width: None,
                height: None,
                local_path: Some(saved.local_path),
            });
        }
    }
    Ok((text, attachments))
}

struct Saved {
    filename: String,
    size: u64,
    local_path: PathBuf,
}

/// 流式落盘单个文件字段：消毒文件名 → 防覆盖 → 边读边写边累计，超限删半截。
/// 逐字复刻 robit-agent media.rs:62-105 的守卫姿态（Content-Length 预检在
/// multipart 场景不适用——字段无独立长度头，靠流式累计单保险）。
async fn save_field(
    mut field: Field<'_>,
    media_dir: &Path,
    content_type: &str,
    filename: Option<&str>,
    max_bytes: u64,
) -> Result<Saved, UploadError> {
    let save_filename = sanitize_filename(filename, Some(content_type));
    let save_path = resolve_collision(media_dir, &save_filename).await;

    let mut file = tokio::fs::File::create(&save_path)
        .await
        .map_err(UploadError::Io)?;
    let mut total: u64 = 0;
    let result: Result<(), UploadError> = async {
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| UploadError::BadForm(e.to_string()))?
        {
            total += chunk.len() as u64;
            if max_bytes > 0 && total > max_bytes {
                return Err(UploadError::TooLarge(max_bytes));
            }
            file.write_all(&chunk).await.map_err(UploadError::Io)?;
        }
        file.flush().await.map_err(UploadError::Io)?;
        Ok(())
    }
    .await;

    if let Err(e) = result {
        drop(file);
        let _ = tokio::fs::remove_file(&save_path).await;
        return Err(e);
    }
    if total == 0 {
        let _ = tokio::fs::remove_file(&save_path).await;
        return Err(UploadError::Empty);
    }
    Ok(Saved {
        // filename = 消毒后的原始名（标注标签，QQ 同款）；local_path = 防覆盖后的
        // 真实落盘名（可能带 -1/-2 后缀）——模型按 local_path 读，两者各司其职。
        filename: save_filename,
        size: total,
        local_path: save_path,
    })
}

enum UploadError {
    TooLarge(u64),
    TooManyFiles(usize),
    Empty,
    BadForm(String),
    Io(std::io::Error),
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            UploadError::TooLarge(limit) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("文件超过大小上限（{} MB）", limit / 1024 / 1024),
            ),
            UploadError::TooManyFiles(n) => (
                StatusCode::BAD_REQUEST,
                format!("附件数量超过上限（{n} 个）"),
            ),
            UploadError::Empty => (StatusCode::BAD_REQUEST, "附件为空文件".to_string()),
            UploadError::BadForm(e) => (
                StatusCode::BAD_REQUEST,
                format!("上传表单解析失败：{e}"),
            ),
            UploadError::Io(e) => {
                tracing::error!("上传落盘 IO 失败：{e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "文件落盘失败（详情见服务日志）".to_string(),
                )
            }
        };
        (status, msg).into_response()
    }
}

// ============================================================================
// 落盘卫生规则（复刻 robit-agent media.rs 私有 fn，R12 诉求上游公开）
// ============================================================================

/// 文件名消毒（复刻 robit-agent media.rs:125-146，一处有意硬化）：
/// 只取 `file_name()` 分量（剥目录与 `..`）→ 滤控制字符及残留 `/ \ :` →
/// trim 空白与首尾点 → 空结果 UUID v4 → 无扩展名按 content_type 补。
/// **跨平台硬化（2026-11-17，首次 Linux CI 暴露）**：取分量前先把反斜杠归一为
/// 正斜杠——`Path::file_name()` 平台相关（Linux 不认 `\` 分隔符，Windows 客户端
/// 上传的 `a/b\c:d.txt` 在 Linux 会残留 `b` 成分），归一后同一上传跨平台落盘同名，
/// Windows 风格穿越在 Linux 服务端也被完整剥除（纵深防御）。上游 robit 原版无此
/// 归一（IM 附件走 robit 自己的消毒）——有意分歧已记 docs/07 R12③ 移交。
fn sanitize_filename(filename: Option<&str>, content_type: Option<&str>) -> String {
    let normalized = filename.map(|n| n.replace('\\', "/")).unwrap_or_default();
    let component = Path::new(&normalized)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut cleaned: String = component
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\' && *c != ':')
        .collect();
    cleaned = cleaned.trim().trim_matches('.').trim().to_string();

    if cleaned.is_empty() {
        cleaned = uuid::Uuid::new_v4().to_string();
    }
    if Path::new(&cleaned).extension().is_none() {
        if let Some(ext) = extension_from_content_type(content_type) {
            cleaned.push('.');
            cleaned.push_str(ext);
        }
    }
    cleaned
}

/// MIME → 扩展名映射（复刻 robit-agent media.rs:149-170，无名/无扩展名文件用）。
fn extension_from_content_type(content_type: Option<&str>) -> Option<&'static str> {
    let ct = content_type?.split(';').next()?.trim().to_ascii_lowercase();
    let ext = match ct.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "text/markdown" => "md",
        "application/json" => "json",
        "application/zip" => "zip",
        "application/msword" => "doc",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        _ => return None,
    };
    Some(ext)
}

/// 防覆盖（复刻 robit-agent media.rs:174-200）：`name.ext → name-1.ext → …`
/// 试到 100 次仍冲突 → UUID v4 名保留扩展名。**绝不静默覆盖**。
async fn resolve_collision(save_dir: &Path, filename: &str) -> PathBuf {
    let candidate = save_dir.join(filename);
    if !tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned());
    for i in 1..=100u32 {
        let name = match &ext {
            Some(e) => format!("{}-{}.{}", stem, i, e),
            None => format!("{}-{}", stem, i),
        };
        let p = save_dir.join(name);
        if !tokio::fs::try_exists(&p).await.unwrap_or(false) {
            return p;
        }
    }
    let name = match &ext {
        Some(e) => format!("{}.{}", uuid::Uuid::new_v4(), e),
        None => uuid::Uuid::new_v4().to_string(),
    };
    save_dir.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立临时目录（uuid 命名，避免并行测试互撞）。
    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zimu-upload-{}-{}", tag, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    #[test]
    fn sanitize_strips_traversal_and_separators() {
        // 路径穿越：只取 file_name 分量
        assert_eq!(sanitize_filename(Some("../../etc/passwd"), None), "passwd");
        assert_eq!(sanitize_filename(Some("..\\..\\secret.txt"), None), "secret.txt");
        // file_name 取末分量（c:d.txt）后，残留分隔符与控制字符滤除
        assert_eq!(sanitize_filename(Some("a/b\\c:d.txt"), None), "cd.txt");
        assert_eq!(sanitize_filename(Some("bad\u{0}name.csv"), None), "badname.csv");
        // 首尾点与空白 trim
        assert_eq!(sanitize_filename(Some("  .hidden.  "), None), "hidden");
    }

    #[test]
    fn sanitize_empty_falls_back_to_uuid() {
        let name = sanitize_filename(Some("..."), None);
        assert!(!name.is_empty());
        assert!(uuid::Uuid::parse_str(&name).is_ok(), "空名应兜底 UUID：{name}");
        let none = sanitize_filename(None, None);
        assert!(uuid::Uuid::parse_str(&none).is_ok());
    }

    #[test]
    fn sanitize_adds_extension_from_content_type() {
        assert_eq!(sanitize_filename(Some("data"), Some("text/csv")), "data.csv");
        assert_eq!(sanitize_filename(Some("photo"), Some("image/png")), "photo.png");
        // 已有扩展名不覆盖
        assert_eq!(sanitize_filename(Some("a.txt"), Some("text/csv")), "a.txt");
        // 未知 MIME 不造扩展名
        assert_eq!(sanitize_filename(Some("blob"), Some("application/x-unknown")), "blob");
        // MIME 带参数取分号前
        assert_eq!(extension_from_content_type(Some("text/plain; charset=utf-8")), Some("txt"));
    }

    #[tokio::test]
    async fn collision_appends_numeric_suffix() {
        let dir = tmp_dir("collision");
        tokio::fs::write(dir.join("a.csv"), b"x").await.unwrap();
        let p1 = resolve_collision(&dir, "a.csv").await;
        assert_eq!(p1.file_name().unwrap(), "a-1.csv");
        tokio::fs::write(&p1, b"x").await.unwrap();
        let p2 = resolve_collision(&dir, "a.csv").await;
        assert_eq!(p2.file_name().unwrap(), "a-2.csv");
        // 无扩展名同样加后缀
        tokio::fs::write(dir.join("blob"), b"x").await.unwrap();
        let p3 = resolve_collision(&dir, "blob").await;
        assert_eq!(p3.file_name().unwrap(), "blob-1");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn collision_free_name_unchanged() {
        let dir = tmp_dir("free");
        let p = resolve_collision(&dir, "fresh.csv").await;
        assert_eq!(p.file_name().unwrap(), "fresh.csv");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
