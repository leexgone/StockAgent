//! IM 入站白名单装饰器（T5/ADR-0011 拍板⑤）：`PlatformAdapter` 的**纯组合**包装——
//! 不绕 trait、不改 robit，绑定判定表在 `zimu.db channel_bindings`。
//!
//! 语义（拍板：绑定关系界面管理、连接参数配置文件）：
//! - **入站闸门**：`recv_event` 里未绑定 chat 的消息**静默丢弃 + 日志留 chat_id**
//!   （不回「未绑定」提示——避免任何 QQ 用户发一条就收到回复的骚扰面；日志里的
//!   chat_id 足够人工完成绑定：`POST /api/channel_bindings`）；
//! - **出站不拦**：`send_message` 等透传 inner——主动推送（T6）走宿主自持的 inner
//!   Arc 或这里的透传均可，白名单是「谁能对子木说话」，不是「子木能答谁」的
//!   第二道闸（绑定即授权，解绑后回复自然停：入站没了，出站由推送侧另行判定）；
//! - **判定失败按未绑定**（fail-closed）：store 出错时宁可不回应答，日志吼出来；
//! - `capabilities()` 是 trait 关联静态函数——generic impl 里 `T::capabilities()`
//!   静态转发即可，装饰器不改变平台能力宣称。

use std::sync::Arc;

use async_trait::async_trait;
use robit_agent::error::Result;
use robit_chatbot::{PlatformAdapter, PlatformCaps, PlatformEvent, SendResult, UploadResult};

use crate::holdings_api::SharedStore;

pub struct BoundOnly<T: PlatformAdapter> {
    inner: Arc<T>,
    store: SharedStore,
    /// 通道词汇表（'qq'/'wechat'）；与 channel_bindings.channel 列同源
    channel: &'static str,
}

impl<T: PlatformAdapter> BoundOnly<T> {
    pub fn new(inner: Arc<T>, store: SharedStore, channel: &'static str) -> Self {
        Self { inner, store, channel }
    }

    /// inner 句柄（T6 推送侧要自持原始 adapter 出站；decorator 也转发出站，
    /// 保留取用口让两条路都通）
    pub fn inner(&self) -> &Arc<T> {
        &self.inner
    }

    /// 单点判定：失败按未绑定处理（fail-closed）并如实 warn。
    fn is_bound(&self, chat_id: &str) -> bool {
        match zimu_agent::lock(&self.store).is_channel_bound(self.channel, chat_id) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("绑定表读取失败（按未绑定丢弃入站）：{e:#}");
                false
            }
        }
    }
}

#[async_trait]
impl<T: PlatformAdapter + 'static> PlatformAdapter for BoundOnly<T> {
    fn capabilities() -> PlatformCaps {
        // 静态转发：装饰不改变平台能力（robit 若把它改成 self 方法更灵活，R8 素材已减一条）
        T::capabilities()
    }

    async fn recv_event(&self) -> Result<PlatformEvent> {
        loop {
            let ev = self.inner.recv_event().await?;
            match &ev {
                PlatformEvent::Message(msg) => {
                    if self.is_bound(&msg.sender.chat_id) {
                        return Ok(ev);
                    }
                    // 静默丢弃 + 留案底：chat_id 进日志供人工绑定
                    tracing::info!(
                        "未绑定会话来消息，丢弃：channel={} chat_id={} text={:.40}",
                        self.channel,
                        msg.sender.chat_id,
                        msg.text,
                    );
                }
                // Disconnected / Other 原样透传（宿主 supervisor 负责处理）
                _ => return Ok(ev),
            }
        }
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<SendResult> {
        self.inner.send_message(chat_id, text).await
    }

    async fn edit_message(&self, chat_id: &str, msg_id: &str, text: &str) -> Result<()> {
        self.inner.edit_message(chat_id, msg_id, text).await
    }

    async fn upload_file(&self, chat_id: &str, file_path: &str, media_type: &str) -> Result<UploadResult> {
        self.inner.upload_file(chat_id, file_path, media_type).await
    }

    async fn send_media_message(
        &self,
        chat_id: &str,
        file_url: &str,
        file_name: &str,
        media_type: &str,
    ) -> Result<SendResult> {
        self.inner.send_media_message(chat_id, file_url, file_name, media_type).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robit_agent::error::AgentError;
    use robit_chatbot::{ChatMessage, ChatType, MarkdownFeatures, SenderInfo};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// 事件队列 + 出站记录（照 robit manager.rs 测试的 MockPlatform 形态）
    struct MockPlatform {
        events: Mutex<VecDeque<PlatformEvent>>,
        sent: Mutex<Vec<(String, String)>>,
    }

    impl MockPlatform {
        fn with(events: Vec<PlatformEvent>) -> Arc<Self> {
            Arc::new(Self { events: Mutex::new(VecDeque::from(events)), sent: Mutex::new(Vec::new()) })
        }
    }

    fn msg(chat_id: &str, text: &str) -> PlatformEvent {
        PlatformEvent::Message(ChatMessage {
            text: text.into(),
            sender: SenderInfo {
                user_id: chat_id.into(),
                chat_id: chat_id.into(),
                chat_type: ChatType::Private,
            },
            attachments: vec![],
        })
    }

    fn caps() -> PlatformCaps {
        PlatformCaps {
            supports_edit: false,
            returns_msg_id: true,
            supports_markdown: false,
            markdown_features: MarkdownFeatures::default(),
            max_message_length: 100,
            supports_images: false,
            supports_files: false,
            max_upload_size: 0,
        }
    }

    #[async_trait]
    impl PlatformAdapter for MockPlatform {
        fn capabilities() -> PlatformCaps {
            caps()
        }
        async fn recv_event(&self) -> Result<PlatformEvent> {
            self.events
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| AgentError::InternalError("事件耗尽".into()))
        }
        async fn send_message(&self, chat_id: &str, text: &str) -> Result<SendResult> {
            self.sent.lock().unwrap().push((chat_id.into(), text.into()));
            Ok(SendResult { msg_id: "mock".into() })
        }
    }

    /// 每测试独立路径（同 pid 并发跑，共享文件会互删竞态）
    fn tmp_store(tag: &str) -> SharedStore {
        let dir = std::env::temp_dir().join(format!("zimu-bound-{}-{tag}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bound.db");
        let _ = std::fs::remove_file(&path);
        Arc::new(Mutex::new(
            zimu_store::Store::open(path.to_str().unwrap()).unwrap(),
        ))
    }

    #[tokio::test]
    async fn unbound_chat_silently_dropped_until_bound() {
        let store = tmp_store("unbound");
        let mock = MockPlatform::with(vec![msg("private:EVE", "骚扰"), msg("private:ME", "正主")]);
        let adapter = BoundOnly::new(mock.clone(), store.clone(), "qq");

        // 未绑定：EVE 被吞，循环取到下一条 ME——同样未绑定 → 队列耗尽 Err
        let ev = adapter.recv_event().await;
        assert!(ev.is_err(), "全部未绑定 → 透传 inner 耗尽错误，不造幽灵消息");

        // 绑定 ME 后：EVE 照旧吞、ME 放行
        zimu_agent::lock(&store).bind_channel("qq", "private:ME", "private", "本人", "2026-11-07 09:00").unwrap();
        let mock = MockPlatform::with(vec![msg("private:EVE", "骚扰"), msg("private:ME", "正主")]);
        let adapter = BoundOnly::new(mock, store.clone(), "qq");
        let got = adapter.recv_event().await.expect("绑定会话应放行");
        match got {
            PlatformEvent::Message(m) => assert_eq!(m.sender.chat_id, "private:ME"),
            other => panic!("期望 Message，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn outbound_and_nonmessage_events_pass_through() {
        let store = tmp_store("outbound");
        // Other/Disconnected 不经白名单
        let mock = MockPlatform::with(vec![PlatformEvent::Other(serde_json::json!({"raw": 1}))]);
        let adapter = BoundOnly::new(mock.clone(), store.clone(), "qq");
        assert!(matches!(adapter.recv_event().await.unwrap(), PlatformEvent::Other(_)));

        // 出站透传（推送侧语义在 T6：出站不受入站闸门约束）
        adapter.send_message("private:ANY", "推送").await.unwrap();
        assert_eq!(mock.sent.lock().unwrap().as_slice(), [("private:ANY".to_string(), "推送".to_string())]);

        // 能力宣称静态转发
        assert_eq!(BoundOnly::<MockPlatform>::capabilities().max_message_length, 100);
    }
}
