//! 待绑定队列（ADR-0017 D3 补记）：**观察自动化、授权仍人工**——未绑定会话
//! 来消息被 `BoundOnly` 静默丢弃时，顺手把 chat_id 记进本队列（内存态），
//! wechat 屏绑定卡经 `GET /api/channel_bindings/pending` 自动列出，主人一键
//! 绑定（不用抄日志）。**fail-closed 闸门不变**：入列 ≠ 放行，绑定仍是主人
//! 明示动作（POST /api/channel_bindings），绑定成功即从队列移除。
//!
//! 语义：按 (channel, chat_id) 去重累计（hits/last_seen/last_text 刷新，
//! 活跃者移队尾）；上限 20 条逐出最旧；**进程内存态不落库**——重启清零
//! （未绑定者再发一条消息即重新出现），消息摘录只留 ≤40 字符（与日志同宽，
//! 帮主人辨认哪条是自己发的），不留完整消息内容。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use utoipa::ToSchema;

use crate::chat_record::ts_now;

/// 队列上限（个人规模：主人 + 少量家人/群，20 条足够；逐出最旧）
const CAP: usize = 20;
/// 消息摘录宽度（字符，与 BoundOnly 日志 `{:.40}` 同宽）
const TEXT_PREVIEW: usize = 40;

/// 一条待绑定观察记录（未绑定会话的来消息尝试，去重累计）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PendingAttempt {
    /// 通道词汇（qq / wechat）
    pub channel: String,
    /// 平台会话 id（绑定表单直接取用）
    pub chat_id: String,
    /// 最近一条消息摘录（≤40 字符）——帮主人辨认哪条是自己发的
    pub last_text: String,
    /// 首次观察到（上海 YYYY-MM-DD HH:MM）
    pub first_seen: String,
    /// 最近观察到
    pub last_seen: String,
    /// 未绑定被丢弃的来消息次数（去重累计）
    pub hits: u32,
}

/// 共享队列句柄（BoundOnly 写入 / 绑定端点读取与移除）。
#[derive(Clone, Default)]
pub struct PendingBindings(Arc<Mutex<VecDeque<PendingAttempt>>>);

impl PendingBindings {
    /// 记录一次未绑定来消息（BoundOnly 丢弃路径调用）。锁内只做内存操作。
    pub fn record(&self, channel: &str, chat_id: &str, text: &str) {
        let now = ts_now();
        let preview: String = text.chars().take(TEXT_PREVIEW).collect();
        let Ok(mut q) = self.0.lock() else {
            return; // 锁中毒不致命：观察面丢一条无碍，闸门判定不受影响
        };
        if let Some(pos) = q.iter().position(|a| a.channel == channel && a.chat_id == chat_id) {
            let mut a = q.remove(pos).unwrap();
            a.last_seen = now;
            a.last_text = preview;
            a.hits = a.hits.saturating_add(1);
            q.push_back(a); // 活跃者移队尾（list 反序 = 最近活跃在前）
        } else {
            q.push_back(PendingAttempt {
                channel: channel.to_string(),
                chat_id: chat_id.to_string(),
                last_text: preview,
                first_seen: now.clone(),
                last_seen: now,
                hits: 1,
            });
            while q.len() > CAP {
                q.pop_front();
            }
        }
    }

    /// 最近活跃在前（绑定卡展示序）。
    pub fn list(&self) -> Vec<PendingAttempt> {
        match self.0.lock() {
            Ok(q) => q.iter().rev().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 绑定成功后移除（授权完成，观察记录使命结束）。
    pub fn remove(&self, channel: &str, chat_id: &str) {
        if let Ok(mut q) = self.0.lock() {
            q.retain(|a| !(a.channel == channel && a.chat_id == chat_id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_accumulates_and_moves_to_front() {
        let p = PendingBindings::default();
        p.record("wechat", "private:A", "第一条");
        p.record("qq", "private:B", "乙");
        p.record("wechat", "private:A", "第二条更长一些的消息");
        let rows = p.list();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].chat_id, "private:A", "最近活跃在前");
        assert_eq!(rows[0].hits, 2);
        assert_eq!(rows[0].last_text, "第二条更长一些的消息");
        assert_eq!(rows[1].chat_id, "private:B");
    }

    #[test]
    fn cap_evicts_oldest() {
        let p = PendingBindings::default();
        for i in 0..(CAP + 5) {
            p.record("qq", &format!("private:{i}"), "x");
        }
        let rows = p.list();
        assert_eq!(rows.len(), CAP);
        assert_eq!(rows[0].chat_id, format!("private:{}", CAP + 4));
        assert!(rows.iter().all(|r| r.chat_id != "private:0"), "最旧被逐出");
    }

    #[test]
    fn remove_on_bind_and_text_preview_width() {
        let p = PendingBindings::default();
        let long: String = "长".repeat(100);
        p.record("wechat", "private:A", &long);
        assert_eq!(p.list()[0].last_text.chars().count(), TEXT_PREVIEW);
        p.remove("wechat", "private:A");
        assert!(p.list().is_empty());
        // 跨通道同名 chat_id 不互删
        p.record("qq", "private:X", "q");
        p.record("wechat", "private:X", "w");
        p.remove("qq", "private:X");
        let rows = p.list();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].channel, "wechat");
    }
}