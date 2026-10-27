# CLAUDE.md — 子木 · 股票智能助理（StockAgent）

个人股票助理程序。当前阶段：**P3 口径可执行化收官（2026-10-28）**——黄金测试向量（`fixtures/golden/`）八组 76 用例两边全绿：Rust `zimu-domain`（`cargo test -p zimu-domain`）与 frontend TS 共享层 `src/domain/`（`npm test`）跑同一份 fixture，无 pending 组（`price_limit` 已随 P1-T2 涨跌停口径定稿转 active）。P2 技术基座已收官（axum + robit SSE / React 工作台 / Python 行情，Web 桥沉淀为 robit `robit-web` v0.1.26）。P1 原型定稿推进中（T2–T8 ✅，余 T1 空态壳、T9 人工验收）。下一步主线 P1 → P4（数据管道，前置 C5/C8 拍板）。`design/` 零构建原型保留为设计参照与验收基准。技术路线定案见 ADR-0005：Rust + robit agent 基建 / React + Vite 前端 / Python 行情 sidecar。

产品形态：发布后是**常驻 Web 服务**（浏览器访问）+ 微信/QQ 双向会话入口，不是桌面程序——这一点与 robit（TUI/GUI 桌面端）有意区分，做架构决定时别把两者混同。

## 现在这个仓库里有什么

```
backend/           Rust 服务（Cargo workspace）：crates/zimu-server（axum，health + SPA 静态托管）
                   crates/zimu-agent-cli（robit 集成 spike：Agent REPL + echo 工具，P2-T2）
                   crates/zimu-domain（领域纯函数库，P3-T3：03 数字口径唯一 Rust 实现 + 黄金向量测试）
frontend/          React + TS + Vite 前端：index 工作台静态复刻（P2-T4）+ 智能问答视图（P2-T3 SSE 会话）；
                   src/domain/ 为领域纯函数 TS 共享层（P3-T4：镜像 zimu-domain，vitest 跑同一份黄金向量），令牌在 src/styles/tokens.css
datafetch/         Python 行情 sidecar（uv + FastAPI + akshare），P2-T1 spike 产物
design/            界面原型：7 个 html + assets/{ui.css, data.js, charts.js}
fixtures/golden/   黄金测试向量（P3）：数字口径的权威样例集，Rust 领域库与 TS 共享层跑同一份；schema 见 03 附录
docs/              产品规格与工程规范（编号即阅读顺序）
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
- `design/` 原型保持零构建（不引入打包器、框架、CDN、npm 包），直到 `frontend/` 迁移完成前它都是设计参照与验收基准。
- 技术路线见 ADR-0005：`backend/`（Rust workspace，依赖 robit）、`frontend/`（React + TS + Vite）、`datafetch/`（Python sidecar，uv + FastAPI + akshare/baostock）。目录已随 P2 spike 创建（datafetch P2-T1 ✅；backend + frontend 最简框架 2026-10-27 立起；P2-T2 ✅——`zimu-agent-cli` 直接消费 crates.io 的 robit 0.1.25，流式对话 + echo 工具 function-calling 验收通过；P2-T3 ✅——Agent SSE 桥进 zimu-server（`/api/chat*` 端点 + ChatPanel，浏览器/curl 均见流式输出）；P2-T4 ✅——index 工作台静态复刻进 React（Chrome 外壳 + 六区块，`demo.ts` 同口径移植 `data.js`，41 项派生数字与原型逐位一致；会话面板移入「智能问答」视图；P2-T5 ✅——共同演进回路首次走通：R1 沉淀为 robit `robit-web` crate（`WebEvent`/`WebFrontend`/`WebBridge`，v0.1.26 本地 tag），zimu-server 以 git pin 消费、宿主手写桥删除）。前端版本基准对齐 robit-gui（React 19 / Vite 6 / TS 5.6），后端 crate 版本对齐 robit workspace，升级前先想两边。
- 两工程共同演进：框架级缺口先改 robit（`E:\Github\robit`，改动保持通用性、走它自己的发布流程），子木 pin tag/rev 消费（开发期可 path override）；痛点记入 `docs/07-backlog.md` R 系列，**不要写绕过 robit trait 的补丁**，领域逻辑不渗进 robit。回路已随 P2-T5 演练走通：robit 新基线分支 `feat/web-bridge` → tag `v0.1.26` → 子木 `backend/Cargo.toml` git pin（演练期 `file://` 指向本地仓库；robit 推送 + crates.io 发布后换回 registry 写法，见 `backend/README.md`）。
- `design/` 原型只用演示样本；真实行情自 P2 起经 datafetch sidecar 进入。**真实持仓、券商账户数据、任何 API key 永不入库。**

**代码规范**
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

数字口径有自动化验收：两边 runner 跑**同一份** `fixtures/golden/`——backend `cargo test -p zimu-domain`（Rust 领域库），frontend `npm test`（vitest，TS 共享层）。改口径的顺序固定：**先改 `docs/03` 与 fixtures，再改两边实现**；实现之间永不互相对拍。

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
- 后端栈、前端框架、真实行情源已由 ADR-0005 定案；当前待拍板：C5 录入方式（P4 前）、C8 打分生成（P4-T6 前）、C4 微信渠道（P5 前），见 `06-roadmap.md` 决策闸门表。

## 安全与责任边界

程序只给建议，不代下单。每条建议必须带目标价/止损/失效条件与风险自陈；`conf` 指样本内信号一致性，不是胜率，不要在文案里改成收益承诺或确定性表述。
