//! 通道推送出站（T6/ADR-0011 → ADR-0017 D5 caps 参数化 → ADR-0019 轮内合并
//! → P7-T8 问答推送透传 → ADR-0020 长度全量归 robit）——文案组装（预警
//! N=1 单条 / N≥2 合并一条）、多目标循环、逐目标留痕都在宿主；调度器只说
//! 「推这一批」（scheduler trait 通道无关）。
//!
//! **全文透传不自截**（ADR-0020，2026-11-23 移除宿主 clip_by_lines 自截层）：
//! 三类推送（预警/摘要/问答）长度守卫统一归 robit adapter 透明封装（R16，
//! robit 0.4.1 `outbound::prepare_outbound`：微信 ≤3 段分段逐段过节流、
//! QQ 编辑平台单条截断+省略号；≤cap 幂等直通。用户拍板不在宿主复刻分段器）。
//! robit 0.5.1 起 `prepare_outbound` 扩展为「内容投影 + 长度守卫」（R17 T3：
//! 表格线性化 + 纯文本投影，先投影后分段）——本链显式清洗器调用保留，
//! adapter 侧二次投影幂等无操作（线性化输出无表格语法残留）。
//!
//! **零硬编码通道词汇**：通道名取 `T::platform_name()`（恰为 channel_bindings
//! 的 channel 词汇），投影参数取 `T::capabilities()`——QQ 数值行为逐位
//! 不变（caps.qq 的 features/2000 上限有回归钉单测），微信纯文本投影自动成立
//! （MarkdownFeatures::wechat() 全 false → 可读纯文本）。
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
///
/// **全文透传不自截**（ADR-0020）：长度守卫归 robit adapter 透明封装
/// （R16：微信超长自动分段 ≤3 段、每段独立过节流，段失败放弃余段并返回
/// 已发段成功——push_log ok=true 意味至少首段送达，段级细节在 robit 日志；
/// QQ 编辑平台单条截断+省略号）。0.5.1 起同一守卫先做内容投影（R17 T3，
/// 表格线性化）再分段。本函数随 robit 升版零改动。
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
    // （QQ 只有 code_blocks；微信全 false → 纯文本投影，0.5.0 起表格线性化
    // R17）；**不截断**——长度归 robit adapter 守卫（R16/R17 T3，先投影后
    // 分段；adapter 侧二次投影幂等无操作，ADR-0020）。
    let caps = T::capabilities();
    let projected = robit_chatbot::markdown::prepare_markdown_for_platform(text, &caps.markdown_features);
    // 清洗器非快速通道恒在末尾追加一个换行（robit markdown.rs `trim_end() + "\n"`）
    // ——剥掉这个排版尾部换行（非内容截断）：预警/摘要消息字节钉逐位不变
    // （旧 clip_by_lines 的 lines() 重构曾隐式剥掉它），IM 消息也不带多余尾空行。
    let body = projected.trim_end_matches('\n');
    let mut any = false;
    for t in targets {
        let (ok, note): (bool, String) = match active.send(&t.chat_id, body).await {
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

/// 预警推送文案（原 main.rs 装配处抽出，数字 = 判定产出原值不二次加工）：
/// 返回 (消息正文, 留痕主题)。
pub fn alert_message(a: &NewAlertInfo) -> (String, String) {
    (
        format!("子木预警 · {} {}", a.name, a.text),
        format!("预警 {} {}", a.code, a.alert_type),
    )
}

/// 预警推送文案 · 批面（ADR-0019 轮内合并）：N=1 逐位保持单条格式
/// （`alert_message`，fixture 兼容）；N≥2 合并为一条多行消息——标题行
/// `子木预警 · 本轮 {n} 条` + 逐行 `{序}. {name}（{code}）{text}`（含 code
/// 便于回查 alerts 表）。**行序 = 判定产出序，不按 level 重排**（level 是
/// 方向/动作词汇非严重度轴，03 §11/§12 词汇纪律）。留痕主题 `预警批次 {n} 条`
/// （不带 code 列表——subject 复制进每个目标的 push_log detail 行，N 大时
/// 膨胀日志；单条 code 可由 alerts 表同 ts 前缀回查）。超长全文透传归 robit
/// 长度守卫（ADR-0020：微信 ≤3 段分段 / QQ 单条截断+省略号），超出部分在
/// alerts 表与 Web 可查。
pub fn alert_batch_message(alerts: &[NewAlertInfo]) -> (String, String) {
    match alerts {
        [one] => alert_message(one),
        many => (
            format!(
                "子木预警 · 本轮 {} 条\n{}",
                many.len(),
                many.iter()
                    .enumerate()
                    .map(|(i, a)| format!("{}. {}（{}）{}", i + 1, a.name, a.code, a.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            format!("预警批次 {} 条", many.len()),
        ),
    }
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

/// 单源闭包（ADR-0020）：调度器复合 pusher 与手动问答推送共用同一工厂——
/// 链路恒全文透传（长度归 robit，R16/R17），不再有 Clip/透传形态分叉。
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

/// 复合预警 pusher（ADR-0019 批面）：收本轮全部新预警（≥1 条），文案按
/// `alert_batch_message` 合并组装恰一次（N=1 单条格式 / N≥2 合并一条），
/// 逐通道顺序执行取 OR（个人规模 ≤2 通道，顺序保证 push_log 时序确定、
/// store 锁竞争最小）。空 vec → false（未启用任何通道 = 未接线语义）。
pub fn combine_alert_pushers(channels: Vec<ChannelPush>) -> AlertPusher {
    Arc::new(move |batch: Vec<NewAlertInfo>| {
        let channels = channels.clone();
        Box::pin(async move {
            let (msg, subject) = alert_batch_message(&batch);
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

    /// 微信推送：Markdown 剥成纯文本 + **超长全文出站不自截**（ADR-0020，
    /// 长度归 robit adapter——mock 不模拟分段），留痕 channel="wechat"
    #[tokio::test]
    async fn wechat_push_cleans_markdown_keeps_full_text() {
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
        // ADR-0020：宿主零截断——尾行必须在、无截断标记（分段是 robit adapter 的事）
        assert!(body.contains("第199行"), "尾行必须在——不做宿主自截");
        assert!(!body.contains("已截断"), "透传形态无截断标记");
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
        assert!(alert(vec![info.clone()]).await);
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
        let body = wx.sent.lock().unwrap()[1].1.clone();
        assert!(body.starts_with("子木盘后摘要 2026-11-17\n总资产 290362.30\n"));
        assert!(body.contains("当日盈亏 +1234.56（+0.43%）"));
        assert!(body.contains("累计盈亏 -5678.90（-1.92%）"));
        assert!(body.contains("现金 90362.30 · 仓位 68.9% · 集中度 0.25"));

        // 空 vec = 未接线语义
        assert!(!combine_alert_pushers(vec![])(vec![info]).await);
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

    /// ADR-0019 合并格式钉：N=1 与 alert_message 逐字节相等；N=3 正文精确钉
    /// （标题行 + 逐行含 code）、subject「预警批次 3 条」、**行序 = 输入序**
    /// （用非字典序输入钉「不按 level/名称重排」）
    #[test]
    fn alert_batch_message_formats() {
        let one = NewAlertInfo {
            id: 1,
            code: "600519".into(),
            name: "贵州茅台".into(),
            alert_type: "跌破成本".into(),
            level: "down".into(),
            text: "现价 105.00 低于成本 110.05，浮亏 -4.59%".into(),
        };
        assert_eq!(alert_batch_message(std::slice::from_ref(&one)), alert_message(&one));

        let mk = |id: i64, code: &str, name: &str, t: &str, level: &str, text: &str| NewAlertInfo {
            id,
            code: code.into(),
            name: name.into(),
            alert_type: t.into(),
            level: level.into(),
            text: text.into(),
        };
        // 输入序刻意非字典序（涨跌停/成本/放量混排）：输出必须原样保序
        let batch = vec![
            mk(2, "000858", "五粮液", "触及止损线", "down", "现价 118.20 已触及止损线 120.00"),
            mk(3, "601012", "隆基绿能", "放量", "down", "当日累计量为近20日均量的 2.8 倍，涨跌幅 -6.20%"),
            mk(1, "600519", "贵州茅台", "跌破成本", "down", "现价 105.00 低于成本 110.05，浮亏 -4.59%"),
        ];
        let (msg, subject) = alert_batch_message(&batch);
        assert_eq!(
            msg,
            "子木预警 · 本轮 3 条\n\
             1. 五粮液（000858）现价 118.20 已触及止损线 120.00\n\
             2. 隆基绿能（601012）当日累计量为近20日均量的 2.8 倍，涨跌幅 -6.20%\n\
             3. 贵州茅台（600519）现价 105.00 低于成本 110.05，浮亏 -4.59%"
        );
        assert_eq!(subject, "预警批次 3 条");
    }

    /// ADR-0019 端到端：3 条预警一批 → 每通道恰收 **1 条**合并消息（不是 3 条），
    /// push_log 每通道一行、kind 恒 alert_push、detail 含批次主题
    #[tokio::test]
    async fn batch_push_sends_one_merged_message_per_channel() {
        let store = tmp_store("push-batch");
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
        let alert = combine_alert_pushers(channels);

        let mk = |id: i64, code: &str, name: &str, t: &str, level: &str, text: &str| NewAlertInfo {
            id,
            code: code.into(),
            name: name.into(),
            alert_type: t.into(),
            level: level.into(),
            text: text.into(),
        };
        let batch = vec![
            mk(1, "600519", "贵州茅台", "跌破成本", "down", "现价 105.00 低于成本 110.05，浮亏 -4.59%"),
            mk(2, "000858", "五粮液", "触及止损线", "down", "现价 118.20 已触及止损线 120.00"),
            mk(3, "601012", "隆基绿能", "放量", "down", "当日累计量为近20日均量的 2.8 倍，涨跌幅 -6.20%"),
        ];
        assert!(alert(batch).await);

        // 每通道恰 1 条合并消息（合并形态生效）
        let sent = qq.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "qq 通道应恰收 1 条合并消息");
        assert!(sent[0].1.starts_with("子木预警 · 本轮 3 条\n"), "qq 正文形态：{}", sent[0].1);
        assert!(sent[0].1.contains("3. 隆基绿能（601012）"), "逐行含序号+code");
        drop(sent);
        let sent = wx.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "wechat 通道应恰收 1 条合并消息");
        assert!(sent[0].1.starts_with("子木预警 · 本轮 3 条\n"), "wechat 正文形态：{}", sent[0].1);
        assert!(sent[0].1.contains("3. 隆基绿能（601012）"), "逐行含序号+code");
        let rows = logs(&store);
        assert_eq!(rows.len(), 2, "每通道每绑定一行留痕");
        assert!(rows.iter().all(|r| r.kind == "alert_push"));
        assert!(rows.iter().all(|r| r.detail.contains("预警批次 3 条")));
    }

    /// 问答推送（P7-T8 / ADR-0020 单一透传形态）：超长文本**全文出站不自截**
    /// （长度守卫归 robit adapter，R16）；Markdown 清洗仍生效
    #[tokio::test]
    async fn chat_push_sends_full_text_without_clipping() {
        let store = tmp_store("push-passthrough");
        bind(&store, "wechat", "private:WXME");
        let mock = WechatMock::with(vec![]);
        let active = ActiveAdapter::<WechatMock>::default();
        active.set(mock.clone());

        // 200 行 × 20 字符 ≈ 4200 字符 > 2000；首行带 bold 标记（清洗仍生效）
        let mut text = String::from("**加粗**开头\n");
        for i in 0..200 {
            text.push_str(&format!("第{i:03}行内容十字符号填充\n"));
        }
        let ok = push_to_bound::<WechatMock>(
            &store, &active, "chat_push", &text, "问答推送 2026-11-21 09:03",
        )
        .await;
        assert!(ok);
        // 锁只活在语句内（clone 出来再断言）——跨 await 持锁禁止
        let sent = mock.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 1, "透传 = 单条全文（分段是 robit adapter 的事，mock 不模拟）");
        let body = &sent[0].1;
        assert!(body.contains("第199行"), "尾行必须在——不做宿主自截");
        assert!(!body.contains("**"), "Markdown 清洗仍生效");
        assert!(!body.contains("已截断"), "透传形态无截断标记");
        let rows = logs(&store);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "chat_push");
        assert!(rows[0].ok);
    }
}