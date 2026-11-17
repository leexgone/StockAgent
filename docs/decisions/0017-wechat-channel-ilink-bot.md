# 0017 · 微信通道接入：iLink Bot 双 manager + CLI 扫码绑定（C4 关闭）

- **状态**：接受
- **日期**：2026-11-17
- **决策者**：主人拍板（范围 / 绑定形态 / 交付节奏三项），Claude 协作探查与实施

## 背景

C4 是最后一个待拍板决策闸门（`07`），两问悬而未决：① 微信能否**双向会话**（产品要求会话入口，不只是推送）② 是否需要**公网入站**（企业微信自建应用回调需公网地址或内网穿透）。方向早已拍板（ADR-0011，2026-11-07）：**OpenClaw 式个人微信助理，非公众号/服务号**；接入方案归 robit 工程解决，子木只消费 `PlatformAdapter` 面，模块到货后按 ADR-0011 形态换泛型参数。QQ 验证通道已随 P7-T2 落地，验证了「Web 只是通道之一」的抽象成立。

**robit v0.3.0 到货**（2026-11-17 发版，crates.io 在线核验）：新 crate `robit-wechat` = 微信官方 **iLink AI Bot**（`ilinkai.weixin.qq.com`，官方 npm 包 `@tencent-weixin/openclaw-weixin` 的等价 Rust 移植，协议 SDK `weixin-agent 0.3`）——**不是**服务号、**不是** gewechat/wechaty/pad 协议代理。C4 两问就此有了答案：**① 双向会话 = 能**（1:1 私聊，`chat_id` 恒 `private:{ilink_user_id}`，无群聊）；**② 公网入站 = 无需**（入站长轮询 getUpdates 是出站连接，出站 sendMessage 走 HTTP）→ **C4 关闭，无需安全 ADR**（ADR-0005 内网监听面不变）。

升级面已逐行核验（`git diff v0.2.1..v0.3.0`）：全 additive——`PlatformAdapter` trait / `ChatbotManager::new` 六参签名不变，robit-web wire 零变化，存储 v7 零迁移，robit-agent/qq/web 源码零 diff 纯版本跟随，rusqlite 0.42 无连带；QQ 消费代码零适配。子木侧骨架全在：`BoundOnly` 装饰器、`channel_bindings` 表/端点（channel 列无 CHECK，"wechat" 词汇已预留）、`ChannelFlags.wechat` 开关、调度器推送 trait 全部通道无关。

wechat 屏是 P5-T2 唯一余屏（gate C4）。其「推送订阅」卡背后的订阅体系（6 类订阅 + 时段 + 静音标的）**零落库**（无表无端点，纯原型 localStorage），且 vision（`01:33`）与原型（price/pnl/news/advice/daily/confirm）两套订阅词汇冲突未决——本轮不做，登记 C15 另案。

## 决定

1. **D1 · C4 定案 = 微信官方 iLink AI Bot**：消费 `robit-wechat 0.3.0`（五件 robit 依赖同步 bump 0.3.0）。仅 1:1 私聊；一次性扫码换长期 `bot_token`；无 app_id/app_secret、无开发者资质门槛。原型服务号文案（AppID/Token/Webhook）作废。
2. **D2 · 双 manager 同 working_dir**：同进程两个 `ChatbotManager`（单泛型，一平台一 manager）共享 config 与 `AgentCore` 三 Arc；同 `backend/` 共 robit.db，会话按 `(chat_id, source, namespace)` 隔离（robit 0.2.0 schema v7，R8④ 收口）——**ADR-0011 后果段「第二 Manager 拆 working_dir → memory.md 分叉」警示的前提解除**，不拆 working_dir，人格 `agent.md` 与记忆 `memory.md` 跨通道同源（拍板⑧）。chatbot crate 内部不取目录锁，单进程双 manager 无锁冲突。
3. **D3 · 绑定流程 = CLI 扫码 + Web 白名单管理**：部署侧 out-of-band 跑 `robit-wechat -w <backend> login`（交互式终端二维码 + 配对码）换长期 bot_token 落 `backend/.robit/wechat-state.json`；**扫码不上 Web**。Web 绑定卡 = 通道无关白名单管理（绑定/解绑/状态展示，QQ 同样受益）。**D3 补记（2026-11-17 真机走查反馈，主人拍板）**：首绑 chat_id 不再靠抄日志——`BoundOnly` 丢弃未绑定来消息时顺手记入**待绑定观察队列**（宿主内存态：去重累计 hits + 消息摘录 ≤40 字，上限 20 条，重启清零，不落库），新端点 `GET /api/channel_bindings/pending` 透出，绑定卡自动列出 + 一键绑定（kind 由 chat_id 前缀推断），绑定成功即移出队列。**观察自动化、授权仍人工**：入列 ≠ 放行，绑定点击仍是主人明示动作，fail-closed 闸门不变；日志案底保留作兜底。robit 零改动（记录点在子木自己的装饰器丢弃路径）。**停服登录是硬约定**：已核实 `WechatState::save` 非原子（doc 注释写 atomic，实为 `fs::write` 整文件覆盖，robit-wechat state.rs:58-67），且 zimu-server 不持 DirectoryLock（锁只在 robit-wechat bin）——健康运行期 adapter 每次 sync_buf/context_token 更新都整文件重写，并发登录写入的新 token 会被旧内存快照整体覆盖。**token 失效后 adapter 静默**（poll 已 cancel、无入站即无 state 写），失效态不停服登录是安全的，且可被 D4 探测自动恢复。state 文件含凭据，已被 `**/.robit/*` gitignore 覆盖，**不加豁免**。
4. **D4 · token 失效终态的 supervisor 策略 = 停止快速重建 + 低频探测恢复**：检测纯推断、robit 零改动——wechat adapter 只在 errcode -14（token 失效）时发 `Disconnected`（platform.rs:331-341），manager 对 `Disconnected` 返回 `Ok(())`（manager.rs:243-246），shutdown 也返回 Ok ⇒ **`manager.run` 返回 Ok 且 stop 未置位 = token 失效终态**。策略：停止快速重建（同 token 重建无意义）→ error 日志给出重扫指引 → **60s 低频只读探测**（重解析 `WechatConfig::from_config`，比对 bot_token 与失效值）→ token 变化即重置退避自动重建（重扫后 ≤60s 零接触恢复）。局限如实记：同值重登录、config override 未更新两种情形探测不可识别。其余 Err/非失效退出仍走 QQ 同款 1s→30s 退避重建。
5. **D5 · 推送参数化（caps 驱动）**：宿主 `push_to_bound` 泛型化——通道词汇取 `T::platform_name()`、清洗与截断参数取 `T::capabilities()`，零硬编码通道词汇；**QQ 数值行为逐位不变**（`caps.qq()` 的 features 与 2000 上限单测钉死回归）。复合 pusher = 每通道闭包顺序执行取 OR，`SchedulerCore::new` 签名不动。推送失败不自动重试（ADR-0011 后果段不变）；15:05 仍只推数字摘要、不做 `inject_message` 触发回合（ADR-0011 本轮边界不变，新能力消费另案拍板）。微信纯文本投影 = robit 清洗器 + `MarkdownFeatures::wechat()`（全 false → 可读纯文本）+ 2000 字按行截断（不切半行）。
6. **D6 · Health 契约面**：加 `wechat_online`（照 `qq_online` 口径）；**同批加 `qq_enabled`/`wechat_enabled`**（additive，启动时 `[zimu.channels]` 快照）——`02` 通道卡「启用开关界面只读展示」规格要求区分「未启用」与「离线」，只有 online 位会把未启用误标异常。此为唯一超出 `wechat_online` 的契约增项。
7. **D7 · 订阅体系另案（C15）**：wechat 屏推送订阅卡显示**待启动态**；「当前生效」段如实列两条真实推送（`alert_push` 预警 / `post_market_review_push` 15:05 盘后数字摘要，绑定即推、无订阅过滤）。**不枚举六类订阅标签**——vision 与原型词汇冲突未决，渲染任一即提前站队。`docs/03` 本轮零改动（微信推送复用现有口径；kind 词汇全量对齐归 C15）。
8. **D8 · robit 缺口批记 R13，子木零绕过**：① 微信入站媒体由 adapter 自下载（AES-CDN 解密落盘、`local_path` 预填），manager 见 local_path 跳过通用下载分支 → **绕过 `max_media_bytes` 守卫**；② 语音 silk 无转写，对模型不可消费；③ Image MIME 恒 `image/jpeg`（png 也标 jpeg）；④ `WechatState::save` 非原子 + 登录与运行无互斥（D3）。框架级缺口先改 robit（ADR-0005 协作纪律），不写宿主绕过补丁；绑定白名单限缩风险面（只有主人绑定的会话能发附件），接受。

## 选项与取舍

- **渠道选型**：iLink AI Bot（**选**——官方通道无封号风险面、出站长轮询无需公网回调、无资质门槛、与 OpenClaw 拍板方向同族）；微信服务号/公众号（否——2026-11-07 已拍板非服务号，且模板消息/客服消息形态与个人助理不符）；商业 iPad/Mac 协议代理（gewechat/wechaty 系，否——封号风险 + 需公网回调，robit 选型调研 `robit/docs/robit-plan-wechat-bot.md` 明确排除）。
- **双 manager 布局**：同 working_dir 共库（**选**——R8④ `(source, namespace)` 隔离已收口，人格/记忆同源是拍板⑧的结构前提）；拆 working_dir 防共库（否——警示写于 0.2.0 之前，前提已变；拆分反而造成 memory.md/agent.md 分叉）。
- **绑定流程**：CLI 扫码 + Web 白名单管理（**选**——robit 零改动、符合「接入方案归 robit」拍板、停服约定可控）；Web 扫码流程（本轮否——robit 无非交互式登录库 API，`run_login` 绑死 stdin/stdout；作为 R13 后续增强候选移交，到货后另案）。
- **token 失效策略**：低频探测恢复（**选**——只读探测零副作用，重扫后 ≤60s 零接触恢复）；快速退避重建（否——同 token 重建是无意义请求 + 日志刷屏 + 服务端限频面）；彻底停摆等人工重启（可行但运维差——重扫后还要重启服务）。
- **推送形态**：caps 参数化裸推（**选**——QQ 同构、嘴不同手相同拍板⑦、数字摘要不经模型转写）；`inject_message` 走模型组织语言（本轮否——ADR-0011「IM 注入触发回合本轮不做」边界不变；有档案但引入模型延迟与数字转写面，另案拍板）。
- **Health 面**：online + enabled 三键（**选**——规格要求只读展示启用开关，additive 成本低）；仅 wechat_online（否——通道卡无法区分未启用与离线）。
- **本轮范围**：通道接线 + 屏移植、订阅卡待启用（**选**——主人拍板；C4 与 P5-T2 一并收口）；全量含订阅体系（否——词汇冲突未决、schema v5 + 03 口径定稿周期长，登记 C15）。

## 后果

**正面**：C4 关闭、P5-T2 七屏齐；微信双向会话 + 预警/盘后摘要双通道推送；QQ 绑定管理从此有界面（通道无关白名单卡）；推送链去 QQ 硬编码后第三通道（若将来有）只需换泛型参数；token 失效重扫后零接触恢复。

**负面 / 接受的成本**：
- **冷首发未验证**：对从未对话过的 peer（无 `context_token`）主动推送是否被服务端接受，robit 仓库内无验证记录——真机冒烟首测项；失败则 push_log 如实留痕（不重试），文档落「先对话后推送」指引。
- **登录是部署侧 CLI**：扫码不在 Web 完成，运维多一步 out-of-band 操作；停服登录是硬约定（D3）。
- **token 失效需人工重扫**：无自动重登录；探测以 token 值变化为信号，同值重登录/config override 未更新不可检测（D4 局限）。
- **2000 字截断不分段**：长摘要/回复被截（caps 驱动按行截断 + 省略标记）；微信纯文本无 Markdown、不支持原地编辑。
- **入站媒体无大小守卫**（R13①）：白名单内 peer 可落大文件，磁盘增长面；media/ 已 gitignore，保留策略可选启用 `[app.bot.media]`。
- **wechat group 绑定惰性**：后端端点词汇向前兼容不拒 wechat+group，但 adapter 只产 private chat_id——此类绑定永不匹配；前端表单选 wechat 时 kind 锁死 private。
- **依赖树新增**：weixin-agent 0.3 / qrcode / clap / tokio-util（robit-wechat lib 消费即编译 login 模块依赖，仅编译成本）。

**以后要改回来得动哪里**：换渠道 → 新 ADR 取代本条 + `wechat_channel.rs` 换 adapter；Web 扫码 → robit 补非交互式登录 API（R13）后前端绑定卡加扫码流程 + 本条 D3 修订；订阅体系 → C15 拍板（先对齐词汇）→ schema v5 + 端点 + 03 §12 口径 + push 卡翻真；inject_message 推送 → 另案 ADR + 复合 pusher 换实现（SchedulerCore 不动）；拆 working_dir → 须重新审视人格/记忆分叉（ADR-0011 拍板⑧）。