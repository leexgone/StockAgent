# 0014 · 智能体启用 robit 标准工具面：全套消费、确认关闭自动执行

- **状态**：接受（整体推翻 `07` R2「纯领域装配」决策；写账本五件软门禁由 [0012](./0012-agent-ledger-write-tools.md)/[0013](./0013-agent-holding-snapshot-tools.md) 定，本 ADR 不变）
- **日期**：2026-11-14
- **决策者**：leexgone（产品所有者），Claude Code 辅助整理

## 背景

robit 0.2.1 闭环 M1（附件标注行携带本地保存路径），数据文件对模型「路径可见」——但子木工具面 14 件全是账本/行情域，**没有手读文件**：模型能说出「文件在 media/对账单.csv」，却无从读取内容。用户先拍板启用 bash，随后扩大为「**robit 的核心工具都应该启用**」，并两度补充：read_image 按需注册（不因主模型支持图像而剥夺独立 vision 描述能力）；**关闭确认、自动执行**；配置文件遵循 robit 官方配置规则。

R2 的历史顾虑（server 工作目录有 `.env`，通用 read 暴露给 LLM 是安全面）在本轮被用户知情推翻：子木是主人自己的个人助理（OpenClaw 形态），通用文件/shell 能力是产品意图的一部分，不是意外暴露。

## 决定

1. **标准工具面全量消费**：装配走 robit 公开内核 `create_tools_from_config_with(&config, skills_arc, None)`（与 bootstrap 同一实现：截断缺省 500 行/51200 字节/图像最长边 1024、image provider 解析、memory 模式判定、file 模式守卫全内置），领域 14 件随后注册（同名零冲突）。**不用 `bootstrap_with`**——它强引 `~/.robit/skills` 与 `<wd>/.robit/skills` 全局目录，违背子木 skill 单源政策（仓库 `skills/` 为准，个人全局模板不进服务）；内核路径 skill 加载主权完全留子木。
2. **确认关闭、自动执行**：本地 config `[app] auto_approve = true`（robit 官方键）。agent 层跳过全部确认（robit-agent agent.rs:896-930）、QQ Confirmer 直接放行不注册 pending（关键词不再截消息）。write/edit/bash/generate_image 工具层 `requires_confirmation=true` 是 robit 事实，被此键覆盖。**确认回路本轮不承压**，C13 两洞暂不承重（将来若重开确认须先补，robit 修复登记保留）。
3. **read_image 按需注册**：内核因主模型 `supports_images=true` 强制跳过 read_image（bootstrap.rs:361-371，视其冗余）；子木装配层补条件注册——`resolve_vision_model(&config)` 解析成功即注册 `ReadImageTool`（独立 vision 模型描述图片，与 read 的图像注入分支互补而非冗余），未配 `default_vision_model` 则不在册。全 pub API，宿主装配政策，非绕过框架。
4. **条件注册与记忆模式**：generate_image 随 image provider 配置（default_image_model 已配 wanxiang，取决于 BAILIAN_API_KEY 解析）；记忆四件不注册（`memory_mode` 缺省 file，memory.md 形态维持，落盘经 write/edit 自动执行）。
5. **三通道共享**（ADR-0011⑦「嘴不同手相同」）：标准面进唯一装配线即 Web/QQ/CLI 全开，不做按通道 gating。
6. **search_history 库语义与 init_db**：search_history 检索 robit 自持库（IM 会话档案）；Web 会话档案在 zimu.db（ADR-0011 通道独立），不在其内——人格与文档如实说明。Web 通道从不初始化 robit.db（QQ 由 ChatbotManager 自建），装配补一次 `storage::init_db`（幂等）使 search_history 三端可用（Web 侧检索 IM 档案或空结果，不报 no such table）。
7. **配置纪律**：只用 robit 文档化键位与类型（`[app] auto_approve`、`[app.context]`、`default_vision_model`、`default_image_model` 等），不自创键节；`[zimu.channels]` 是既有例外（robit 静默忽略未知节，子木 raw 解析，ADR-0011 拍板⑥）。

### 信任边界扩张（明示，用户知情接受）

read/write/edit **无路径沙箱**（绝对路径直通），bash 任意命令（cmd /C / sh -c，无过滤），确认闸已关——**.env 与凭据可读、任意路径可写、IM 提示注入即执行**。防线收缩为三层软/外围约束：

- **人格纪律**（`prompts/agent.md` 文件与 shell 纪律段，守护测试钉死）：禁读/输出 .env 与凭据；账本写入只走写账本五件，禁止 bash/write/edit/sqlite 直写 zimu.db；禁触交易通道；破坏性操作须主人明示；
- **BoundOnly 白名单**（ADR-0011）：IM 入站只有绑定会话能说话——注入面限于主人自己的会话；
- **域校验**：写账本五件的 store 事务校验与留痕不受标准面影响（绕过路径已被人格纪律禁止，但物理上存在——如实记录）。

写账本五件**软门禁不变**（自动落账+读回+人格纪律）：领域工具域校验+冲正可逆，标准面是主人授意的通用能力，两种形态并存不矛盾。

## 选项与取舍

- **create_tools_from_config_with + 领域注册（选）**：与 bootstrap 同内核零重复，skill 主权保留，条件注册（read_image）可在装配层补。
- **bootstrap_with(extra_tools)（不选）**：强引全局 skill 目录（政策漂移）+ enabled_skills 过滤新生效；SkillRegistry 自建与子木单源加载二选一，迁移成本换不来收益。
- **全手搓 12 件注册（不选）**：重复 provider 解析/memory 判定/截断缺省内核，robit 升级时逐件追签名。
- **受限收读工具 read_media_file（不选）**：对「解析上传文件」相称，但用户要的是通用能力（OpenClaw 形态），不止读文件。
- **保留确认闸（不选）**：用户拍板关闭自动执行；确认回路三端健在（Web 确认框/QQ 关键词/CLI Y/n），将来重开 = config 翻 false + 先补 C13。
- **按通道 gating（不选）**：违反 ADR-0011⑦，收窄某通道能力须显式新决策。

## 后果

- 注册面组成：**14 领域 + robit 标准面 11–13 件**（read/write/edit/ls/find/grep/bash/load_skill/search_history/query_task 恒在；generate_image 随 key、read_image 随 vision 配置），注册 25–27——文案写组成不写死数，tool_names 动态（SSE 徽章 hint 自动）。
- 装配重排：skills 先行（LoadSkillTool 需要 Arc<SkillRegistry>）→ 内核工具面 → read_image 按需 → init_db → 领域注册；`build_registry` 改形 `register_domain_tools`。
- 人格新增「文件与 shell 纪律」段（自动执行语境，纪律是唯一软约束）；persona 守护断言 7→10。
- R2 整体推翻（状态列追加）；C13 暂不承重；**R11 新登记**（robit 工具缺口批：bash GBK 优先解码/截断按字符/超时不杀子进程、robit-web 文档虚指 timeout、Confirmer 无 id 关联、search_history 硬编码 global_storage、find/grep 不读 .gitignore、read_image 强制不注册建议配置可控等）移交 robit。
- 走查纪律升级：write/edit/bash 自动执行**真能写库**——人工走查务必 ZIMU_DB_PATH 指 zimu.db 副本。
- 回滚路径：config `auto_approve = false` 即恢复确认闸（须先补 C13）；标准面整体撤下 = 装配回退 build_registry 旧形（git 一笔 revert）。
