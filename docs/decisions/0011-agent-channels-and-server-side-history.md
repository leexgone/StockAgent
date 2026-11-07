# 0011 · 智能体接入模块：通道独立会话、服务端真值与 QQ 验证通道

- **状态**：接受
- **日期**：2026-11-07
- **决策者**：leexgone（产品所有者），Claude Code 辅助整理

## 背景

产品形态要求「常驻 Web 服务 + 微信/QQ 会话入口」（ADR-0005），且定位为 OpenClaw 式**个人助理**：一个助理、多个 IM 入口（2026-11-07 用户拍板方向，非公众号/服务号形态）。而现状的网页对话智能体是**进程级单例、单会话、无身份、纯内存**：agent 侧历史重启即丢（`chat_messages` 零落库，robit 会话存储库零引用），前端 localStorage 展示档从不回传模型（刷新后「你看到的」与「模型记得的」分叉，B16）；robit 框架约束探查确认**一个 Agent 实例只能挂一个 Frontend**（robit-agent agent.rs:120），多通道必然是多 Agent 实例 + 共享装配三件套（框架自家姿势）。QQ 通道是第一个可立即验证的第二消费者（robit-qq 0.1.28 已在 crates.io）；微信等 robit 平台模块（个人微信接入方案归 robit 工程解决，2026-11-07 拍板）。

## 决定

1. **会话拓扑 = 通道独立**：Web 保持单会话；IM 按 `chat_id` 分会话（robit-chatbot 模型）。不做跨通道会话聚合——网页与微信的对话记录互不相见。
2. **会话真值在服务端**（本轮落地）：Web 会话档案落 `zimu.db` v4 `chat_messages`（user/assistant 文本 + tool 展示行；**thinking 不入库**，与框架「thinking 不写回 history」同口径）；启动时 `Agent::with_history` 复活回灌。前端 localStorage **降级为服务端不可达时的只读展示缓存**（上限 30 条）。IM 会话历史归 robit 自持库（`robit.db` sessions/messages），子木不读写。
3. **工具轨迹存档不回灌**：`tool` 行只为展示与审计（「依据来源」如实可回看）；复活上下文只取 user/assistant——回灌会把行情/持仓陈旧数字喂回模型，违反 03「派生数字一律现算」。
4. **QQ 作验证通道**（子木侧改造 + 现成 `QqPlatformAdapter` 库面消费）：不改 robit、不写微信代码；robit 缺口以 R8 需求清单移交，按共同演进流程另行拍板。
5. **通道配置管理面 = 混合分界**：**连接参数（含凭据与启用开关）只走配置文件**——工程本地 `backend/.robit/config.toml`（`${VAR}` 占位，密钥在 `~/.robit/.env` 与 `backend/.robit/.env`，2026-11-07 配置分层拍板；子木自有节 `[zimu.channels]` 缺省停用 fail-closed）；界面只做脱敏只读状态展示。**绑定关系与推送偏好 = 界面管理、落 zimu.db**（`channel_bindings` 表 + `/api/channel_bindings` 端点）。理由：凭永不进 zimu.db（会被冒烟副本带走）；服务无鉴权不宜 POST 凭据；`QqConfig::from_config` 是 robit 原生消费面。
6. **通道启用开关走配置文件**：`[zimu.channels].qq / wechat`，未启用则**不构造**该通道 adapter / ChatbotManager / 任何会话 Agent。
7. **通道能力一致性——「嘴不同手相同」**：各通道智能体共享同一装配三件套（`zimu_agent::assembly::AgentCore`：LlmClient/ToolRegistry/SkillRegistry），工具面（9 领域工具，含写库类）、Skill 面（/复盘 /盘前 /建议）、确认语义全同；无下单类工具的红线渠道无关。差异只在**投影与交互面**（SSE 结构化帧 vs 聊天文本 + 关键字确认；thinking 仅 Web 展示；QQ 侧 Markdown 按平台子集清洗）。按通道阉割/扩权不是本模块的形态——要收窄某通道能力应是显式的新决策。
8. **主人格提示词入库**：`backend/.robit/prompts/agent.md`（robit 原生覆盖路径 `PromptBuilder::with_working_dir`）——此前子木日常轮实际命中 robit 内置「代码打工」人格；子木身份/边界硬规则/「结论-为什么-怎么做」结构入人格。QQ 通道 working_dir 与 Web 同源（backend/）是人格与记忆文件（memory.md）跨通道同源的结构性前提。

## 选项与取舍

- **Web 档案落 zimu.db v4 vs 复用 robit storage.rs 会话库**：选前者——迁移纪律、备份、读端点全在 zimu-store；robit 会话库是 ChatbotManager 的私有物（source 硬编码 "qq"、查询无平台维度、共库 chat_id 全局唯一索引），子木去写它的表等于制造第二真值源与共库隐患。IM 侧照用 robit 库（单 Manager 无冲突面）。
- **localStorage 退役 vs 降级缓存**：选降级——服务端不可达（重启窗口/离线）时用户仍看得见上一次内容，现有写缓存代码零成本保留；真值归属以服务端为准（水合覆盖缓存，本地清空仅在服务端确认成功后）。
- **IM 会话注入触发回合**：本轮**不做**——ChatbotManager 无公开注入 API（绕法是 adapter 合成事件，会过 Confirmer/斜杠命令解析且与真实消息不可分辨，副作用大）；记 R8。15:05 向 IM 只推**数字摘要**（equity_snapshots 原值），复盘正文留在 Web 会话。
- **通道独立 vs 用户归一**：拍板通道独立（实现最省、语义最清晰）；跨通道聚合（同一个人多入口接续）留作将来的显式扩展，不是默认义务。
- **人格提示词入库 vs 仅本机文件**：拍板入库（产品文案资产与 skills/ 同性质；`.gitignore` 例外只豁免 `prompts/agent.md`）；长期正解是 robit 提供宿主注入 API（R8），文件路是现状唯一路。

## 后果

- 正面：重启不失忆（复活日志实锤「会话复活：从档案回灌 N 条」）；档案即队列（先落库再注入，注入失败的行下次启动回灌补齐）；QQ 接入验证了「Web 只是通道之一」的抽象成立，微信到货时只换泛型参数；`/api/chat/history|reset` + `/api/channel_bindings` 进契约，前端类型同源。
- 代价与接受的边界：`chat_messages` 与 robit `robit.db` 两套会话存储并存（各归各通道，通道独立语义下不互认）；**复活回灌只含文本对**，工具轮次不进模型上下文（robit 同语义，非子木特例）；Web 仍是单会话（多用户/多会话不是本轮义务）；服务无鉴权前提被 IM 打破后，**绑定白名单是唯一入站闸门**（decorator fail-closed：判定失败不应答）；推送失败不自动重试（同日去重已定格，留痕即补救面）。
- 记忆分叉面收束：人格 agent.md 与 memory.md 按 working_dir 搜索——QQ/Web 同源 backend/ 即共享同一份画像（通道独立管的是聊天记录，不是"同一个助理分裂成两个记忆"）；将来第二 Manager（微信）若为防 robit.db 共库而拆 working_dir，其 memory.md 分叉需在当时决策中重新审视（R8 一并记）。
