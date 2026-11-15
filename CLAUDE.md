# CLAUDE.md — 子木 · 股票智能助理（StockAgent）

个人股票助理程序。当前阶段：**P5 前端移植推进中（T1 工程约定 ✅ 2026-10-29，ADR-0008；T2 逐屏移植 🔨——index ✅、dashboard ✅、holdings ✅、analysis ✅、chat ✅、diagnosis ✅ 2026-10-30（手写 SVG 图表组件折线/蜡烛/得分弧/迷你走势/环形 + 自选卡收口 B13 前端半边 + 加减仓抽屉接 API 收口 P4-T1 + 分析建议读 advice_history 留档绝不伪造 + chat 真实 Agent SSE 三卡 + diagnosis 风险指标走 perf 黄金向量字段；B10/B11/B12 已拍板定稿 2026-10-30——B12 奇零股清仓豁免、B11 基准侧指标（bench_vol/beta/corr 含 ε 守卫）收入 §7 与 fixtures；B14 腾讯指数 spot 源已落地），**T2 除 C4 闸门外全部移植完成**，余 wechat（gate C4 待拍板）；T3 ✅ design/ 已冻结 2026-10-30——README 加冻结横幅指向 frontend/，原型保留为像素/交互验收基准不再新增功能）**——P4 数据管道与存储已收官（2026-10-29，T2–T7 ✅；T1 前端抽屉接 API 已随 P5-T2 holdings 屏于 2026-10-30 收口；P4-T8 持仓直接维护 2026-11-01 增补 ✅——ADR-0009 快照校正三入口（直接录入/编辑/删除）+ holding_edits 留痕，schema v3，03 §13 口径定稿）。黄金测试向量（`fixtures/golden/`）十组 108 用例两边全绿（`advice` 打分组随 P4-T6、`alerts` 预警组随 P4-T7 于 2026-10-29 加入；B11/B12 定稿随 2026-10-30 加 lot 4 例 + perf 1 例并翻 `vq-sell-odd-lot`）：Rust `zimu-domain`（`cargo test -p zimu-domain`）与 frontend TS 共享层 `src/domain/`（`npm test`）跑同一份 fixture，无 pending 组（`price_limit` 已随 P1-T2 涨跌停口径定稿转 active）。P2 技术基座已收官（axum + robit SSE / React 工作台 / Python 行情，Web 桥沉淀为 robit `robit-web` v0.1.26）。P1 原型定稿 ✅（2026-10-29，T1–T9 全落地——T9 人工验收通过：7 屏 × file:///http.server 两种打开方式控制台零报错，窄屏、真实时钟、顶栏刷新、预警→建议→试算→结论链路、`?empty=1` 空态走查全过）。P4 数据管道（2026-10-28 开工，10-29 收官）：P4-T2 ✅ `zimu-store` SQLite schema/迁移（六表 + user_version 逐版迁移，`restart_does_not_lose_data` 通过，B12 奇零股存储就绪）；P4-T1 后端录入完成 🔨——`/api/holdings` `/api/holdings/trade` `/api/trades` `/api/account` 端点 + 录入事务（校验全复用 `zimu-domain`），C5 关闭（ADR-0006 手工逐笔先行），剩前端抽屉接 API（随 P5-T2）；P4-T3 ✅ `datafetch` 正式化——`zimu_datafetch` 包四端点 + 兜底链/熔断/限频/缓存 + `DfError` 错误契约，`ZIMU_DF_DISABLE` 注入验证「主源故障降级不崩」，baostock 复测恢复进兜底链（契约边界见 `04` 附录 B）；P4-T4 ✅ `zimu-market` 行情适配器（sidecar 唯一消费入口）+ `/api/market/*` 六端点——刷新回写走 `recalc` 重算、市场时钟按 `03` §9 生产口径（恒 Asia/Shanghai、交易日历优先、sidecar 挂了降级 503/启发式时钟不瘫）、当日 bar 合成见 `04` 附录 B；P4-T5 ✅ API 契约同源——serde 结构体 → OpenAPI 3.1（`docs/api/openapi.json`，utoipa 生成**永不手改**）、契约测试（实时响应 + `fixtures/api/` 样例过 schema 校验，见 `04` 附录 C）、前端 TS 类型自契约生成（`npm run gen:api`）、B6 已解（建议文案数字模板插值取 recalc 快照）；P4-T6 ✅ agent 领域工具与 Skill 模板（2026-10-29）——`zimu-agent` 公共 crate 8 工具（行情/持仓/纪律/试算/刷新/打分/文案回填，当时无写账本类；2026-11-08 经 ADR-0012 增至 11 领域工具）+ 仓库根 `skills/` 三模板（/复盘 /盘前 /建议，内嵌 03 口径与免责硬规则）；打分按 ADR-0007 规则函数出数（`zimu-domain::advice`，03 §10 生产口径 + 黄金向量），`calc_advice` 数字列先写 `advice_history`、`finalize_advice` 只回填模型文案——数字不经模型转写（B6 生产侧、B9 写入侧）；P4-T7 ✅ 调度与预警引擎（2026-10-29）——`zimu-agent::scheduler` 交易日盘中分钟级轮询（市场时钟 §9 闸门 → 刷新链复用 → `zimu-domain::alert` 按 `03` §12 判定序出预警 → 同日 (code,type) 去重落 `alerts`；行情失败不用旧值判）+ 15:05 盘后链（`equity_snapshots` 快照 date 主键幂等、数字 = recalc agg 原值；`/复盘` 注入 agent 会话、`push_log` 留痕，agent 离线快照照写）；新端点 `GET /api/alerts`、`POST /api/alerts/{id}/done`、`GET /api/equity_snapshots`；`ZIMU_SCHEDULER_DISABLE=1` 可停用调度。P5-T1 ✅ 前端工程约定（2026-10-29，ADR-0008）——react-router v7 路由表（`src/routes.ts` 单一来源，路径与原型文件名一致，未移植屏 = 占位屏）、openapi-fetch 契约客户端（`src/api/client.ts` 唯一 REST 入口，SSE 会话是唯一例外）、zustand 状态方案（派生数字以后端为唯一真值，抽屉实时试算用 `src/domain/` TS 库）、手写组件 + 设计令牌（不引 UI 库/图表库/Tailwind，要引先补 ADR）。`design/` 零构建原型保留为设计参照与验收基准。技术路线定案见 ADR-0005：Rust + robit agent 基建 / React + Vite 前端 / Python 行情 sidecar。

产品形态：发布后是**常驻 Web 服务**（浏览器访问）+ 微信/QQ 双向会话入口，不是桌面程序——这一点与 robit（TUI/GUI 桌面端）有意区分，做架构决定时别把两者混同。

## 现在这个仓库里有什么

```
backend/           Rust 服务（Cargo workspace）：crates/zimu-server（axum，health + SPA 静态托管 + Agent SSE 会话 + Web 文件上传（multipart，ADR-0015）+ 会话档案与重置端点 + 通道绑定管理 + 持仓录入/直接维护 API + 行情 API + 预警/快照 API + OpenAPI 契约；bin 侧启动调度器 + QQ IM 通道（ADR-0011：`[zimu.channels].qq` 缺省停用；`ChatbotManager<BoundOnly<QqPlatformAdapter>>`，robit-chatbot/robit-qq = 0.2.1）+ Ctrl+C 优雅关闭）
                   crates/zimu-agent（agent 应用层，P4-T6/T7 + ADR-0011/0012/0013/0014：领域工具 14 件（含写账本五件 record_trade/adjust_cash/get_trades/upsert_holding/remove_holding，自动落账+读回）+ robit 标准工具面（read/write/edit/ls/find/grep/bash 等，确认关闭自动执行，注册 25–27 随配置）+ Skill 加载 + 刷新链应用服务 + 调度预警引擎 scheduler + 唯一装配线 assembly::AgentCore（Web 桥/IM 通道/CLI 共用三 Arc）+ 通道推送回调面（AlertPusher/SummaryPusher，T6）——server 与 CLI 共用，数字口径只在 zimu-domain）
                   crates/zimu-agent-cli（Agent 调试 REPL：P2-T2 spike 起，P4-T6 换装与 server 相同的领域工具面与 Skill）
                   crates/zimu-domain（领域纯函数库，P3-T3：03 数字口径唯一 Rust 实现 + 黄金向量测试；P4-T6 增 advice 打分、P4-T7 增 alert 预警判定）
                   crates/zimu-store（存储层，P4-T1/T2/T7 + B13 + ADR-0009/0011：SQLite 十表（schema v2 增 watchlist 自选、v3 增 holding_edits 维护留痕、v4 增 chat_messages Web 会话档案 + channel_bindings IM 入站白名单）与迁移 + 录入事务 + 持仓直接维护（upsert/删除/留痕——快照校正不动现金不写 trades，03 §13）+ 行情回写 + 行业补全（fill_sectors 仅空串、手工值优先，P7-T6/ADR-0016）+ advice_history/alerts/equity_snapshots/push_log/watchlist/会话档案/绑定 读写，校验复用 zimu-domain，.db 不入库）
                   crates/zimu-market（行情适配器，P4-T4：datafetch sidecar 唯一消费入口——类型化契约/降级三分类/市场时钟/当日 bar 合成/行业映射缓存 SectorMapCache P7-T6）
skills/            Agent Skill 模板（P4-T6，robit SKILL.md 约定）：post-market-review（/review /复盘）、pre-market-watch（/watch /盘前）、generate-advice（/advice /建议）——产品文案资产，入库评审；ZIMU_SKILLS_DIR 可覆盖
frontend/          React + TS + Vite 前端：index 工作台 + dashboard 总览仪表盘 + holdings 持仓管理 + analysis AI 分析建议（P5-T2 真实数据接线：zustand stores + /api/* 消费，mock 层 demo.ts 已删；dashboard 含手写 SVG 图表组件 components/charts/*（折线/迷你走势/环形/蜡烛/得分弧）、kline/watchlist store 与自选维护入口——B13 前端半边；holdings 含加减仓抽屉 TradeDrawer——实时试算走 src/domain、提交 POST /api/holdings/trade 以后端收据覆盖，收口 P4-T1；直接维护（ADR-0009）——HoldingFormDrawer「直接录入 / 行内编辑」+ 两步删除 + 「维护记录」卡（快照校正：不动现金、不写流水，holdingEdits store 读 /api/holding_edits）；analysis 读 /api/advice 留档原值 + 真实 kline 蜡烛，绝不伪造建议、无留档走诚实空态；chat 智能问答功能对等——真实 Agent SSE 线程 ChatPanel + 文件上传（📎/拖拽/粘贴，ADR-0015）+ 上下文/边界/最近会话三卡，会话真值在服务端 chat_messages（ADR-0011：进屏 history 水合 + reset 清空，localStorage 降级为不可达时的只读缓存）；diagnosis 组合诊断——风险指标走 perf 黄金向量字段（B11 定稿后含 bench_vol/beta/corr）+ volSynth 再平衡测算，基准对齐抽 lib/bench 与 dashboard 共用）+ 占位屏（routes.ts ported 标记，T2 逐屏替换；余 wechat gate C4）；
                   src/domain/ 为领域纯函数 TS 共享层（P3-T4：镜像 zimu-domain，vitest 跑同一份黄金向量），令牌在 src/styles/tokens.css；
                   src/api/schema.d.ts 为 OpenAPI 契约生成的 TS 类型面（P4-T5，`npm run gen:api` 再生成，不手改）；
                   P5-T1 工程约定落地（ADR-0008）：react-router 路由表 src/routes.ts + openapi-fetch 客户端 src/api/client.ts + zustand 状态基线
datafetch/         Python 行情 sidecar（uv + FastAPI + akshare/baostock）：P4-T3 正式化——`zimu_datafetch/` 包（/quote /kline /cb_list /calendar /sector_map，兜底链+熔断+限频+缓存+错误契约；/sector_map 行业映射 P7-T6/ADR-0016），spike 探测脚本保留为复现入口
design/            界面原型（P5-T3 已冻结 2026-10-30）：7 个 html + assets/{ui.css, data.js, charts.js}——设计参照与像素/交互验收基准，生产实现在 frontend/，原型只保留不新增功能
fixtures/golden/   黄金测试向量（P3）：数字口径的权威样例集，Rust 领域库与 TS 共享层跑同一份；schema 见 03 附录
fixtures/api/      API 响应样例（P4-T5）：每端点一份提交样例，契约测试对其过 OpenAPI schema 校验；前端可直接当 mock
docs/              产品规格与工程规范（编号即阅读顺序）
docs/api/          OpenAPI 3.1 契约（由代码生成永不手改；再生成与守护命令见 04 附录 C）
docs/decisions/    ADR 决策记录
```

原型直接双击 `design/index.html` 可跑；本地服务：`python -m http.server 8000 --directory design`。

## 动手之前必须读的三份

| 要做什么 | 先读 |
|---|---|
| 改任何界面/文案/交互 | `docs/02-screens.md` + `docs/05-design-system.md` |
| 碰数字、指标、业务规则 | `docs/03-domain-rules.md`（口径唯一来源） |
| 改 `data.js` 或准备接真实数据 | `docs/04-data-contract.md` |

`design/README.md` 只保留「怎么打开原型」与指向 `docs/` 的导引表（P1-T8 已收敛，规格单一来源 = `docs/`）。

## 工程约定

**技术形态（受 ADR-0005 约束；0002/0003 已被其取代）**
- `design/` 原型保持零构建（不引入打包器、框架、CDN、npm 包）；**P5-T3 已冻结（2026-10-30）**——它是设计参照与像素/交互验收基准，生产实现在 `frontend/`（六屏已接真实数据，wechat 待 C4）。冻结后原型只保留、不再新增功能，任何界面/规格改动走 `frontend/` 与 `docs/`。
- 技术路线见 ADR-0005：`backend/`（Rust workspace，依赖 robit）、`frontend/`（React + TS + Vite）、`datafetch/`（Python sidecar，uv + FastAPI + akshare/baostock）。目录已随 P2 spike 创建（datafetch P2-T1 ✅；backend + frontend 最简框架 2026-10-27 立起；P2-T2 ✅——`zimu-agent-cli` 直接消费 crates.io 的 robit 0.1.25，流式对话 + echo 工具 function-calling 验收通过；P2-T3 ✅——Agent SSE 桥进 zimu-server（`/api/chat*` 端点 + ChatPanel，浏览器/curl 均见流式输出）；P2-T4 ✅——index 工作台静态复刻进 React（Chrome 外壳 + 六区块，`demo.ts` 同口径移植 `data.js`，41 项派生数字与原型逐位一致；会话面板移入「智能问答」视图；P2-T5 ✅——共同演进回路首次走通：R1 沉淀为 robit `robit-web` crate（`WebEvent`/`WebFrontend`/`WebBridge`，v0.1.26 本地 tag），zimu-server 以 git pin 消费、宿主手写桥删除）。前端版本基准对齐 robit-gui（React 19 / Vite 6 / TS 5.6），后端 crate 版本对齐 robit workspace，升级前先想两边。
- 两工程共同演进：框架级缺口先改 robit（`E:\Github\robit`，改动保持通用性、走它自己的发布流程），子木消费其 crates.io 正式版（开发期可临时改回 git tag/rev pin）；痛点记入 `docs/07-backlog.md` R 系列，**不要写绕过 robit trait 的补丁**，领域逻辑不渗进 robit。回路已**三轮全链走通**：第一轮 R1（P2-T5 演练 → 2026-10-30 正式化）——`feat/web-bridge` → 合并推送 `main` → tag `v0.1.26` → crates.io 发布 → 子木 registry 消费；第二轮 R2–R5（2026-10-31）——子木整理四条宿主形态需求（bootstrap 组合 / 日志可组合原语 / 额外技能目录 / anyhow 互操作）→ robit `feat/host-ergonomics-r2-r5` 落地发布 **v0.1.27** → 子木切换 R3+R4，R5 经评估**有意不消费**（存储错误脱敏话术，理由记录在 backlog R 系列与代码注释）；R2 原有意不消费，**2026-11-14 经 ADR-0014 整体推翻**（标准工具面全量消费、确认关闭自动执行）；第三轮 R7（2026-11-05 → 11-07）——子木移交思考过程输出需求文档（唯一真值 `robit/docs/robit-rfc-thinking-output.md`）→ robit `feat/thinking-output` 落地发布 **v0.1.28**（reasoning_content 双路径捕获 / `AgentEvent::ThinkingDelta` / robit-web `thinking_delta` wire 帧 / R6 空白帧发射端收口）→ 子木升版消费（CLI 补臂 + ChatPanel 思考段三态折叠，连带 rusqlite 0.40→0.41 对齐）。当前消费 `robit-ai/robit-agent/robit-web = "0.2.1"` + `robit-chatbot/robit-qq = "0.2.1"`（P7-T2 QQ 验证通道起），见 `backend/README.md`。**第四轮已开启（R9，2026-11-08）**：子木 QQ 真机走查发现 chatbot ThinkingDelta 每帧一条 trace 刷屏（单回合实测 236 行）→ robit 0.1.29 改回合收口汇总计数（changelog 记名子木消费反馈；无 API/wire 变化，子木纯升版）；**第四轮全链收口（2026-11-14）**：R8 六件套（chatbot 宿主形态：注入 API/source 参数化/cleanup 实装/库路径平台维度/memory 工具跟随全局存储 + 人格提示词宿主注入 API）+ R10 附件批七条（QQ 入站附件链路：内容不可达/双重下载/标注重复/文件名未消毒/同名覆盖/无大小限制/无清理策略）经交接文档（robit `docs/robit-rfc-qq-media-and-host-feedback.md`）移交后 **robit 0.2.0 全量落地**（13/13，robit-web 零 diff wire 不变），子木同日升版消费（最小适配：Agent 构造 custom_prompt=None / MessageData.source / ToolContext.memory_dir / platform_name() 转发 / rusqlite 0.42 对齐；QQ 会话库按开发期拍板删除重建）。**子木侧零绕过**；inject_message/人格注入/media 保留策略等新能力另案拍板消费。**M1 收尾（0.2.1，同日）**：附件标注行携带本地保存路径（`describe_with_local`，相对 working_dir），数据文件对模型路径可见——内容读取经 ADR-0014 标准工具面闭环（read/bash 自动执行，2026-11-14）。微信接入方向已拍板（OpenClaw 式个人助理，非服务号）：接入方案归 robit 解决，子木只消费 PlatformAdapter 面（ADR-0011/C4 备注）。
- `design/` 原型只用演示样本；真实行情自 P2 起经 datafetch sidecar 进入。**真实持仓、券商账户数据、任何 API key 永不入库。**
- **会话真值在服务端、通道独立**（ADR-0011）：Web 会话档案 = `zimu.db` `chat_messages`（先落库再注入；复活只回灌 user/assistant 文本对，**工具轨迹存档不回灌**——陈旧数字不进上下文；thinking 与通知两处都不入档），IM 会话历史归 robit 自持库；localStorage 只是降级缓存。多通道 = 共享 `AgentCore` 三 Arc + 各通道自己的 Agent 实例（一个 Frontend 只能挂一个 Agent 的框架事实），**能力面渠道无关（嘴不同手相同）、会话按通道独立**；通道启用与凭据只在工程本地配置文件 `backend/.robit/config.toml`（`[zimu.channels]` 缺省停用 fail-closed），绑定关系界面管理（`channel_bindings`）——**IM 入站唯一闸门是绑定白名单**（fail-closed：判定失败不应答），服务本身仍无鉴权、监听面不变（子木的 IM 信任边界 = 绑定表）。

**前端工程约定（P5-T1 定稿，ADR-0008；`frontend/` 内所有新代码适用）**
- 路由：react-router v7（BrowserRouter），路由表单一来源 `src/routes.ts`——路径与原型文件名一致（`/` `/dashboard` `/holdings` `/analysis` `/chat` `/diagnosis` `/wechat`），未移植屏渲染占位屏，T2 每移植一屏翻 `ported` 并在 App 挂真实屏；未知路径回落 `/`；新屏必须走 routes.ts 注册 + Chrome 导航项，禁止组件内私自切视图。
- API client：唯一 REST 入口 `src/api/client.ts`（openapi-fetch + `npm run gen:api` 生成的 `schema.d.ts`）；不手拼 URL、不裸 fetch，`[data, error]` 元组的 error 分支必须处理；唯一例外 = SSE 会话 `/api/chat*`（ChatPanel 原生 fetch 流式读取）。契约变更：后端再生成 openapi.json → `npm run gen:api` → tsc 暴露漂移。
- 状态：zustand（与 robit-gui 基线一致），store 按域拆 `src/store/<domain>.ts`（随 T2 逐屏落地，不预建空壳）。**派生数字以后端为唯一真值**（recalc/打分/预警判定全在服务端），store 只缓存 API payload 与 UI 状态；唯一例外 = 加减仓抽屉实时试算用 `src/domain/` TS 库，提交后以后端返回覆盖。
- 组件与样式：手写组件 + 设计令牌（`styles/tokens.css` / `ui.css`），图表手写 SVG 镜像 `design/assets/charts.js`；不引 UI 组件库、图表库、Tailwind——要引先补 ADR。已记录的例外：助手消息 Markdown 渲染用 react-markdown + remark-gfm（ADR-0010，文本渲染工具非 UI 库；原始 HTML/图片不渲染、链接 noopener）。原型约定全部保留：每屏 `data-od-id`、底部 `*-disclaimer`、格式化统一 `money/fmt/pct/wan`（过渡期在 `data/demo.ts`，删除 demo.ts 前先抽 `src/lib/`）、三套涨跌语义不混类、`--accent` 每屏 ≤2 处。
- mock 数据：`demo.ts` 随 T2 逐屏被真实 API 替换，最后一屏迁完即删（`design/` 冻结见 P5-T3）。

**代码规范（`design/` 原型）**
- 页面：每屏一个自执行 `<script>`，通过 `window.ZM`（数据/工具）与 `window.ZMC`（图表）取用共享能力；页面开头调用 `D.chrome({ active, crumb, onRefresh })` 渲染外壳，`onRefresh` 里重绘本页读汇总数据的区块（顶栏刷新按钮会先跳动行情并重算，再回调它）。
- 派生数字一律现算，不写死字面量；需要新指标时加进 `data.js`，别在页面里就地算完写进文案。
- 逐笔派生与全部汇总只出自 `ZM.recalc()`：价格、持仓、现金变动后必须调用（`tickPrices` 内部已调）；页面不得自行补算派生值、不得给 `ZM` 挂新属性，改现金要写 `ZM.ws.cash` 而不是 `ZM.cash`。
- 每个可走查的区块带 `data-od-id="..."`（kebab-case，重复项用 `-0`、`-1` 下标），设计走查与自动化截图依赖它。
- 每个页面底部保留 `*-disclaimer` 免责区块，新增界面不能漏。
- 涨跌配色按 A 股惯例：`--up` 红 = 涨，`--down` 绿 = 跌，只用于涨跌方向的数字与标记。好坏轴（合规/违规、得分、状态）用 `.ok/.warn/.danger`，交易方向（买入/卖出、加仓/减仓）用 `.buy/.sell`（`--up/--down` 别名令牌）——三套语义禁止混类（P1-T4 定稿，映射见 `03` §11、令牌见 `05`）。
- `--accent` 每屏不超过两处，只用于交互态。

**中文文案**
- 界面文案与文档用简体中文，技术标识符保持英文；数字用 `font-variant-numeric: tabular-nums`，价格/代码走 `--font-mono`。
- 金额、百分比的格式化统一走 `ZM.money / fmt / pct / wan`，不要手搓 `toFixed` 拼百分号。

## 验证方式

数字口径有自动化验收：两边 runner 跑**同一份** `fixtures/golden/`——backend `cargo test -p zimu-domain`（Rust 领域库），frontend `npm test`（vitest，TS 共享层）。改口径的顺序固定：**先改 `docs/03` 与 fixtures，再改两边实现**；实现之间永不互相对拍。录入流程与持久化用 `cargo test -p zimu-store`（事务/拒绝/重启不丢数据/行情回写/预警与快照读写）；存储层只复用 domain 校验，**不新定义任何口径**。行情链路用 `cargo test -p zimu-market -p zimu-server`（适配器单测 + API 集成测试：API 数字与 `zimu-domain::recalc` 逐位等价、FakeApi 注入的 sidecar 降级，全离线）。agent 工具链与调度引擎用 `cargo test -p zimu-agent`（黄金向量同款输入端到端：当日 bar 合成 → 打分 → advice_history 留档；工具数字与 domain 直算按容差一致——JSON 文本往返有 serde_json 解析 1-ulp 误差，比对规则与 03 附录同款；Skill 模板加载/触发词唯一/tools_required 已注册断言；调度器 `tick(now)` 注入模拟时刻——盘中触发预警并同日去重、非交易日/午休空转不打 sidecar、sidecar 挂不判旧值、15:05 快照手算锚点 + 复盘回调恰一次 + push_log 留痕，全离线）。API 契约用 `cargo test -p zimu-server --test contract`（`docs/api/openapi.json` 与代码逐字节一致 + 实时响应与 `fixtures/api/` 样例过 schema 校验；改契约结构体后按 `04` 附录 C 再生成文档与样例，前端 `npm run gen:api` 同步类型）。

`design/` 原型没有测试框架，改完请人工过一遍（浏览器控制台必须无报错）：

1. 直接双击打开每个页面（`file://`，注意跨页依赖会在这种方式下暴露）。
2. `python -m http.server` 后访问，逐项检查：控制台无错误、侧栏/顶栏/底栏渲染正常、数字与图表口径一致。
3. 窄屏：拖到 900px 以下确认侧栏转抽屉、底部标签栏可点。
4. 交易时段相关逻辑用系统时间验证（`ZM.marketClock()` 读真实时钟）。

## 提交约定

- 分支：`main` 为主干，功能分支 `feat/<简述>`、修复 `fix/<简述>`。
- Commit message 用 Conventional Commits + 中文正文，例如：
  `docs: 补业务口径手册，固定费用与仓位分母口径`
  `fix(holdings): 涨跌停校验按板块区分 10%/20%`
- 不要提交：`.env`、密钥（微信 AppID/Token/Webhook、行情源 key）、券商对账单、真实持仓导出。`.gitignore` 已覆盖，改动时注意别放宽这些规则。
- 只在被要求时 commit / push。

## 决策与技术选型怎么推进

- 有取舍的决定写进 `docs/decisions/NNNN-<slug>.md`（模板见该目录 README），编号递增，状态 `提议 / 接受 / 已取代`。
- 未定事项集中在 `docs/07-backlog.md`（问题）与 `docs/06-roadmap.md`（阶段）。
- 后端栈、前端框架、真实行情源已由 ADR-0005 定案；C5 录入方式（ADR-0006：手工逐笔先行，CSV 后续增强）与 C8 打分生成（ADR-0007：规则打分 + 模型写文案）已于 2026-10-28 拍板。当前待拍板只剩 C4 微信渠道（P5 前），见 `06-roadmap.md` 决策闸门表。

## 安全与责任边界

本系统不对接任何交易系统，只是一份账户信息快照账本（ADR-0012，2026-11-08 准则）——「不代下单」是表述纪律：不得让用户误以为系统能真实执行交易，真实操作永远由用户在券商端完成。智能体可写账本（交易录入/现金校正 ADR-0012 + 持仓快照校正 ADR-0013，自动落账+读回）：记账数字必须来自用户明示、先试算后落账并宣读收据、流水只增不改错账反向冲正、快照校正整行覆盖先读后改、删除持仓须对话内两步确认（纪律全文见 `zimu-agent/src/tools/mod.rs` 头）。每条建议必须带目标价/止损/失效条件与风险自陈；`conf` 指样本内信号一致性，不是胜率，不要在文案里改成收益承诺或确定性表述。ADR-0014（2026-11-14）起智能体持有 robit 标准工具面（文件/shell/生图，**确认关闭自动执行**）：工具无沙箱，纪律是唯一软约束——禁读/输出 .env 与凭据、账本写入只走写账本五件（禁 bash/write/edit 直写 zimu.db）、破坏性操作须主人明示（纪律全文见 `prompts/agent.md`「文件与 shell 纪律」段，守护测试钉死；信任边界扩张明示于 ADR-0014）。Web 对话框文件上传（ADR-0015）只向 `media/` 写消毒后文件、**上传本身不执行任何东西**，解析由智能体按上述纪律自动执行（与 QQ 附件同链，robit 零改动）。
