# 子木 · 股票智能助理（StockAgent）

面向个人投资者的股票助理程序：盯持仓、读行情、给可执行的买卖建议、诊断组合风险。发布形态是**常驻 Web 服务**——浏览器访问全部界面，也可以通过微信 / QQ 与助理对话、接收预警与盘后复盘（与 robit 的桌面端定位不同）。

**当前阶段：P4 数据管道与存储收官（2026-10-29）——录入 API + SQLite 存储、`datafetch` 行情 sidecar 正式化、`zimu-market` Rust 行情适配器 + `/api/market/*`（刷新回写→`recalc` 重算、市场时钟、sidecar 挂了整体降级不瘫）、OpenAPI 契约同源（serde 结构体生成 `docs/api/openapi.json`，契约测试样例响应过 schema 校验，前端 TS 类型自契约生成）（2026-10-28，C5 关闭）；agent 领域工具与 Skill 模板（P4-T6）——`zimu-agent` crate 8 工具 + `skills/` 三模板（/复盘 /盘前 /建议），打分按 ADR-0007 规则函数出数（03 §10 生产口径 + 黄金向量 advice 组）、模型只写文案且数字列先落 `advice_history` 留档；调度与预警引擎（P4-T7）——交易日盘中分钟级轮询（预警判定口径 03 §12 + 黄金向量 alerts 组，同日同型去重落 `alerts`）+ 15:05 盘后链（`equity_snapshots` 净值快照幂等写入、`/复盘` 注入 agent 会话、`push_log` 留痕）+ `GET /api/alerts` 等三端点；P1 原型定稿收官（2026-10-29，T9 人工验收通过——7 屏 × file:///http.server 两种方式控制台零报错，窄屏/真实时钟/刷新/预警链路/空态走查全过）；P5 前端移植开工——T1 工程约定 ✅（2026-10-29，[ADR-0008](docs/decisions/0008-frontend-conventions.md)：react-router v7 路由表 + openapi-fetch 契约客户端 + zustand 状态基线，手写组件 + 设计令牌不引 UI 库）；T2 逐屏移植 🔨——index 工作台 ✅（真实数据接线，mock 层 demo.ts 清零）、dashboard 总览仪表盘 ✅（2026-10-30，手写 SVG 图表组件 + 净值/沪深 300 基准对齐走 domain perf + 自选卡增删维护——B13 全链闭合）。** 技术路线已定案（[ADR-0005](docs/decisions/0005-tech-route-rust-robit-react-sidecar.md)）：Rust + [robit](https://github.com/leexgone/robit) agent 基建 / React + Vite 前端 / Python 行情 sidecar / SQLite。已跑通：`backend/`（axum）托管 `frontend/`（React）构建产物、单端口起服务——index 工作台静态复刻 + 智能问答 SSE 会话（robit Agent）+ **手工逐笔录录入 API（`/api/holdings/trade` 等，校验复用领域库、落 SQLite 事务）**；`docs/03` 全部数字口径落成 Rust（`zimu-domain`）与 TS（`frontend/src/domain/`）两份纯函数实现，跑同一份黄金测试向量全绿。`design/` 原型（零构建、演示样本）保留为设计参照与验收基准。

```powershell
# 运行（生产形态：后端托管前端构建产物，单端口）
cd frontend; npm install; npm run build
cd ..\backend; cargo run -p zimu-server   # → http://127.0.0.1:8080（workspace 多 bin，必须带 -p）

# 行情 sidecar（/api/market/* 需要；缺席时行情端点降级 503，持仓/会话照常）
cd datafetch; uv run uvicorn zimu_datafetch.server:app --host 127.0.0.1 --port 8300

# 开发形态：前端热更新（5173，/api 反代 8080），细节见 backend/README.md

# 口径测试：两边跑同一份 fixtures/golden/（改口径先改 docs/03 与 fixtures）
cd backend; cargo test -p zimu-domain     # Rust 领域库
cd backend; cargo test -p zimu-store      # 存储层：录入事务 / 校验拒绝 / 重启不丢 / 行情回写
cd backend; cargo test -p zimu-market     # 行情适配器：契约模型 / 市场时钟 / 当日 bar 合成 / 错误映射
cd backend; cargo test -p zimu-server     # API 集成 + 契约：数字与 domain 逐位等价 / sidecar 降级 / 样例响应过 schema（离线）
cd backend; cargo test -p zimu-agent      # agent 工具链 + 调度引擎：黄金向量端到端 / 打分留档 / Skill 加载 / 模拟盘中触发预警与 15:05 盘后链（离线）
cd frontend; npm test                     # TS 共享层（vitest）
cd frontend; npm run gen:api              # 契约变更后再生成 TS 类型（docs/api/openapi.json → src/api/schema.d.ts）
cd datafetch; uv run pytest               # 行情 sidecar：兜底链/熔断/限频/缓存（假源离线）
cd datafetch; uv run python live_smoke.py # sidecar 联网冒烟：真实通道 + 故障注入验收
```

```bash
python -m http.server 8000 --directory design   # 原型：或直接双击 design/index.html
# → http://localhost:8000/
```

## 目录

| 路径 | 内容 |
|---|---|
| `backend/` | Rust 服务（Cargo workspace）：`zimu-server`（axum 主服务——health + SPA 静态托管 + Agent SSE 会话 + 持仓录入 API + 行情 API + 预警/快照 API + OpenAPI 契约，bin 侧启动调度器）、`zimu-domain`（领域纯函数库：`docs/03` 全部数字口径的 Rust 实现 + 黄金向量测试，含打分模块 `advice` 与预警判定 `alert`）、`zimu-store`（存储层：SQLite schema + 迁移 + 录入事务 + 行情回写 + 建议/预警/快照/推送日志读写，P4）、`zimu-market`（行情适配器：sidecar 唯一消费入口——契约模型/降级/市场时钟，P4-T4）、`zimu-agent`（agent 应用层：领域工具 + Skill 加载 + 刷新链服务 + 调度预警引擎，P4-T6/T7）、`zimu-agent-cli`（Agent 调试 REPL）；细节见 `backend/README.md` |
| `skills/` | Agent Skill 模板（P4-T6）：盘后复盘 / 盘前关注 / 建议生成三份 SKILL.md（robit 约定），内嵌 `03` 口径引用、免责边界与「文案数字必须原样取自工具返回」硬规则 |
| `frontend/` | React + TS + Vite 前端：index 工作台 + dashboard 总览仪表盘（P5-T2 真实数据接线，mock 层已删；dashboard 含手写 SVG 图表 `components/charts/*`——折线/迷你走势/环形，kline 与 watchlist store，自选增删维护入口 = B13 前端半边）+ 智能问答视图（SSE 会话）+ 占位屏（P5-T2 逐屏替换）；`src/domain/` 为 TS 领域共享层（镜像 `zimu-domain`），`src/api/schema.d.ts` 为 OpenAPI 契约生成的类型面（P4-T5），设计令牌在 `src/styles/tokens.css`；P5-T1 工程约定落地（ADR-0008）——react-router 路由表 `src/routes.ts`（未移植屏 = 占位屏）+ openapi-fetch 客户端 `src/api/client.ts` + zustand 状态基线 |
| `datafetch/` | Python 行情 sidecar（uv + FastAPI + akshare/baostock）：P4-T3 正式化——`/quote` `/kline` `/cb_list` `/calendar` + 兜底链/熔断/限频/缓存/错误契约，主源故障降级不崩；由 `backend/crates/zimu-market` 唯一消费（P4-T4 已接线） |
| `design/` | 界面原型（7 个页面 + 共享样式/数据/图表层），原型自带的说明见 `design/README.md` |
| `fixtures/golden/` | 黄金测试向量：数字口径的权威样例集（10 组 103 用例，含 advice 打分与 alerts 预警组），Rust 与 TS 两边跑同一份；schema 见 `docs/03` 附录 |
| `fixtures/api/` | API 响应样例（P4-T5）：每端点一份提交样例，契约测试对其过 OpenAPI schema 校验；前端移植可直接当 mock |
| `docs/` | 产品规格与工程规范：范围、界面、业务口径、数据契约、设计系统、路线图、待办；`docs/api/openapi.json` 为机器可读 API 契约（由代码生成，见 `04` 附录 C） |
| `docs/decisions/` | 决策记录（ADR）—— 每个「为什么这样做」都留档 |
| `CLAUDE.md` | 给 Claude Code 的工程约定，动手前先读这个 |

## 原型包含的界面

| 界面 | 职责 |
|---|---|
| `index.html` | 工作台入口：随时段变化的问候、资产快照、界面导航、待决事项 |
| `dashboard.html` | 总览：净值 vs 沪深300、当日盈亏归因、待处理预警、持仓走势、收盘摘要 |
| `holdings.html` | 持仓：明细/排序/筛选，加减仓抽屉（整手与涨跌停校验、费用、摊薄成本、仓位重算） |
| `analysis.html` | AI 建议：结论 + 得分 + 置信度、四维拆解、30 日 K 线、分笔执行计划、风险自陈 |
| `chat.html` | 问答：结论 / 为什么 / 怎么做三段式回答，附依据来源与追问 |
| `diagnosis.html` | 组合诊断：风险指标、回撤水下曲线、行业集中度与 HHI、再平衡清单 |
| `wechat.html` | 微信接入：通道与降级、推送订阅、静音标的、绑定、卡片模板、推送日志 |

## 文档地图

改原型之前，按需读：

- `docs/01-vision-and-scope.md` —— 做什么、不做什么、合规边界
- `docs/02-screens.md` —— 每屏模块与数据依赖
- `docs/03-domain-rules.md` —— **口径手册**：整手、费用、摊薄成本、涨跌停、仓位纪律、波动测算（附录＝黄金向量 schema）
- `docs/04-data-contract.md` —— `window.ZM` 数据契约，接真实数据时唯一的替换边界
- `docs/05-design-system.md` —— 视觉令牌、红涨绿跌语义、断点
- `docs/06-roadmap.md` / `docs/07-backlog.md` —— 阶段计划与已知问题
- `docs/decisions/` —— ADR：文档先行、零构建原型、演示数据边界、技术路线（Rust + robit + React + sidecar）

## 三条硬约定

1. **页面上不写死数字。** 总资产、盈亏、占比、集中度等一律从持仓与净值序列派生，避免文案与图表互相打架。
2. **共享派生逻辑只能放在 `design/assets/data.js`。** 页面不得各自往全局对象上补函数（已有跨页引用踩坑，见 `docs/07-backlog.md`）。
3. **`design/` 原型不引入构建、框架与第三方依赖**；正式前端按 ADR-0005 在 `frontend/`（React + Vite）另起，原型保留为参照直至迁移完成。

## 免责声明

本项目用于界面与信息架构验证。所有「买入 / 持有 / 卖出」结论均由演示数据生成，**不构成投资建议**；即便接入真实行情，每一笔委托仍需你本人确认。程序不连接券商交易接口，不代提交委托。

## 许可

Apache-2.0，见 `LICENSE`。
