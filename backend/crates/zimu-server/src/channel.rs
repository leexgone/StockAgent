//! 通道句柄（ADR-0017 B4，自 qq_channel.rs 的 QQChannel 抽出）：QQ/微信
//! supervisor 结构全同，共享一型——health 的在线位由 main 自持同一个
//! `Arc<AtomicBool>` 读；优雅关闭逐通道 `request_stop()` 后 `join()`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub struct ChannelHandle {
    stop: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
    handle: JoinHandle<()>,
}

impl ChannelHandle {
    pub fn new(stop: Arc<AtomicBool>, shutdown: Arc<Notify>, handle: JoinHandle<()>) -> Self {
        Self { stop, shutdown, handle }
    }

    /// 停：置位 + 唤醒 manager.run 的 select（robit-chatbot manager.rs:224-227 契约）。
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.shutdown.notify_waiters();
    }

    /// 等通道任务退出 + 200ms flush（SQLite WAL 落盘——robit-qq main.rs:113 同款收尾）。
    pub async fn join(self, wait: Duration) {
        let stop = self.stop.clone();
        let shutdown = self.shutdown.clone();
        let _ = tokio::time::timeout(wait, async {
            // 兜底再喊一次（幂等）：调用方 request_stop 与这里之间可能有竞态
            stop.store(true, Ordering::SeqCst);
            shutdown.notify_waiters();
            let _ = self.handle.await;
        })
        .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}