//! QQ 通道接线（T5/ADR-0011）：`ChatbotManager<BoundOnly<QqPlatformAdapter>>` 挂进
//! zimu-server 常驻进程——IM 入口与 Web 入口共享同一装配三件套（能力一致性拍板⑦），
//! 会话按 chat_id 独立（拍板①），入站过绑定白名单（拍板⑤）。
//!
//! 生命周期两层，**职责不同不要混**：
//! - `QqPlatformAdapter` **自带重连 supervisor**（robit-qq platform.rs:137/164——
//!   网关约 30 分钟轮换无感重连，不发 `Disconnected`）：连接层的自愈归它；
//! - 本文件的循环只兜**残余退出路径**（`manager.run` 因 recv Err/Disconnected 返回、
//!   或构造失败）：退避重建整个 manager（1s→30s 封顶，形状照 robit reconnect_backoff）。
//!   （R8 记录：若 robit 后续在 run 循环里内置监督，本层可退役。）
//!
//! 两道 fail-closed 闸门（拍板⑥）：`[zimu.channels].qq` 未启用 → 本模块根本不 spawn；
//! 启用但凭据未解析（`${…}` 残留，robit from_config 对 channels 原样拷贝不校验）→
//! 每次 run_once 拒绝并退避，绝不让坏值拖到换 token 时才炸。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use robit_chatbot::ChatbotManager;
use robit_qq::{QqConfig, QqPlatformAdapter};
use tokio::sync::Notify;

use crate::agent_bridge::AgentBridge;
use crate::channel::ChannelHandle;
// bin 模块引 lib 目标：走 zimu_server:: 路径（crate:: 是 bin 自己，见 agent_bridge 同款）
use zimu_server::channel_adapter::BoundOnly;
use zimu_server::holdings_api::SharedStore;

/// 在线连接槽（T6 推送侧的"只推在线的"）：泛型槽收口在 lib `channel_push`
/// （ADR-0017 D5 caps 参数化），QQ 是具体别名——main.rs 调用面不变。
pub type ActiveAdapter = zimu_server::channel_push::ActiveAdapter<QqPlatformAdapter>;

/// 前提（main.rs 已判）：`[zimu.channels].qq = true` 且 Web 通道在线（共享三件套）。
/// working_dir 与 Web 同源（backend/）——人格 agent.md 与 memory.md 按 working_dir
/// 搜索，同源是 T8 的结构前提（ADR-0011）；微信第二 Manager 同 working_dir 共库
/// （ADR-0017 D2：(source, namespace) 隔离已收口，不拆目录——旧 ZIMU_QQ_WORKDIR
/// 覆盖开关设想作废）。
pub fn spawn(
    bridge: Arc<AgentBridge>,
    store: SharedStore,
    working_dir: PathBuf,
    online: Arc<AtomicBool>,
    active: ActiveAdapter,
) -> ChannelHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(Notify::new());
    let handle = tokio::spawn(run_supervisor(
        bridge,
        store,
        working_dir,
        online.clone(),
        stop.clone(),
        shutdown.clone(),
        active,
    ));
    ChannelHandle::new(stop, shutdown, handle)
}

async fn run_supervisor(
    bridge: Arc<AgentBridge>,
    store: SharedStore,
    working_dir: PathBuf,
    online: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
    active: ActiveAdapter,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match run_once(&bridge, &store, &working_dir, &shutdown, &stop, &online, &active).await {
            Ok(()) => {
                if stop.load(Ordering::SeqCst) {
                    break; // 关停路径的"正常退出"，不重连
                }
                tracing::warn!("QQ 通道事件循环退出（platform Err/Disconnected）：退避 {backoff:?} 后重建");
            }
            Err(e) => {
                tracing::warn!("QQ 通道启动/运行失败：{e:#}：退避 {backoff:?} 后重试");
            }
        }
        online.store(false, Ordering::SeqCst);
        active.clear(); // 推送侧此刻起 send 返回 None（未连接如实失败）
        if stop.load(Ordering::SeqCst) {
            break;
        }
        // 退避睡眠可被关停即时打断
        tokio::select! {
            _ = shutdown.notified() => break,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
    online.store(false, Ordering::SeqCst);
    tracing::info!("QQ 通道 supervisor 已退出");
}

/// 一次完整的「连 → 建 → 跑」：返回即需要重建（Err = 连/建失败）。
async fn run_once(
    bridge: &Arc<AgentBridge>,
    store: &SharedStore,
    working_dir: &PathBuf,
    shutdown: &Arc<Notify>,
    stop: &Arc<AtomicBool>,
    online: &Arc<AtomicBool>,
    active: &ActiveAdapter,
) -> Result<()> {
    // 第二份 RobitConfig：不可 Clone 且 ChatbotManager::new 按值吃（manager.rs:117）；
    // load_config 重复调用安全（纯文件读，robit-ai config.rs:388）
    let config = robit_ai::load_config(None).context("QQ 通道重读 config.toml 失败")?;
    let qq_cfg = QqConfig::from_config(&config).map_err(|e| anyhow!("QQ 配置缺失：{e}"))?;

    // 闸门二（fail-closed）：占位符未解析就别连——错误在启动面暴露，不等换 token 401
    if qq_cfg.app_id.contains("${") || qq_cfg.app_secret.contains("${") {
        return Err(anyhow!(
            "QQ 凭据未解析（app_id/app_secret 仍是 ${{VAR}} 原文）：把 ZIMU_QQ_APP_ID/ZIMU_QQ_APP_SECRET 补进 backend/.robit/.env"
        ));
    }

    // 连接层自带重连 supervisor（模块头），这里的 connect 只负责首连建立
    let adapter = QqPlatformAdapter::connect(qq_cfg, shutdown.clone())
        .await
        .context("连接 QQ 网关失败")?;
    // 推送侧句柄（T6）：出站走原始 adapter（不过白名单——入站闸门与主动出站不同层）
    active.set(adapter.clone());
    let bounded = Arc::new(BoundOnly::new(adapter, store.clone(), "qq"));

    let manager = ChatbotManager::new(
        bounded,
        config,
        working_dir.clone(),
        bridge.client.clone(),
        bridge.registry.clone(),
        bridge.skills_registry.clone(),
    )
    .map_err(|e| anyhow!("ChatbotManager 构造失败：{e:?}"))?;

    online.store(true, Ordering::SeqCst);
    tracing::info!("QQ 通道已连接：会话按 chat_id 独立管理（ADR-0011 拍板①），入站过绑定白名单（拍板⑤）");

    // 与关停通知赛跑：manager.run 自身 select shutdown（返回 Ok），这里再兜一层
    // 保证 stop 置位后第一时间收束，不等平台事件（Disconnected 实际不触发，见模块头）
    tokio::select! {
        biased;
        _ = shutdown.notified() => {
            stop.store(true, Ordering::SeqCst);
            Ok(())
        }
        res = manager.run(shutdown.clone()) => res.map_err(|e| anyhow!("QQ 事件循环错误：{e}")),
    }
}
