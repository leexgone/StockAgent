# 0022 · 启用 robit 计划任务（仅 IM 通道）

- **状态**：接受
- **日期**：2026-11-24
- **决策者**：主人拍板（启用 robit 计划任务；范围仅 IM 通道），Claude 协作探查与实施

## 背景

robit 0.7.0（2026-11-24 发版）落地计划任务 M1（设计件 `robit/docs/robit-rfc-scheduled-tasks.md`）：用户可在 IM 会话里用自然语言创建提醒/周期任务（「每天 21:00 提醒我复盘」），到点由宿主级 Scheduler 自主触发 agent 回合，经 `inject_message` 投递回任务所属会话。M1 官方接线只在 Bot 路径——`[app.schedule] enabled=true` 后 `ChatbotManager::new` 自开 ScheduleStore（同 robit.db 第二连接）、`run` 自 spawn Scheduler（shutdown 先停调度再排水）、逐会话 Agent 自挂 `with_schedule`，**宿主零代码**。Web/CLI 的 raw-Agent 接线在 robit 路线图属 M2（`FrontendMessage::ScheduledInput` + 投递 helper，尚未发布）。

子木产品形态是常驻 Web 服务 + IM 双向入口，提醒/周期任务是 IM 助理的自然能力面（OpenClaw 式个人助理）。主人拍板启用。

## 决定

1. **D1 · 范围仅 IM 通道（QQ/微信）**：消费 M1 官方接线，启用 = 纯配置（`[app.schedule] enabled=true`）。Web/CLI 不预建接线——共享内核会注册 schedule_* 四件工具，但无调度上下文，调用如实报「Scheduled tasks are not available in this session」（robit 钉死行为）；人格注明引导主人去 IM 创建。Web 接线等 robit M2 官方发布再消费，不做抛弃性自接线。
2. **D2 · 缺省启用**：`deploy/config.example.toml`（新装播种源）与开发 config 加 `[app.schedule] enabled = true`，其余字段吃 robit 缺省（missed_policy=latest / min_interval_secs=60 / max_active_tasks=50/归属 / delivery_retries=3）。**既有 NAS 卷 config 永不覆盖**（entrypoint 播种语义）——升级后须手工补这一节再重启（docs/08 写明）。人格补强同款语义（卷内不覆盖，工具描述自带核心纪律，功能不依赖人格补强）。
3. **D3 · 信任边界明示（ADR-0014 的时间维延伸）**：任务触发 = **无人值守自主回合**，工具自动执行（auto_approve=true 下 schedule_create/edit 确认按 robit 设计直接放行，无 schedule 专属豁免，RFC §8 明示）。承接面不变：任务只能由主人经**绑定 IM 会话**创建（入站白名单 fail-closed，ADR-0011）；人格纪律仍是唯一软约束——**自主回合不扩张授权**（破坏性操作仍须主人明示、数字纪律不变、禁读凭据照旧，人格新增计划任务纪律条）；robit 缺省护栏（60s 最小间隔、50 任务/归属、投递重试上限）接受。
4. **D4 · 停机补触发与重试缺省接受**：missed_policy=latest——重启后每任务补最近一次 missed（catch_up=true），不风暴补发；投递失败退避 5s/30s/120s 三次，终败 failed 留痕、任务仍 active（next_run 已推进）。delivered = 交给通道，非回合产出成功（产出以会话事件为准）。
5. **D5 · 与子木行情调度独立并存**：SchedulerCore（zimu.db alerts/equity_snapshots，行情预警/盘后链）与 robit Scheduler（robit.db scheduled_tasks/task_runs，v8 自动迁移）独立存储、独立循环、独立关停——`ZIMU_SCHEDULER_DISABLE` 只停行情调度，计划任务随 `[app.schedule]` 开关。双 manager 共库靠 `(source,namespace)` 归属隔离 + 认领事务（同归属双进程也不双触发，崩溃认领启动对账）。

## 选项与取舍

- **Web 自接线（否）**：宿主级 Scheduler/ScheduleDelivery/wrap_prompt 虽全公开，但投递回路/管理面是 robit M2 领地——自接线大概率随 M2 重写，违背「框架级缺口归 robit、不预建抛弃性工作」约定（R16 同款取舍）。
- **缺省关 opt-in（否）**：主人拍板启用；信任面在既有边界内（绑定白名单 + 人格纪律），不是新暴露面。fail-closed 约定针对通道凭据（`[zimu.channels]`），计划任务是通道内能力。
- **schedule 专属确认闸（否）**：robit 无此机制（auto_approve 统一覆盖）；重开确认是 ADR-0014 既有拍板的全局问题（须先补 C13），不为 schedule 单开。
- **missed_policy=skip（否）**：latest 补最近一次对提醒场景更友好（NAS 重启后不漏最近提醒），风暴风险已被「每任务恰一次」消除。

## 后果

**正面**：IM 助理获得提醒/周期任务能力（自然语言创建 + /schedule 直管）；宿主零接线代码（config-only）；与行情调度互不干扰；回滚 = config 翻 false 即全关（工具不注册、调度器不启）。

**负面 / 接受的成本**：

- 自主回合无人值守执行工具（auto_approve）——信任边界扩张明示于 D3，人格纪律承接；任务 prompt 由主人创建时的明示驱动，注入面限于绑定会话。
- Web/CLI 工具可见但不可用（如实报错）——M2 前的诚实降级，人格引导去 IM。
- 既有 NAS 卷须手工补 config 节（播种永不覆盖的既有代价，同人格/config）。
- 注册面 +4（schedule_* 随配置），工具数文案连带（26–32 随配置）。

**以后要改回来得动哪里**：config.example.toml 与 dev config 删 `[app.schedule]` 节（或 enabled=false）、人格删计划任务纪律条、assembly STANDARD_TOOL_NAMES 还原 12、docs/03 §12 计划任务小节与 02/08 注记还原，并置本 ADR 已取代。Web 接线消费（robit M2 发布后）另案拍板。