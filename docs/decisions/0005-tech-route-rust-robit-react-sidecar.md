# 0005 · 技术路线：Rust + robit 基建、React 前端、Python 行情 sidecar

- **状态**：接受
- **日期**：2026-10-25
- **决策者**：Steven Lee
- **取代**：`0002`（前端零构建——原型迁移完成前仍适用）、`0003`（暂缓选型——选型已定）

## 背景

P0 文档基线与原型 A1/B1 修复完成后，契约（`04`）与业务口径（`03`）已稳定，具备选型条件。决定性变量：决策者自有开源项目 **robit**（`github.com/leexgone/robit`，Apache-2.0，Rust monorepo）已经提供了子木核心体验所需的全部基建——agent 运行时（工具调用、流式、上下文压缩）、多 provider LLM 接入（OpenAI 兼容，可直连现有阿里云 MaaS 网关的 qwen 系）、Skill 提示模板、多平台 chatbot 基建（`PlatformAdapter` trait + 可参考的 QQ 实现）、rusqlite 存储惯例。子木的「AI 分析建议 / 智能问答 / 盘后复盘推送」本质就是一个带领域工具的 agent 产品，复用 robit 的收益超过了语言生态的差异（行情库在 Python 侧，用 sidecar 桥接）。

`0003` 原定解除条件（C4/C5 有结论 + P1–P3 完成）未全部满足；提前决策的理由是 robit 的存在改变了成本结构。C4（推送渠道）与 C5（首笔持仓录入）**仍然开放**：C5 最晚 P6 前落定；C4 的渠道选择最晚 P5（wechat 屏改造为 PushProvider 配置管理）前落定，架构面已由本决定第 5 条解决。

## 决定

1. **后端**：Rust（tokio 生态）。StockAgent 独立仓库新建 `backend/` Cargo workspace，依赖 `robit-ai` / `robit-agent` / `robit-chatbot`。开发期用 cargo path override 指向本地 robit 检出（`E:\Github\robit`），提交与发布 pin 到 git tag/rev；robit 尚在 0.1.x，升级走 lockstep（若已发布 crates.io 可切版本依赖）。协作模式见下节。
2. **产品形态与 API 层**：子木发布后是**常驻 Web 服务**——这是与 robit（桌面端 TUI/GUI 程序）的关键定位差异。单个 backend 进程（axum）托管 React SPA 静态产物 + REST + SSE（agent 流式输出），浏览器是主界面，局域网内访问；微信/QQ 是会话入口（见第 5 条）。默认**不做公网入站**——唯一例外：若 C4 选定的微信渠道需要消息回调，走内网穿透/反代，届时单独补安全 ADR。
3. **LLM**：走 robit-ai 的 provider 配置接阿里云 MaaS 网关（qwen 系，OpenAI 兼容）；密钥 `${ENV}` 替换，永不入库。
4. **建议生成（C8 的架构面）**：agent + 领域工具（`get_quote` / `get_holdings` / `calc_discipline` / `simulate_trade` 等，替代通用 bash/write）+ Skill 模板（盘后复盘、减仓计划等固定套路，模板内嵌 `03` 口径与免责边界）。`score/conf/dims` 具体怎么算仍开放（见 C8）。
5. **推送与会话渠道（C4 部分决策）**：微信/QQ 不只是推送出口，而是**双向会话入口**——复用 robit-chatbot 的多会话基建（每会话独立 Agent session），`PlatformAdapter` 统一接三类端：浏览器 SSE 会话、IM 双向会话、单向推送。QQ 侧可直接复用 `robit-qq`（官方 Bot，出站 WebSocket 网关，NAT 后即可用、无需公网）；微信侧各渠道的双向能力与公网要求差异大（企业微信自建应用回调需公网入站、群机器人只发不收、测试号不长期稳定），已列为 C4 调研的核心约束。先落 console + 文件留档两个适配器跑通主流程，加渠道不动主程序。
6. **调度**：tokio 定时任务（盘中分钟级预警轮询、15:05 盘后复盘触发 Skill），不引入独立 worker。
7. **存储（C9 已决）**：SQLite（rusqlite bundled），单文件、gitignored；持仓、建议留档（带生成时间与数据版本，解 B9）、预警、净值快照、交易流水。备份 = 拷文件。
8. **行情（C3 已决）**：Python sidecar（`datafetch/`，uv 管理，FastAPI 只暴露 `/quote` `/kline` `/cb_list` 等少数端点），akshare 为主、baostock 兜底，切换与重试逻辑全在 Python 侧；Rust 主程序经 HTTP 消费，把它当唯一行情适配器。sidecar 崩溃不影响主程序。**无论选型多自信，P2 必须先做半天字段/频率/限频 spike 验证。**
9. **前端（C2 已决）**：React + TS + Vite（`frontend/`），7 屏原型逐屏移植；设计令牌（oklch、红涨绿跌、tabular-nums、断点）从 `ui.css` 迁移，`data-od-id` 惯例保留。与 robit-gui 同栈，前端经验可直接复用（但子木的目标形态是 Web 服务，**不做桌面壳**）。`design/` 原型保留为设计参照，迁移完成前继续按 `0002` 的零构建约定维护。
10. **口径一致性**：`03` 公式存在 **Rust（权威实现，用于推送/复盘/预警）与 TS（试算交互）双实现**；P2 的「手算可对用例」固化为**黄金测试向量**（JSON fixture 入库），两边跑同一组数据，防止口径漂移。

## 与 robit 的协作模式（两工程共同演进）

- **两个工程、两个仓库**：StockAgent 是产品，robit 是通用 agent 框架，各自独立演进（均 Apache-2.0、同一作者，无许可摩擦）。不合并仓库，不 vendor。
- **改动方向原则**：框架级缺口——例如 `PlatformAdapter` 需要「只推不收」的单向语义、agent 需要无人值守的定时会话、`Frontend` 事件需要 SSE/Web 桥——**先改 robit**，改动保持通用性、走 robit 自己的文档与发布流程；子木 pin 新版本消费。纯领域逻辑（行情、持仓、纪律、建议模板、SQLite schema）一律留在 StockAgent，**不渗进 robit**。
- **设计时反向输入**：子木消费 robit 各 trait（工具注册、事件协议、配置加载）时，把 API 形状上的痛点记录到 `07-backlog.md`（R 系列），作为 robit 重构的需求来源；不允许在子木侧写绕过 trait 的补丁——绕过去等于把框架债藏进产品。
- **依赖纪律**：本地联调用 path override，落库必须 pin rev/tag；robit 的破坏性变更要先在 robit 里写好迁移说明再升级子木。

## 目录规划（P2 启动时创建，勿提前半建）

```text
backend/     Rust workspace（axum + robit 依赖 + 领域工具/调度/存储）
frontend/    React + TS + Vite SPA
datafetch/   Python sidecar（uv + FastAPI + akshare/baostock）
design/      原型（保留至 frontend 迁移完成，之后冻结为参照）
```

## 选项与取舍

| 分叉 | 选定 | 落选 | 理由 |
|---|---|---|---|
| 后端语言 | **Rust + robit** | Python+FastAPI（`0003` 原候选）、Node/TS | agent/推送/存储基建现成；Python 生态优势由 sidecar 保留；Node 需从零搭 agent 层 |
| robit 集成方式 | **独立仓库 + git 依赖** | 进 robit workspace 加 crate、vendor | 产品与框架分离，robit 保持通用定位；path override 可临时用于同步改框架 |
| 前端框架 | **React + Vite** | Vue 3、继续零构建 | 与 robit-gui 同栈，前端经验直接复用；形态是 Web 服务，不做桌面壳（决定第 9 条） |
| 行情桥 | **Python sidecar** | Rust 直连东财/新浪、定时落库 | 直连 = 自己重写 mini-akshare 且接口常变；落库盘中时效差、双调度协同复杂 |
| 推送 | **可插拔适配器，渠道后定** | 现在绑死企业微信/Server酱 | C4 调研未完成，先跑通主流程 |

## 后果

- 正面：agent 循环、LLM provider、多会话 chatbot 基建、SQLite 惯例零成本获得；serde 结构体天然是 `04` 契约的类型化载体；单二进制 + sidecar 部署轻，家用常开机器即可——浏览器与 QQ 端（出站 WS）零公网依赖，微信双向会话视 C4 渠道可能需要内网穿透。
- 负面 / 代价：仓库变成 Rust + TS + Python 三语言，工程复杂度上升；`03` 公式双实现要靠黄金向量长期护航；robit 0.1.x API 不稳——共同演进模式把「被动追上游」变成「主动改上游」，代价是每次框架级改动要同时维护 robit 的通用性与子木的消费面，需要 pin rev 纪律兜底。
- 仍然开放：C4 推送渠道（P6 前定）、C5 首笔持仓录入方式（P6 前定，影响空态与导入端点）、C8 打分生成方式（P4/P6 之间定）。B2–B9 口径修复与栈无关，可并行。
- 改回来的成本：后端换语言 = 重写（但黄金向量与契约文档可带走）；前端换框架 = 重移植（设计令牌与 `04` 契约不变）；sidecar 换直连 = 只动行情适配器。

关联：取代 `0002`（部分）、`0003`；`0001`、`0004` 仍有效；`06-roadmap.md` P2–P6 按本决定重排。
