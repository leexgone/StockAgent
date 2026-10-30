# 子木 · 股票智能助理 — 界面原型

纯前端 HTML / CSS / JS 原型：零构建、无外部依赖、断网也能跑。用来验证一个**个人股票助理**的信息架构与交互 —— 持仓管理、行情追踪、AI 分析与买卖建议、组合诊断、微信推送接入。

> **🧊 已冻结（P5-T3，2026-10-30）**：本目录是**设计参照与验收基准**，不是活实现——生产实现在 [`frontend/`](../frontend/)（React + Vite，已逐屏接后端真实数据 / API）。
>
> - 7 屏中 **index / dashboard / holdings / analysis / chat / diagnosis 已移植**（真实行情、录入事务、建议留档、Agent 会话、风险指标全部接线，数字以后端为唯一真值）；**wechat 屏待 C4 微信渠道拍板**后补，故原型暂留该屏作参照。
> - 此后原型**只保留、不再新增功能**：任何界面 / 规格改动走 `frontend/` 与 `docs/`；验收对照时以本目录为像素与交互基准。
> - 原型内行情、持仓、净值、分析结论仍全部来自 `assets/data.js` 的**演示样本**（未接行情源与模型，不构成投资建议）——这正是它与 `frontend/` 真实实现的关键区别，别把原型数字当真实数据。

## 打开方式

直接双击 `index.html` 即可。想用本地服务：

```bash
python -m http.server 8000 --directory design
# 浏览器打开 http://localhost:8000/
```

原型会把演示状态存进浏览器 localStorage（问答会话、微信接入配置）；演示时不想带上你点过的状态，用无痕窗口打开。键名与清理方式见 `docs/04-data-contract.md`「浏览器本地状态」。

任意页面加 `?empty=1` 可走查**空持仓状态**：持仓、预警、问答、建议、净值全部清空，7 屏显示空态引导；「录入第一笔」入口就是持仓页的表单抽屉（手工逐笔先行）。该状态存在 sessionStorage，跨页保持、关标签即退出；`?empty=0` 随时恢复演示数据。

## 规格写在哪

本 README 有意**不复制任何规格**——单一来源在 `docs/`（编号即阅读顺序）：

| 想知道… | 去这里 |
|---|---|
| 产品定位与边界（明确不做：港股美股、场外基金、代下单等） | [`docs/01-vision-and-scope.md`](../docs/01-vision-and-scope.md) |
| 7 屏各是什么、逐区块看什么 | [`docs/02-screens.md`](../docs/02-screens.md) |
| 业务规则与数字口径：整手、涨跌停、费用、摊薄成本、仓位纪律、波动测算 | [`docs/03-domain-rules.md`](../docs/03-domain-rules.md) |
| `assets/data.js` 数据契约（`window.ZM` / `window.ZMC`，换真实数据只改这一层） | [`docs/04-data-contract.md`](../docs/04-data-contract.md) |
| 设计令牌与组件约定：色彩语义（涨跌 / 好坏 / 交易方向三套）、排版、断点、`data-od-id` 走查锚点 | [`docs/05-design-system.md`](../docs/05-design-system.md) |
| 已知问题与待决策 | [`docs/07-backlog.md`](../docs/07-backlog.md) |
| 路线图与阶段验收标准 | [`docs/06-roadmap.md`](../docs/06-roadmap.md) |

原型的工程约定（零构建、`recalc()` 唯一重算入口、派生数字一律现算等）见根目录 `CLAUDE.md` 与 `docs/05`。

## 许可

Apache-2.0，见仓库根目录 `LICENSE`。
