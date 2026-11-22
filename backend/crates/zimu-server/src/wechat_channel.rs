//! 微信通道接线（P6-T2/ADR-0017）：`ChatbotManager<BoundOnly<WechatPlatformAdapter>>`
//! 挂进 zimu-server 常驻进程——与 QQ 双 manager 并存，共享装配三件套（拍板⑦）、
//! 同 working_dir 共 robit.db（会话按 `(chat_id, source="wechat", namespace)` 隔离，
//! D2：R8④ 收口，不拆目录）、入站过绑定白名单（拍板⑤，装饰器零改动换词汇）。
//!
//! 渠道 = 微信官方 iLink AI Bot（robit-wechat 0.5.1）：出站长轮询无需公网回调，
//! 1:1 私聊（chat_id 恒 `private:{ilink_user_id}`）。登录 = 部署侧 CLI
//! `robit-wechat -w <backend> login`（**停服约定**，D3：state save 非原子 +
//! 运行中无互斥，健康期并发登录会覆盖新 token；失效态 adapter 静默，不停服安全）。
//! 出站反刷（R14/robit 0.4.0）：adapter 内置账号级发送节流（throttle 模块，
//! 微信缺省启用：最小间隔 2s、窗口 10 条/180s、-2 冷却 120s，快照随
//! wechat-state.json 持久化跨重启）——宿主 `push_to_bound` 直调 adapter 自动受护，
//! 子木零适配；-2 瞬态限流码经 `AgentError::RateLimited` 类型化上报。
//!
//! 生命周期两层 + 一个终态分支，**职责不同不要混**：
//! - `WechatPlatformAdapter` **自带 poll supervisor**（robit-wechat platform.rs:378，
//!   传输错误指数退避 1s→30s）：连接层自愈归它；
//! - 本文件循环兜**残余退出路径**（构造失败 / recv Err）：退避重建整个 manager
//!   （1s→30s 封顶，QQ 同款）；
//! - **token 失效（errcode -14）是终态**，重建无意义（state 不变）——推断链：
//!   adapter 只在 `on_token_stale` 发 `Disconnected`（platform.rs:510-514），
//!   manager 对 `Disconnected` 返回 `Ok(())`（robit-chatbot manager.rs:262），
//!   shutdown 也返回 Ok 但 stop 已置位 ⇒ **run 返回 Ok 且 stop 未置位 = token 失效**。
//!   策略（D4）：停止快速重建 → error 日志给重扫指引 → 60s 低频**只读**探测
//!   （重解析 from_config 比对 bot_token）→ 变化即重置退避自动重建（重扫后
//!   ≤60s 零接触恢复）。局限：同值重登录 / config override 未更新不可检测。
//!
//! 两道 fail-closed 闸门（拍板⑥）：`[zimu.channels].wechat` 未启用 → 根本不 spawn；
//! 启用但凭据未解析（config override 的 `${…}` 残留）→ 每次 run_once 拒绝并退避。
//! token 缺失（未登录）由 `WechatConfig::from_config` Err 自然 fail-closed。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use robit_chatbot::ChatbotManager;
use robit_wechat::{WechatConfig, WechatPlatformAdapter};
use tokio_util::sync::CancellationToken;

use crate::agent_bridge::AgentBridge;
use crate::channel::ChannelHandle;
// bin 模块引 lib 目标：走 zimu_server:: 路径（crate:: 是 bin 自己）
use zimu_server::channel_adapter::BoundOnly;
use zimu_server::holdings_api::SharedStore;

/// 在线连接槽（推送侧）：泛型槽收口在 lib `channel_push`（ADR-0017 D5）。
pub type ActiveAdapter = zimu_server::channel_push::ActiveAdapter<WechatPlatformAdapter>;

/// token 失效后的只读探测间隔（D4）：重扫后 ≤60s 自动恢复。
const PROBE_INTERVAL: Duration = Duration::from_secs(60);

/// 前提（main.rs 已判）：`[zimu.channels].wechat = true` 且 Web 通道在线（共享三件套）。
pub fn spawn(
    bridge: Arc<AgentBridge>,
    store: SharedStore,
    working_dir: PathBuf,
    online: Arc<AtomicBool>,
    active: ActiveAdapter,
    pending: zimu_server::pending_bindings::PendingBindings,
) -> ChannelHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(run_supervisor(
        bridge,
        store,
        working_dir,
        online.clone(),
        stop.clone(),
        shutdown.clone(),
        active,
        pending,
    ));
    ChannelHandle::new(stop, shutdown, handle)
}

/// run_once 的退出语义：Stopped = 关停路径；TokenStale = -14 终态（携失效时
/// 使用的 bot_token 供探测比对——**只进内存与日志指引，不落盘不外泄**）。
enum RunOutcome {
    Stopped,
    TokenStale(String),
}

async fn run_supervisor(
    bridge: Arc<AgentBridge>,
    store: SharedStore,
    working_dir: PathBuf,
    online: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    shutdown: CancellationToken,
    active: ActiveAdapter,
    pending: zimu_server::pending_bindings::PendingBindings,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match run_once(&bridge, &store, &working_dir, &shutdown, &stop, &online, &active, &pending).await {
            Ok(RunOutcome::Stopped) => break,
            Ok(RunOutcome::TokenStale(stale_token)) => {
                online.store(false, Ordering::SeqCst);
                active.clear();
                tracing::error!(
                    "微信 bot_token 已失效（errcode -14 终态，无自动重登录）：请重新扫码登录 \
                     `robit-wechat -w <backend> login`（健康运行期须停服，失效态可不停服——\
                     ADR-0017 D3）；重扫后本循环 60s 内探测到新 token 自动重建"
                );
                // 探测循环：只读重解析配置比对 token，变化即恢复；关停可即时打断
                let mut recovered = false;
                while !stop.load(Ordering::SeqCst) {
                    tokio::select! {
                        _ = shutdown.cancelled() => stop.store(true, Ordering::SeqCst),
                        _ = tokio::time::sleep(PROBE_INTERVAL) => {}
                    }
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if probe_new_token(&working_dir, &stale_token) {
                        tracing::info!("检测到新微信 bot_token：自动重建通道");
                        recovered = true;
                        break;
                    }
                }
                if !recovered {
                    break;
                }
                backoff = Duration::from_secs(1);
                continue;
            }
            Err(e) => {
                tracing::warn!("微信通道启动/运行失败：{e:#}：退避 {backoff:?} 后重试");
                online.store(false, Ordering::SeqCst);
                active.clear(); // 推送侧此刻起 send 返回 None（未连接如实失败）
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
    online.store(false, Ordering::SeqCst);
    tracing::info!("微信通道 supervisor 已退出");
}

/// 只读探测（D4）：重解析配置取 bot_token，与失效值不同即视为已重扫。
/// 不写 state、不 connect——零副作用；解析失败（文件半截/缺失）按「未恢复」继续等。
fn probe_new_token(working_dir: &Path, stale: &str) -> bool {
    let Ok(config) = robit_ai::load_config(None) else {
        return false;
    };
    match WechatConfig::from_config(&config, working_dir) {
        Ok(cfg) => cfg.bot_token != stale,
        Err(_) => false,
    }
}

/// 闸门二（fail-closed）：config override 的占位符未解析就别连——错误在启动面
/// 暴露。state 文件来的 token 不会含 `${`（扫码产物），此闸门只兜 override 路径。
fn credential_gate(cfg: &WechatConfig) -> Result<()> {
    if cfg.bot_token.contains("${") {
        return Err(anyhow!(
            "微信凭据未解析（bot_token 仍是 ${{VAR}} 原文）：把对应变量补进 backend/.robit/.env，\
             或移除 [channels.wechat_bot] bot_token 覆盖改用扫码登录（robit-wechat login）"
        ));
    }
    Ok(())
}

/// 一次完整的「连 → 建 → 跑」：返回即需要 supervisor 决策（Err = 连/建失败退避；
/// TokenStale = 终态探测；Stopped = 关停）。
async fn run_once(
    bridge: &Arc<AgentBridge>,
    store: &SharedStore,
    working_dir: &PathBuf,
    shutdown: &CancellationToken,
    stop: &Arc<AtomicBool>,
    online: &Arc<AtomicBool>,
    active: &ActiveAdapter,
    pending: &zimu_server::pending_bindings::PendingBindings,
) -> Result<RunOutcome> {
    // 第二份 RobitConfig：不可 Clone 且 ChatbotManager::new 按值吃（QQ 同款）
    let config = robit_ai::load_config(None).context("微信通道重读 config.toml 失败")?;
    let wx_cfg = WechatConfig::from_config(&config, working_dir).map_err(|e| {
        anyhow!(
            "微信配置缺失：{e}——请先停服运行 `robit-wechat -w <backend> login` 扫码登录\
             （ADR-0017 D3 停服约定），或在 [channels.wechat_bot] 配置 bot_token"
        )
    })?;
    credential_gate(&wx_cfg)?;
    let token_used = wx_cfg.bot_token.clone();

    // 连接层自带 poll supervisor（模块头），connect 只负责首连建立
    let adapter = WechatPlatformAdapter::connect(wx_cfg, shutdown.clone())
        .await
        .context("连接微信 iLink Bot 失败")?;
    // 推送侧句柄：出站走原始 adapter（不过白名单——入站闸门与主动出站不同层，QQ 同款）
    active.set(adapter.clone());
    let bounded = Arc::new(BoundOnly::new(adapter, store.clone(), "wechat", pending.clone()));

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
    tracing::info!(
        "微信通道已连接（iLink Bot）：会话按 chat_id 独立（拍板①），入站过绑定白名单（拍板⑤），\
         与 QQ 共 robit.db 按 source 隔离（ADR-0017 D2）"
    );

    // 与关停令牌赛跑：manager.run 自身收 CancellationToken（返回 Ok），这里再兜一层。
    // Ok + stop 未置位 ⇒ Disconnected ⇒ token 失效终态（推断链见模块头）。
    tokio::select! {
        biased;
        _ = shutdown.cancelled() => {
            stop.store(true, Ordering::SeqCst);
            Ok(RunOutcome::Stopped)
        }
        res = manager.run(shutdown.clone()) => match res {
            Ok(()) => {
                if stop.load(Ordering::SeqCst) {
                    Ok(RunOutcome::Stopped)
                } else {
                    Ok(RunOutcome::TokenStale(token_used))
                }
            }
            Err(e) => Err(anyhow!("微信事件循环错误：{e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(token: &str) -> WechatConfig {
        WechatConfig {
            bot_token: token.into(),
            base_url: None,
            cdn_base_url: None,
            working_dir: PathBuf::from("."),
            // robit 0.4.0 新字段：保守缺省（节流启用）——闸门测试只看 bot_token
            rate_limit: robit_chatbot::throttle::ThrottleConfig::default(),
            progress_hint: None,
        }
    }

    #[test]
    fn credential_gate_rejects_unresolved_placeholder() {
        let err = credential_gate(&cfg("${ZIMU_WECHAT_BOT_TOKEN}")).unwrap_err();
        assert!(err.to_string().contains("凭据未解析"));
    }

    #[test]
    fn credential_gate_passes_real_token() {
        assert!(credential_gate(&cfg("ilink-token-abc123")).is_ok());
    }
}