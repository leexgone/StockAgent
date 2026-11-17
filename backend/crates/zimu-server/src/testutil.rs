//! lib 单测共享装置（ADR-0017 B3）：MockPlatform + 临时 store。
//!
//! 自 channel_adapter.rs 测试模块抽出（行为不变），channel_push 的 caps 参数化
//! 测试复用。`PlatformAdapter` 的 `capabilities()`/`platform_name()` 是**无 self
//! 的静态方法**——同一类型无法按实例配置平台身份，故用宏按 (名字, caps) 生成
//! 具体 mock 类型（QqMock/WechatMock/…），静态面恰为被测参数化的驱动源。

use std::sync::{Arc, Mutex};

use robit_chatbot::{ChatMessage, ChatType, PlatformEvent, SenderInfo};

use crate::holdings_api::SharedStore;

/// 一条私聊消息事件（chat_id 即 sender.chat_id，白名单判定键）
pub(crate) fn msg(chat_id: &str, text: &str) -> PlatformEvent {
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

/// 每测试独立路径（同 pid 并发跑，共享文件会互删竞态）
pub(crate) fn tmp_store(tag: &str) -> SharedStore {
    let dir = std::env::temp_dir().join(format!("zimu-bound-{}-{tag}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("bound.db");
    let _ = std::fs::remove_file(&path);
    Arc::new(Mutex::new(
        zimu_store::Store::open(path.to_str().unwrap()).unwrap(),
    ))
}

/// 生成一个 MockPlatform 类型：事件队列 + 出站记录 + 可注入发送失败。
/// 用法：`mock_platform!(QqMock, "qq", robit_chatbot::PlatformCaps::qq());`
macro_rules! mock_platform {
    ($name:ident, $platform:expr, $caps:expr) => {
        struct $name {
            events: std::sync::Mutex<std::collections::VecDeque<robit_chatbot::PlatformEvent>>,
            sent: std::sync::Mutex<Vec<(String, String)>>,
            fail_send: std::sync::atomic::AtomicBool,
        }
        impl $name {
            #[allow(dead_code)]
            fn with(events: Vec<robit_chatbot::PlatformEvent>) -> std::sync::Arc<Self> {
                std::sync::Arc::new(Self {
                    events: std::sync::Mutex::new(std::collections::VecDeque::from(events)),
                    sent: std::sync::Mutex::new(Vec::new()),
                    fail_send: std::sync::atomic::AtomicBool::new(false),
                })
            }
        }
        #[async_trait::async_trait]
        impl robit_chatbot::PlatformAdapter for $name {
            fn capabilities() -> robit_chatbot::PlatformCaps {
                $caps
            }
            fn platform_name() -> &'static str {
                $platform
            }
            async fn recv_event(&self) -> robit_agent::error::Result<robit_chatbot::PlatformEvent> {
                self.events.lock().unwrap().pop_front().ok_or_else(|| {
                    robit_agent::error::AgentError::InternalError("事件耗尽".into())
                })
            }
            async fn send_message(
                &self,
                chat_id: &str,
                text: &str,
            ) -> robit_agent::error::Result<robit_chatbot::SendResult> {
                if self.fail_send.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(robit_agent::error::AgentError::InternalError(
                        "mock 发送失败".into(),
                    ));
                }
                self.sent.lock().unwrap().push((chat_id.into(), text.into()));
                Ok(robit_chatbot::SendResult { msg_id: "mock".into() })
            }
        }
    };
}
pub(crate) use mock_platform;