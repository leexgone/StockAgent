# CLAUDE.md — 青梧 · 股票智能助理（StockAgent）

个人股票助理程序。当前阶段：**纯前端界面原型 + 规格文档**，无后端、无构建、无第三方依赖。

## 现在这个仓库里有什么

```
design/            界面原型：7 个 html + assets/{ui.css, data.js, charts.js}
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

`design/README.md` 是原型自带的说明书，内容若与 `docs/` 冲突，**以 `docs/` 为准**（收敛为单一来源已列入 `docs/07-backlog.md`）。

## 工程约定

**技术形态（受 ADR-0002 / ADR-0003 约束，现阶段不要改变）**
- 不引入打包器、框架、CDN 依赖、npm 包；保持 `html + css + js` 零构建。
- 后端技术栈（Python/FastAPI vs Node/TS）**尚未决定**，不要提前建 `backend/`、`src/`、`package.json`、`pyproject.toml`。
- 行情/持仓数据**全部是演示样本**，不要接入真实行情源，也不要把示例持仓换成任何真实账户数据后提交。

**代码规范**
- 页面：每屏一个自执行 `<script>`，通过 `window.QW`（数据/工具）与 `window.QWC`（图表）取用共享能力；页面开头调用 `D.chrome({ active, crumb, onRefresh })` 渲染外壳，`onRefresh` 里重绘本页读汇总数据的区块（顶栏刷新按钮会先跳动行情并重算，再回调它）。
- 派生数字一律现算，不写死字面量；需要新指标时加进 `data.js`，别在页面里就地算完写进文案。
- 逐笔派生与全部汇总只出自 `QW.recalc()`：价格、持仓、现金变动后必须调用（`tickPrices` 内部已调）；页面不得自行补算派生值、不得给 `QW` 挂新属性，改现金要写 `QW.ws.cash` 而不是 `QW.cash`。
- 每个可走查的区块带 `data-od-id="..."`（kebab-case，重复项用 `-0`、`-1` 下标），设计走查与自动化截图依赖它。
- 每个页面底部保留 `*-disclaimer` 免责区块，新增界面不能漏。
- 涨跌配色按 A 股惯例：`--up` 红 = 涨，`--down` 绿 = 跌，只用于数字与标记；这两个类**不要复用来表达「合规/违规」「好/坏」**（语义已经不够用了，见 backlog）。
- `--accent` 每屏不超过两处，只用于交互态。

**中文文案**
- 界面文案与文档用简体中文，技术标识符保持英文；数字用 `font-variant-numeric: tabular-nums`，价格/代码走 `--font-mono`。
- 金额、百分比的格式化统一走 `QW.money / fmt / pct / wan`，不要手搓 `toFixed` 拼百分号。

## 验证方式

没有测试框架，改完原型请人工过一遍（浏览器控制台必须无报错）：

1. 直接双击打开每个页面（`file://`，注意跨页依赖会在这种方式下暴露）。
2. `python -m http.server` 后访问，逐项检查：控制台无错误、侧栏/顶栏/底栏渲染正常、数字与图表口径一致。
3. 窄屏：拖到 900px 以下确认侧栏转抽屉、底部标签栏可点。
4. 交易时段相关逻辑用系统时间验证（`QW.marketClock()` 读真实时钟）。

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
- 当前明确暂缓：后端栈、前端框架、真实行情源。讨论有结论后再落 ADR。

## 安全与责任边界

程序只给建议，不代下单。每条建议必须带目标价/止损/失效条件与风险自陈；`conf` 指样本内信号一致性，不是胜率，不要在文案里改成收益承诺或确定性表述。
