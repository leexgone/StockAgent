//! 通道推送出站（T6/ADR-0011 → ADR-0017 D5 caps 参数化）——文案组装、多目标
//! 循环、逐目标留痕都在宿主；调度器只说「推这条」（scheduler trait 通道无关）。
//!
//! **零硬编码通道词汇**：通道名取 `T::platform_name()`（恰为 channel_bindings
//! 的 channel 词汇），清洗与截断参数取 `T::capabilities()`——QQ 数值行为逐位
//! 不变（caps.qq 的 features/2000 上限有回归钉单测），微信纯文本投影自动成立
//! （MarkdownFeatures::wechat() 全 false → 可读纯文本 + 2000 字按行截断）。
//!
//! 纪律（ADR-0011 后果段）：送达失败如实记、**不自动重试**（调度器同日去重已
//! 定格，失败的可补救面就是留痕本身，与「受理 ≠ 完成」同纪律）；未在线/无绑定
//! 同样留痕不装送达。锁内只 clone 句柄，await 在锁外（跨 await 持锁禁止）。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use robit_chatbot::{PlatformAdapter, SendResult};
use zimu_agent::scheduler::{AlertPusher, NewAlertInfo, SummaryPusher};
use zimu_store::EquitySnapshotRow;

use crate::holdings_api::SharedStore;

/// 在线连接槽（T6 推送侧的「只推在线的」）：通道 run_once 连上后写入 adapter，
/// 退出/失败清空。宿主 sink 经它出站——未连接时 `send` 返回 None（如实失败，
/// 不装送达）。
///
/// 泛型化（ADR-0017）：QQ/微信各持一槽。**Clone/Default 必须手写**——derive
/// 会附加 `T: Clone/Default` 约束，而平台 adapter（如 WechatPlatformAdapter）
/// 两者皆无；槽的克隆语义只关 Arc 外层，与 T 无关。
pub struct ActiveAdapter<T: PlatformAdapter>(Arc<Mutex<Option<Arc<T>>>>);

impl<T: PlatformAdapter> Clone for ActiveAdapter<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: PlatformAdapter> Default for ActiveAdapter<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
}

impl<T: PlatformAdapter> ActiveAdapter<T> {
    /// 出站一条文本：None = 通道当前不在线；Some(Err) = 平台发送失败。
    pub async fn send(&self, chat_id: &str, text: &str) -> Option<robit_agent::error::Result<SendResult>> {
        let adapter = self.0.lock().ok()?.clone()?;
        Some(adapter.send_message(chat_id, text).await)
    }

    pub fn set(&self, adapter: Arc<T>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(adapter);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

/// 一条系统消息 → 该通道全部已绑定会话，逐目标 push_log 留痕。
/// 返回是否送达至少一目标。通道词汇与投影参数全部来自 T 的静态能力面。
pub async fn push_to_bound<T: PlatformAdapter>(
    store: &SharedStore,
    active: &ActiveAdapter<T>,
    kind: &'static str,
    text: &str,
    subject: &str,
) -> bool {
    let channel = T::platform_name();
    let targets = match zimu_agent::lock(store).channel_bindings(Some(channel)) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("绑定会话读取失败（放弃推送）：{e:#}");
            return false;
        }
    };
    if targets.is_empty() {
        let _ = zimu_agent::lock(store).insert_push_log(
            &crate::chat_record::ts_now(),
            channel,
            kind,
            false,
            &format!("{subject}｜无已绑定会话（跳过推送）"),
        );
        return false;
    }
    // 平台 Markdown 面投影：过 robit-chatbot 现成清洗器降级为该平台可读文本
    // （QQ 只有 code_blocks；微信全 false → 纯文本），再按 caps 上限截断。
    let caps = T::capabilities();
    let cleaned = robit_chatbot::markdown::prepare_markdown_for_platform(text, &caps.markdown_features);
    let clipped = clip_by_lines(&cleaned, caps.max_message_length);
    let mut any = false;
    for t in targets {
        let (ok, note): (bool, String) = match active.send(&t.chat_id, &clipped).await {
            None => (false, "通道未在线（发送放弃）".into()),
            Some(Ok(_)) => (true, "已送达".into()),
            Some(Err(e)) => (false, format!("发送失败：{e}")),
        };
        any |= ok;
        if let Err(e) = zimu_agent::lock(store).insert_push_log(
            &crate::chat_record::ts_now(),
            channel,
            kind,
            ok,
            &format!("{subject} → {}：{note}", t.chat_id),
        ) {
            tracing::warn!("推送留痕失败：{e:#}");
        }
    }
    any
}

/// 按行截到 cap 字符内（不切半行；超限省略标记）——保证多行摘要不被拦腰截断。
/// 注意：省略标记「\n…（已截断）」追加在截断点之后，总长可达 cap+8（QQ 既有
/// 语义，原样迁移保持逐位不变；真实摘要 ~10 行远小于上限，标记路径罕触）。
pub fn clip_by_lines(s: &str, cap: usize) -> String {
    let mut out = String::new();
    for line in s.lines() {
        let candidate = if out.is_empty() {
            line.to_string()
        } else {
            format!("{out}\n{line}")
        };
        if candidate.chars().count() > cap {
            if out.is_empty() {
                // 单行超限：按字符硬截（至少给出开头）
                return s.chars().take(cap.saturating_sub(4)).collect::<String>() + "…截";
            }
            out.push_str("\n…（已截断）");
            return out;
        }
        out = candidate;
    }
    out
}

/// 预警推送文案（原 main.rs 装配处抽出，数字 = 判定产出原值不二次加工）：
/// 返回 (消息正文, 留痕主题)。
pub fn alert_message(a: &NewAlertInfo) -> (String, String) {
    (
        format!("子木预警 · {} {}", a.name, a.text),
        format!("预警 {} {}", a.code, a.alert_type),
    )
}

/// 盘后数字摘要文案（数字 = equity_snapshots 落库原值，03 §12 不现编；
/// 复盘正文留在 Web 会话，这里只推数字摘要）。**前端 wechat 屏预览卡按同款
/// 行格式渲染（WechatScreen summary 预览），改这里须同步那边**。
pub fn summary_message(snap: &EquitySnapshotRow) -> (String, String) {
    (
        format!(
            "子木盘后摘要 {}\n总资产 {:.2}\n当日盈亏 {:+.2}（{:+.2}%）\n累计盈亏 {:+.2}（{:+.2}%）\n现金 {:.2} · 仓位 {:.1}% · 集中度 {:.2}",
            snap.date,
            snap.total_assets,
            snap.day_pnl,
            snap.day_pnl_pct,
            snap.total_pnl,
            snap.total_pnl_pct,
            snap.cash,
            snap.exposure,
            snap.hhi,
        ),
        format!("盘后摘要 {}", snap.date),
    )
}

/// 单通道推送闭包：(kind, 消息正文, 留痕主题) → 是否送达至少一目标。
/// 泛型在此收口为 dyn（组合面不再认平台类型）；kind 是 &'static str 可 Copy，
/// 正文/主题 move 进 async block（boxed future 'static，不能借参数）。
pub type ChannelPush = Arc<
    dyn Fn(&'static str, String, String) -> Pin<Box<dyn Future<Output = bool> + Send>>
        + Send
        + Sync,
>;

pub fn make_channel_push<T: PlatformAdapter + 'static>(
    store: SharedStore,
    active: ActiveAdapter<T>,
) -> ChannelPush {
    Arc::new(move |kind: &'static str, msg: String, subject: String| {
        let store = store.clone();
        let active = active.clone();
        Box::pin(async move { push_to_bound::<T>(&store, &active, kind, &msg, &subject).await })
    })
}

/// 复合预警 pusher：逐通道顺序执行取 OR（个人规模 ≤2 通道，顺序保证 push_log
/// 时序确定、store 锁竞争最小）。空 vec → false（未启用任何通道 = 未接线语义）。
/// 文案只组装一次，逐通道 clone（NewAlertInfo/EquitySnapshotRow 均 Clone）。
pub fn combine_alert_pushers(channels: Vec<ChannelPush>) -> AlertPusher {
    Arc::new(move |a: NewAlertInfo| {
        let channels = channels.clone();
        Box::pin(async move {
            let (msg, subject) = alert_message(&a);
            let mut any = false;
            for ch in channels.iter() {
                any |= ch("alert_push", msg.clone(), subject.clone()).await;
            }
            any
        })
    })
}

/// 复合盘后摘要 pusher（语义同 combine_alert_pushers）。
pub fn combine_summary_pushers(channels: Vec<ChannelPush>) -> SummaryPusher {
    Arc::new(move |snap: EquitySnapshotRow| {
        let channels = channels.clone();
        Box::pin(async move {
            let (msg, subject) = summary_message(&snap);
            let mut any = false;
            for ch in channels.iter() {
                any |= ch("post_market_review_push", msg.clone(), subject.clone()).await;
            }
            any
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{mock_platform, tmp_store};
    use robit_chatbot::{MarkdownFeatures, PlatformCaps};

    mock_platform!(QqMock, "qq", PlatformCaps::qq());
    mock_platform!(WechatMock, "wechat", PlatformCaps::wechat());

    fn bind(store: &SharedStore, channel: &str, chat_id: &str) {
        zimu_agent::lock(store)
            .bind_channel(channel, chat_id, "private", "本人", "2026-11-17 09:00")
            .unwrap();
    }

    fn logs(store: &SharedStore) -> Vec<zimu_store::PushLogRow> {
        zimu_agent::lock(store).push_logs(100).unwrap()
    }

    /// caps.qq 回归钉：参数化后 QQ 的清洗面与长度上限必须逐位不变
    /// （MarkdownFeatures 无 PartialEq，按 Debug 形比较全字段）
    #[test]
    fn qq_caps_regression_pin() {
        let caps = PlatformCaps::qq();
        assert_eq!(caps.max_message_length, 2000);
        assert_eq!(
            format!("{:?}", caps.markdown_features),
            format!("{:?}", MarkdownFeatures::qq())
        );
        // 微信纯文本面：全 false + 同为 2000 上限
        let wx = PlatformCaps::wechat();
        assert_eq!(wx.max_message_length, 2000);
        assert_eq!(
            format!("{:?}", wx.markdown_features),
            format!("{:?}", MarkdownFeatures::default())
        );
        assert!(!wx.supports_markdown);
    }

    /// QQ 推送只发 qq 绑定（wechat 绑定不串台），留痕 channel="qq"
    #[tokio::test]
    async fn qq_push_targets_only_qq_bindings() {
        let store = tmp_store("push-qq");
        bind(&store, "qq", "private:QQME");
        bind(&store, "wechat", "private:WXME");
        let mock = QqMock::with(vec![]);
        let active = ActiveAdapter::<QqMock>::default();
        active.set(mock.clone());

        let ok = push_to_bound::<QqMock>(&store, &active, "alert_push", "预警正文", "预警 600519 stop_loss").await;
        assert!(ok);
        assert_eq!(
            mock.sent.lock().unwrap().as_slice(),
            [("private:QQME".to_string(), "预警正文".to_string())]
        );
        let rows = logs(&store);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].channel, "qq");
        assert_eq!(rows[0].kind, "alert_push");
        assert!(rows[0].ok);
        assert!(rows[0].detail.contains("已送达"));
    }

    /// 微信推送：Markdown 剥成纯文本 + 超 2000 字按行截断，留痕 channel="wechat"
    #[tokio::test]
    async fn wechat_push_cleans_markdown_and_clips() {
        let store = tmp_store("push-wx");
        bind(&store, "wechat", "private:WXME");
        let mock = WechatMock::with(vec![]);
        let active = ActiveAdapter::<WechatMock>::default();
        active.set(mock.clone());

        // 200 行 × 20 字符 ≈ 4200 字符 > 2000；首段带 bold/italic 标记
        let mut text = String::from("**加粗** 与 *斜体*\n");
        for i in 0..200 {
            text.push_str(&format!("第{i:03}行内容十字符号填充\n"));
        }
        let ok = push_to_bound::<WechatMock>(&store, &active, "post_market_review_push", &text, "盘后摘要").await;
        assert!(ok);
        let sent = mock.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "private:WXME");
        let body = &sent[0].1;
        assert!(body.contains("加粗 与 斜体"), "bold/italic 标记应剥除：{body:.60}");
        assert!(!body.contains("**"));
        // clip_by_lines 原样迁移（QQ 行为逐位不变）：正文截到 cap 内，省略标记
        // 「\n…（已截断）」（8 字符）追加在截断点之后——上限语义 = cap + 标记长
        assert!(body.chars().count() <= 2000 + 8, "超长必须截断");
        assert!(body.ends_with("…（已截断）"), "按行截断省略标记");
        assert!(!body.contains("第199行"), "尾部行应被截掉");
        let rows = logs(&store);
        assert_eq!(rows[0].channel, "wechat");
        assert_eq!(rows[0].kind, "post_market_review_push");
    }

    /// 无绑定：跳过推送但留痕（ok=0），不装送达
    #[tokio::test]
    async fn no_bindings_logs_skip() {
        let store = tmp_store("push-nobind");
        let active = ActiveAdapter::<WechatMock>::default();
        let ok = push_to_bound::<WechatMock>(&store, &active, "alert_push", "x", "预警 600519 stop_loss").await;
        assert!(!ok);
        let rows = logs(&store);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].ok);
        assert_eq!(rows[0].channel, "wechat");
        assert!(rows[0].detail.contains("无已绑定会话"));
    }

    /// 有绑定但通道不在线：逐目标如实记「通道未在线」
    #[tokio::test]
    async fn offline_active_fails_honestly() {
        let store = tmp_store("push-offline");
        bind(&store, "qq", "private:QQME");
        let active = ActiveAdapter::<QqMock>::default(); // 未 set = 不在线
        let ok = push_to_bound::<QqMock>(&store, &active, "alert_push", "x", "预警").await;
        assert!(!ok);
        let rows = logs(&store);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].ok);
        assert!(rows[0].detail.contains("通道未在线"));
    }

    /// 发送失败：留痕失败且续发下一目标（不中断循环）
    #[tokio::test]
    async fn send_err_continues_to_next_target() {
        let store = tmp_store("push-senderr");
        bind(&store, "qq", "private:A");
        // 第二条绑定 bound_at 更晚 → 排序在后
        zimu_agent::lock(&store)
            .bind_channel("qq", "private:B", "private", "二号", "2026-11-17 10:00")
            .unwrap();
        let mock = QqMock::with(vec![]);
        mock.fail_send.store(true, std::sync::atomic::Ordering::SeqCst);
        let active = ActiveAdapter::<QqMock>::default();
        active.set(mock.clone());
        let ok = push_to_bound::<QqMock>(&store, &active, "alert_push", "x", "预警").await;
        assert!(!ok);
        let rows = logs(&store);
        assert_eq!(rows.len(), 2, "两个目标各留一条失败痕");
        assert!(rows.iter().all(|r| !r.ok && r.detail.contains("发送失败")));
    }

    /// 复合 pusher：双通道 OR 语义（一边送达即 true）、空 vec false、文案恰一份
    #[tokio::test]
    async fn combine_pushers_or_semantics() {
        let store = tmp_store("push-combine");
        bind(&store, "qq", "private:QQME");
        bind(&store, "wechat", "private:WXME");
        let qq = QqMock::with(vec![]);
        let wx = WechatMock::with(vec![]);
        let qq_active = ActiveAdapter::<QqMock>::default();
        qq_active.set(qq.clone());
        let wx_active = ActiveAdapter::<WechatMock>::default();
        wx_active.set(wx.clone());

        let channels = vec![
            make_channel_push::<QqMock>(store.clone(), qq_active),
            make_channel_push::<WechatMock>(store.clone(), wx_active),
        ];
        let alert = combine_alert_pushers(channels.clone());
        let info = NewAlertInfo {
            id: 1,
            code: "600519".into(),
            name: "贵州茅台".into(),
            alert_type: "stop_loss".into(),
            level: "danger".into(),
            text: "跌破止损线 1420.00".into(),
        };
        assert!(alert(info.clone()).await);
        // 双通道各收一条，文案同款（数字原值）
        assert_eq!(qq.sent.lock().unwrap()[0].1, "子木预警 · 贵州茅台 跌破止损线 1420.00");
        assert_eq!(wx.sent.lock().unwrap()[0].1, "子木预警 · 贵州茅台 跌破止损线 1420.00");
        let rows = logs(&store);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.channel == "qq") && rows.iter().any(|r| r.channel == "wechat"));

        // 摘要复合：数字 = 落库原值格式
        let summary = combine_summary_pushers(channels);
        let snap = EquitySnapshotRow {
            date: "2026-11-17".into(),
            ts: "2026-11-17 15:05".into(),
            total_assets: 290362.30,
            total_mv: 200000.0,
            cash: 90362.30,
            day_pnl: 1234.56,
            day_pnl_pct: 0.43,
            total_pnl: -5678.90,
            total_pnl_pct: -1.92,
            exposure: 68.9,
            hhi: 0.25,
            top3_sector: 80.0,
        };
        assert!(summary(snap).await);
        let body = &wx.sent.lock().unwrap()[1].1;
        assert!(body.starts_with("子木盘后摘要 2026-11-17\n总资产 290362.30\n"));
        assert!(body.contains("当日盈亏 +1234.56（+0.43%）"));
        assert!(body.contains("累计盈亏 -5678.90（-1.92%）"));
        assert!(body.contains("现金 90362.30 · 仓位 68.9% · 集中度 0.25"));

        // 空 vec = 未接线语义
        assert!(!combine_alert_pushers(vec![])(info).await);
        assert!(!combine_summary_pushers(vec![])(EquitySnapshotRow {
            date: "2026-11-17".into(),
            ts: String::new(),
            total_assets: 0.0,
            total_mv: 0.0,
            cash: 0.0,
            day_pnl: 0.0,
            day_pnl_pct: 0.0,
            total_pnl: 0.0,
            total_pnl_pct: 0.0,
            exposure: 0.0,
            hhi: 0.0,
            top3_sector: 0.0,
        }).await);
    }

    /// clip_by_lines 三态：短文本原样 / 多行超限按行截 / 单行超限硬截
    #[test]
    fn clip_by_lines_three_states() {
        assert_eq!(clip_by_lines("短\n文本", 100), "短\n文本");
        let long = format!("{}\n{}\n{}", "a".repeat(10), "b".repeat(10), "c".repeat(10));
        let clipped = clip_by_lines(&long, 25);
        assert_eq!(clipped, format!("{}\n{}\n…（已截断）", "a".repeat(10), "b".repeat(10)));
        let single = clip_by_lines(&"x".repeat(50), 20);
        assert_eq!(single, format!("{}…截", "x".repeat(16)));
    }
}