//! 通道句柄（ADR-0017 B4，自 qq_channel.rs 的 QQChannel 抽出）：QQ/微信
//! supervisor 结构全同，共享一型——health 的在线位由 main 自持同一个
//! `Arc<AtomicBool>` 读；优雅关闭逐通道 `request_stop()` 后 `join()`。
//!
//! robit 0.4.0（v0.3.1 起）：关停信号由 `Arc<Notify>`（边沿触发，无人等待时
//! 信号丢失）统一为 `CancellationToken`（电平触发，取消发生在处理中也不丢）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct ChannelHandle {
    stop: Arc<AtomicBool>,
    shutdown: CancellationToken,
    handle: JoinHandle<()>,
}

impl ChannelHandle {
    pub fn new(stop: Arc<AtomicBool>, shutdown: CancellationToken, handle: JoinHandle<()>) -> Self {
        Self { stop, shutdown, handle }
    }

    /// 停：置位 + 取消令牌（manager.run / adapter connect / 退避睡眠全部收束）。
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.shutdown.cancel();
    }

    /// 等通道任务退出 + 200ms flush（SQLite WAL 落盘——robit-qq main.rs 同款收尾）。
    pub async fn join(self, wait: Duration) {
        let stop = self.stop.clone();
        let shutdown = self.shutdown.clone();
        let _ = tokio::time::timeout(wait, async {
            // 兜底再取消一次（幂等）：调用方 request_stop 与这里之间可能有竞态
            stop.store(true, Ordering::SeqCst);
            shutdown.cancel();
            let _ = self.handle.await;
        })
        .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
