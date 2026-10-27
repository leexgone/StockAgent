# 07 · 已知问题与待决策

分五级：**阻断**（现在就是坏的）、**一致性**（数字/口径互相打架）、**待决策**（需要人拍板）、**框架反馈**（robit 的重构需求，见 ADR-0005「与 robit 的协作模式」）、**已修复**（存档，保留验证方式）。`06-roadmap.md` 的 P1 就是清掉前两级。

## 阻断

### A2 · 首笔持仓没有入口

所有界面假设账户里已经有 10 只标的。真实用户第一次打开时持仓为空，`totalAssets = cash`。除零防护已随 A1/B1 修复做进 `recalc()`（空持仓时各比率为 0，不再是 NaN，已在沙箱验证），但「录入第一笔」的空态与引导路径仍然缺失（`.empty` 样式已存在，见 `ui.css:465` 分区）；`dashboard.html` 的头图与收盘摘要、`index.html` 的问候语在空持仓下会因取 `holdings[0]` 报错，也要一并处理。关联决策 C5。

## 一致性

### B2 · 涨跌停口径三处偏差

见 `03` 第 2 节：板块未区分（创业板/科创板实际 ±20%，ST ±5%）、转债上下限不对称（−20%/+10%）、提示文案与拦截阈值差 0.1%。

### B3 · 过户费文案过时

代码对所有股票收取过户费（`holdings.html:323`，注释却写「沪市」）。2022-04-29 起沪深两市均收，**代码对、文案错**：改 `design/README.md` 与 `holdings.html:111` 的提示文字。

### B4 · 可转债小数位不统一

抽屉按 `kind === 'etf' ? 3 : 2`（`:278`），流水按 `price < 10 ? 3 : 2`（`:389`）。转债在抽屉里显示 `126.72`，在流水里可能显示 `122.800`。

### B5 · `up / down` 类被复用于非涨跌语义

`holdings.html:433` 用 `up` 表示「已触及纪律上限」、`:388` 用 `up/down` 表示买入/卖出、`analysis.html:315` 用它们表示维度得分高低、`charts.js:158` 的 gauge 用它们表示分数。在红涨绿跌的产品里，「红色 = 危险」和「红色 = 上涨」会同时出现且互相冲突。需要引入 `--ok/--warn/--danger` 的用法并逐处替换。

### B6 · 演示数字靠正则改写中文句子维持一致

`data.js:259` 用 `replace(/当前 [\d.]+%/g, …)` 把写死在文案里的百分比覆盖成计算值。目前能对齐，但只匹配「当前 X%」「占总资产 X%」两种句式——`advice` 里还有「减 200 股」「回落到 25% 以下」等句子不受保护。接真实数据后应改为模板插值。

### B7 · 交易所靠代码首字母猜

`analysis.html:284` 用 `code[0] === '6' ? 'SH' : 'SZ'`，而 `holdings` 已有显式 `mkt` 字段。`588000`（科创板 ETF，SH）能被猜对是巧合，`002415`（SZ）也是；`5` 开头的深市 ETF 会猜错。用 `mkt`。同样的猜测还出现在 `holdings.html` 交易提交处新建持仓的 `mkt` 字段。

### B8 · 死代码

`holdings.html:428` 的 `r.inverse ? c / r.v * 100 : c / r.v * 100` 两个分支完全相同，「现金最低保留」的进度条因此按「越大越好」画，与另外三条规则的视觉语义相反。

### B9 · 历史建议与 `advice` 无关

`analysis.html:329` 的 `buildHistory()` 用的是页面内生成的样本，没有数据层的建议留档结构。接真实数据后「回看当时怎么说的」需要 `adviceHistory`（见 `04` 第 6 条），届时这块文案与来源都要重做。

### B10 · 演示样本含已退市/摘牌标的

P2-T1 spike（2026-10-26）实测：`110074 精达转债` 新浪行情定格（open=0、volume=0、价格停在 222.81，已退市强赎）；`127022 恒逸转债` 新浪返回空串（已摘牌）；`113650 博威转债` 待核实。股票/ETF 抽查存活（600519/510300 有正常行情）。影响：接真实行情后这两只的报价为空/定格，`alerts` 里「强赎公告 11-20」等文案与现实脱节；`holdings` 样本还会流入净值与建议的演示数字。修法：P4 数据管道落地时把转债样本换成存活标的（从 `bond_zh_hs_cov_spot` / `bond_cb_redeem_jsl` 现存活名单里选），管道加「标的存活性校验」步骤，`design/` 与未来 seed 同步替换。

### B11 · 基准侧风险指标口径未定

P3-T1 fixture 设计时发现（2026-10-27）：① `data.js:177` 的 `benchVol` 用基准收益的 RMS（不减均值），`diagnosis.html:155` 的 `bvol` 用标准差（减均值）——同名指标两处算法不同；② `diagnosis.html:158` 的夏普没有 `1e-9` 守卫，sd=0 时除零，与 `03` §7 公式不一致；③ 诊断页的 beta、与基准相关系数、超额收益未收入 `03` §7。黄金向量（`fixtures/golden/perf.json`）暂不校验 bench_vol/beta/相关系数；定稿时先补 §7 正文，再加 fixture 用例并统一 `design/` 两处实现。

### B12 · 奇零股卖出规则与 §1 冲突

`03` §1 表格写股票「可卖数量 = 整数股」（A 股卖出允许奇零股、须一次性卖出），但原型买卖一律按 `qty % lot === 0` 拦截（`holdings.html:342`），且「全部清仓」按钮对奇零股持仓（如送股后 150 股）填入 `held` 后会被自己的校验拒绝。fixture 暂按原型行为编码（`lot.json` case `vq-sell-odd-lot`）；定稿后同步改 §1、原型与用例。关联 P4-T2（schema 要能存奇零股持仓）。

## 待决策

| # | 问题 | 备注 |
|---|---|---|
| C1 | 后端技术栈 | ✅ 已决（2026-10-25，ADR-0005）：Rust + robit agent 基建，独立仓库 `backend/` workspace，axum API |
| C2 | 前端是否框架化、何时 | ✅ 已决（ADR-0005）：React + TS + Vite（`frontend/`）；`design/` 原型保留为参照直至迁移完成 |
| C3 | 真实行情源 | ✅ 已决（ADR-0005）：akshare 主 + baostock 兜底，经 Python sidecar（`datafetch/`）HTTP 桥接；P2 spike 验证字段、频率与限频 |
| C4 | 微信接入方式 | 🔶 部分已决（ADR-0005）：架构走 robit-chatbot `PlatformAdapter` 可插拔，先落 console/文件适配器；**微信渠道仍未定**，调研必须同时回答：① 能否**双向会话**（产品要求微信/QQ 会话入口，不只是推送）② 是否需要**公网入站**（企业微信自建应用回调需公网地址或内网穿透；群机器人/Server酱只发不收；测试号不长期稳定）。QQ 侧不受此困——`robit-qq` 出站 WS 网关现成，可作第一个真实适配器。P5 前拍板 |
| C5 | 第一笔持仓怎么录入 | 手工逐笔 / 券商对账单 CSV / 截图识别。影响空态设计与存储格式；**P4 开工前拍板**（闸门见 `06` 总览），P1-T1 的空态壳可先行 |
| C6 | 仓位纪律阈值是否随账户规模浮动 | 25% / 35% / 10% / 12% 目前写死；小账户的单票上限是否该更宽 |
| C7 | 波动合成的相关系数是否用估计值 | 现在是 0.35 / 0.62 假设值；估计需要更长样本与行业分类稳定性 |
| C8 | 分析结论由什么生成 | 🔶 架构已决（ADR-0005）：agent + 领域工具 + Skill 模板；打分方式（规则 / 模型 / 混合）仍未定，P4–P6 之间拍板，影响 `score`、`conf`、`dims` 的可解释性要求 |
| C9 | 数据存储位置 | ✅ 已决（ADR-0005）：SQLite（rusqlite bundled），单文件、gitignored，备份 = 拷文件；持仓仍是敏感数据，永不入库（git 意义上的库） |
| C10 | 许可与版权 | `LICENSE` 是 Apache-2.0 正文，缺少先常见的版权声明附录段落；要不要补 `NOTICE` |
| C11 | `design/README.md` 与 `docs/` 收敛 | 内容重叠（业务规则、数据契约、设计约定）。建议 `design/README.md` 保留「怎么打开原型」，其余改为指向 `docs/` 的链接 |

## 框架反馈（R 系列）

子木消费 robit 的 trait/API 时发现的形状痛点记录在这里，作为 robit 重构的需求来源（协作原则见 ADR-0005：框架级缺口先改 robit，不在子木侧写绕过 trait 的补丁）。每条落定时同步到 robit 仓库自己的 issue/roadmap。

| # | 痛点 / 需求 | 状态 |
|---|---|---|
| R1 | **Web/SSE 桥缺官方实现**（P2-T3）：robit 的 `Frontend` trait 只有 CLI/TUI 形态的参考实现；Web 宿主需要自己写 AgentEvent→wire 类型投影、确认槽位（`request_tool_confirmation` 挂起等 HTTP 回填）、broadcast 多客户端扇出。子木的实现见 `backend/crates/zimu-server/src/agent_bridge.rs`——确认语义与事件投影都是宿主无关的重复劳动。诉求：robit 提供可选 web 桥 crate（SSE/WS 的 Frontend 通用实现 + 消息注入约定），或至少给 `AgentEvent` 一个官方序列化 schema | ✅ 2026-10-27 落地（P2-T5）：robit v0.1.26 新增 `robit-web` crate——`WebEvent` 官方 JSON schema、`WebFrontend`（broadcast 扇出 + 确认槽位）、`WebBridge`（消息注入）；子木 zimu-server 已切换消费（pin tag v0.1.26），宿主手写桥删除。尾巴：robit 分支 `feat/web-bridge` 待合并推送、crates.io 待发布，之后子木 pin 换回 registry 写法 |
| R2 | **ToolRegistry 装配后不可扩展**（P2-T2）：`bootstrap()` / `create_tools_from_config()` 返回不可变 `Arc<ToolRegistry>`，领域工具无法追加进内置工具面；只能绕过 bootstrap 手动 `ToolRegistry::new()` + `register()`（代价：丢掉 read/bash 等通用工具）。诉求：`bootstrap_with(extra_tools)` 或让 registry 支持装配后注册 | ⬜ 待提给 robit |
| R3 | **init_logging 独占全局 tracing subscriber**（P2-T3）：宿主服务自带日志初始化时二者只能选其一——zimu-server 现按「有 robit 配置就交给 init_logging（靠 additional_directives 补 tower_http），否则回落自建 fmt」分支处理，且失去对日志格式的自主权。诉求：提供 tracing `Layer` 形态，由宿主组合进自己的 registry | ⬜ 待提给 robit |

## 已修复（存档）

### A1 · `analysis.html` 打开即报错，后半屏空白 —— 2026-10-24 修复

**原现象**：共享数据层不产出 `weight`，也没有 `totalMvAll()`，两者都是 `holdings.html` 页内自己补的全局（旧版 `:192`/`:196`）。分析页独立加载时 `render()` 在构建执行计划处抛 `TypeError`，**执行计划、风险自陈、历史建议、30 日 K 线、建议排序表全部不渲染**；首页「今天需要你决定」第一条恰好链到 `analysis.html?code=002594`。

**修复**：收进契约层——`recalc()` 统一派生 `h.weight`（`data.js:109`，与 `wp()` 同分母），`totalMvAll()` 由 `data.js:125` 导出；删除 `holdings.html` 的本地定义。分析页**没有**加 `|| 0` 兜底（那会把报错变成静默的 0.0%）。

**验证**：vm 沙箱加载 `data.js`，复现原崩溃表达式可正常求值；每只持仓 `weight` 存在且 `Σweight === exposure`、`weight.toFixed(1) === wp(code)`；七个页面内联脚本全部通过语法编译。

### B1 · 行情跳动后同一屏内数字自相矛盾 —— 2026-10-24 修复

**原现象**：`tickPrices()` 只改 `price/dayChg`；`totalAssets / dayPnl / sectors / hhi / exposure` 等是加载时算一次的常量；各页定时器重算范围不同（holdings 每 6s 重算逐笔、dashboard 每 5s 只重绘净值、其余页面不动），行情条「我的总资产」永远是加载值 → 同一屏多个模块互相对不上。

**修复**：

- `data.js`：汇总改为 `agg` 快照，`recalc()` 统一重算（含逐笔派生与 `weight`）并 `publish()` 回 `ZM`（`data.js:76-123`）；`tickPrices()` 内部调用 `recalc()`、重绘行情条，并**返回价格是否真的变动**（`data.js:492`），休市时自动跳动直接跳过；`chrome()` 新增 `onRefresh` 回调，顶栏刷新按钮跳动后通知页面重绘。
- 页面：dashboard 收拢出 `renderAll()`（头条/净值/配置/归因/持仓表/摘要一起重绘，删掉了只重绘净值的冗余 60s 定时器）；holdings 定时器与刷新回调走同一组渲染函数；analysis `render()`、index `renderHead()`、chat `renderCtx()`、diagnosis `renderAll()` 接入 `onRefresh`。wechat 页不接（其卡片是历史推送样式，非实时视图）。
- 现金改动的唯一源头是 `ws.cash`（交易页提交处），`recalc()` 后重新发布为 `ZM.cash`，不再直接写 `D.cash`。

**验证**：vm 沙箱断言「交易改动 → `recalc()` → 导出快照同步」「手动 tick → 聚合与逐笔自洽」「休市自动 tick 返回 `false` 且价格不动」「空持仓无 NaN」，25 项全部通过（脚本在 `.file-versions/check-contract.js`，该目录被 gitignore，不入库）。
