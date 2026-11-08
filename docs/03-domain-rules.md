# 03 · 业务口径手册

**这份文档是数字的唯一来源。** 任何界面、后端、推送文案要用到这些数字，都按这里的公式算，分母也必须一致。带 ⚠️ 的是当前原型与规则存在偏差、需要拍板的点（汇总在 `07-backlog.md`）。

**术语注（ADR-0012，2026-11-08）**：本系统只是一份账户信息快照账本，**不对接任何交易系统**——全文「委托 / 委托价 / 整手」等指录入成交时的价格数量字段与记账校验口径，不是真实交易指令；「不代下单」是表述纪律（不得让用户误以为系统能真实执行），不是能力边界。

## 1. 标的属性与交易单位

| `kind` | 名称 | 1 手 | 可卖数量 | 报价小数位 |
|---|---|---|---|---|
| `stock` | A 股普通股 | 100 股 | 整数股（买入按手；奇零股一次性全部卖出） | 2 |
| `etf` | 场内 ETF | 100 份 | 整数份（规则同股票） | 3 |
| `cb` | 可转债 | 10 张 | 整数张（规则同股票） | 3 |

- 数量校验（`validate_qty`，**B12 定稿 2026-10-30**）按序判定：
  1. `qty < 0` → 拒绝（`not_lot_multiple`）；
  2. `qty == 0` → 视为「未填写」合法（提交闸门另行要求 `qty > 0`）；
  3. **卖出且 `qty == held` → 合法（奇零股清仓豁免整手校验）**——A 股不足一手的持仓须一次性卖出（如送股后 150 股：可整笔清仓，不可拆成 50 + 100 分笔卖）；
  4. `qty % lot !== 0` → 拒绝「数量需为 N 的整数倍」（`not_lot_multiple`）——买入恒按整手，卖出部分减仓也须整手（150 股里卖 100 合法，剩 50 股奇零下次一次性清）；
  5. 卖出且 `qty > held` → 拒绝「最多可卖 X」（`exceeds_held`）。
- 快捷档位：按当前持仓（无持仓时按 10 手）的 10% / 25% / 50% 向下取整到手，卖出额外给「全部清仓」= `held`（奇零股持仓经上款豁免天然合法）。
- 报价小数位按 `kind` 决定（本表），唯一出处是 `data.js` 的 `priceDec(kind)`（P1-T3 起，B4 已修）：抽屉、成交流水、持仓表、dashboard、分析页、问答引用、微信卡片与诊断建议全部同精度，转债一律 3 位（`126.720`）。

## 2. 价格合法性（涨跌停校验）

P1-T2 定稿（2026-10-28，B2 消除）。涨跌幅比例是**标的属性**，不能完全由代码推断；显示与拦截用**同一对限价**、含边界；旧原型的 0.1% 容差与「−20%/+10%」不对称口径废除。

**比例（`limit_pct`）**——股票/转债按代码前缀推断，ETF 必须由数据层显式配置（P4 接行情源自动填充）：

| 标的 | 板块（代码前缀） | 涨跌幅 |
|---|---|---|
| 股票 | 沪深主板（`60` / `00` 开头，含原中小板 002/003） | ±10%；ST/*ST ±5% |
| 股票 | 创业板（`30` 开头：300/301/302） | ±20%（创业板无 ST 制度，`st` 输入被忽略） |
| 股票 | 科创板（`68` 开头：688/689） | ±20%（科创板暂无 ST 制度，同上） |
| 可转债 | 沪 `11` 开头（110/111/113/118）、深 `12` 开头（123/127/128） | ±20%（沪深 2022-08 转债交易规则；盘中临停由交易所负责，不在本程序口径内） |
| ETF | — | **返回 null**：无法由代码推断（159915 创业板 ETF ±20% 与 159919 沪深300ETF ±10% 同前缀），必须由行情源/数据层配置。参考：创业板/科创板相关 ±20%，其余股票型 ±10% |
| 北交所 | `8` / `4` / `920` 开头（±30%） | 超出产品范围（子木只覆盖沪深）→ null |

- ST 识别依赖行情字段（名称含 ST 或交易所风险警示板标记），P4 才有真实数据；`st` 是显式入参，**不做名称字符串猜测**。
- 原型演示样本无 ST，各标的 `limitPct` 在 `data.js` 手工配置（与上表参考值同源）。

**涨停价 / 跌停价（`limit_prices`）**——按标的报价小数位（§1：stock 2、etf/cb 3）四舍五入（half-up），与交易所算法一致：

```
units      = round(prev × 10^decimals)                          # 整数单元（分/厘），精确恢复
limit_up   = floor((units × (100 + pct) + 50) / 100) / 10^decimals
limit_down = floor((units × (100 − pct) + 50) / 100) / 10^decimals
```

必须用整数单元运算消除浮点噪声：裸浮点 `9.85 × 1.1 = 10.834999…`，按小数四舍五入会错得 `10.83`；整数单元给出与交易所一致的 `10.84`。`prev ≤ 0`（停牌/无行情）或 `pct ≤ 0`（未配置）→ 返回 null，界面显示 `—` 并跳过区间拦截。

**委托价校验（`validate_price`）**：`limit_down ≤ price ≤ limit_up` 合法，**含边界**（压线可成交是交易所语义）；显示文案与拦截判定必须用同一对限价。`reason` 枚举：`above_limit_up` / `below_limit_down` / `invalid_price`（`price ≤ 0`）。阈值比较带 `THRESHOLD_EPS` 守卫（附录比对规则第 5 条）。

范围外（本系统不对接交易系统，永远只做记账试算——见文首术语注）：新股/退市整理期等无涨跌幅场景、转债盘中临停、报价 tick（0.01/0.001 整数倍）校验。

## 3. 费用估算

```
佣金     = max(5, 成交金额 × 0.00025)          # 万 2.5，单笔最低 5 元
印花税   = 卖出 且 kind === 'stock' 时 成交金额 × 0.0005   # 0.05%，单向卖出
过户费   = kind === 'stock' 时 成交金额 × 0.00001        # 0.001%
总费用   = 佣金 + 印花税 + 过户费
```

实现见 `holdings.html:320`。ETF 与可转债免印花税、免过户费。

过户费对**所有**股票收取：自 2022-04-29 起沪深两市均按成交金额 0.001% 收取——「沪市过户费」文案已随 P1-T3（2026-10-28，B3）统一为「过户费 0.001%（沪深两市均收）」。

可转债「转股」的费用与税务和买卖不同，超出产品范围（本系统不对接交易系统，转股费用口径缺失，永不做成交假设）；未来若覆盖，先补本节口径再进实现。

> 界面必须始终带一句「实际以券商结算为准」。

## 4. 摊薄成本与已实现盈亏

```
成交额 amount = qty × price
费用      f   = fee(amount, kind, side)

# 买入
新持仓   afterShares = held + qty
新成本   afterCost   = (cost × held + amount + f) / afterShares     # 费用计入成本
现金     cashAfter   = cash − amount − f

# 卖出
新持仓   afterShares = held − qty
成本     不变
已实现   realized    = (price − cost) × qty − f
现金     cashAfter   = cash + amount − f
```

实现见 `holdings.html:352`。已实现收益率按 `realized / (cost × qty)` 展示。

**试算后的调整后仓位**（分母随本次交易变化，不能用当前总资产）：

```
others  = Σ(其他持仓市值)          # 不含本次标的
taAfter = others + afterShares × price + cashAfter
afterW  = afterShares × price / taAfter × 100
超上限判定：afterW > 25 → 抽屉里给「纪律提示」（holdings.html:361）
```

现金不足（`cashAfter < 0`）时数字标绿并在提交判定中拦截，不弹错误框。

## 5. 逐笔与组合派生指标

全部派生值统一由 `recalc()` 计算（`data.js:82`），页面不自行补算；行情跳动、手动刷新、记入交易都走它，算完自动同步到 `ZM` 导出对象（`publish()`，`data.js:80`）。分母为 0 时（空持仓）各比率取 0，不产生 NaN。

逐笔（`data.js:84`）：

```
dayChg   = (price − prev) / prev × 100        # 当日涨跌幅 %
dayVal   = (price − prev) × shares            # 当日盈亏额
mv       = price × shares                     # 市值
pnl      = (price − cost) × shares            # 累计浮动盈亏
pnlPct   = (price − cost) / cost × 100
weight   = mv / (totalMv + cash) × 100        # 占总资产（含现金），data.js:109
```

组合（`data.js:94`）：

```
totalMv      = Σ mv                     totalAssets = totalMv + cash
dayPnl       = Σ dayVal                 totalPnl    = Σ pnl
costTotal    = Σ cost × shares
totalPnlPct  = totalPnl / costTotal × 100
dayPnlPct    = dayPnl / (totalAssets − dayPnl) × 100   # 分母 = 昨日总资产
exposure     = totalMv / totalAssets × 100             # 仓位（不含现金口径的反面）
```

> `dayPnlPct` 用「昨日总资产」而不是「今日总资产」做分母，这是对的，后端实现不要改成当日均价。

**无行情降级（P4-T4 定稿，2026-10-28）**：行情字段缺失时**不伪造盈亏**——`price` 缺失以摊薄成本 `cost` 顶替（不产生假涨跌）；`prev` 缺失视为等于 `price`，即 `dayChg`/`dayVal` 取 0（「无数据 → 取 0」）。**禁止把缺失 `prev` 置 0**：那会让 `dayVal = price × shares`，把整笔市值算成当日盈利。停牌/`no_data` 标的在行情回写时保持库存旧值（过滤规则见 `04` 附录 B），旧值也没有时按上述顶替。

## 6. 仓位纪律（四条上限，分母不统一，务必照抄）

| 规则 | 阈值 | 分子 | 分母 |
|---|---|---|---|
| 单票占组合 | ≤ 25% | `mv` | **totalAssets（含现金）** |
| 单一行业占市值 | ≤ 35% | `Σ mv(sector)` | **totalMv（不含现金）** |
| 可转债合计占组合 | ≤ 12% | `Σ mv(kind=cb)` | **totalAssets（含现金）** |
| 现金最低保留 | ≥ 10% | `cash` | **totalAssets（含现金）** |

实现：`holdings.html:419`（规则清单）、`data.js:129`（`wp()` 占总资产）、`data.js:130`（`wsec()` 占市值）。

⚠️ 两个分母并存且都出现在文案里（「占总资产 22.6%」与「行业占持仓 35.1%」）。后端与推送文案必须**在数字旁边写清分母**，否则会互相看起来像算错了。建议字段命名固定为 `weightOfAssets` / `weightOfMarketValue`。

**降到目标仓位的卖出数量**（按手向上取整，`data.js:133`）：

```
sellTo(code, target%) = max(lot, ceil((mv − totalAssets × target/100) / price / lot) × lot)
```

`lot` 按第 1 节取值（股票/ETF 100、转债 10）。原型写死 100（只对股票正确），P3 起实现与黄金向量按泛化公式，见文末附录约定 1。

## 7. 净值与风险指标

输入：`equity.nav`（组合净值序列）与 `equity.bench`（基准，已归一到同一起点），日收益取相邻差比。

```
日收益      r[i] = nav[i]/nav[i−1] − 1                     # rb 同理取自 bench
均值       mean = Σr / n
标准差        sd = √(Σ(r−mean)² / n)          # 总体标准差，非样本
年化波动   annVol = sd × √244 × 100            # 244 个交易日
累计收益      cum = (nav[n−1]/nav[0] − 1) × 100
最大回撤     mdd = min over i ( (v − peak)/peak ) × 100，peak 为滚动前高
夏普         sharpe = (mean × 244) / (sd × √244 + 1e−9)   # 未扣无风险利率
胜率        winRate = count(r[i] > bench[i]) / n × 100      # 跑赢基准的天数占比
行业集中度      HHI = Σ wᵢ² / 10000                        # w 为百分比，结果归一 0–1
前三行业合计  top3Sector = Σ w(前 3)
```

**基准侧指标（B11 定稿 2026-10-30）**——`rb` = bench 的相邻差比收益，`nb` = len(rb)：

```
基准波动   benchVol = sd(rb) × √244 × 100      # 与 annVol 同算法（总体标准差、全序列）；
                                              # 原型 data.js 的 RMS 变体废除，两处实现统一到本式
配对前缀        m = min(n, nb)                   # 长度不等时按前 m 对配对（与 winRate 的 zip 语义一致）
协方差      cov = Σ_{i<m} (r[i]−mean_r') × (rb[i]−mean_rb') / m    # mean' 均取配对前缀 m 项
Beta       beta = cov / var_rb'                 # var_rb' = Σ(rb[i]−mean_rb')²/m；sd_rb' ≤ 1e−9 → 0
相关系数      corr = cov / (sd_r' × sd_rb')          # sd' 为配对前缀总体标准差；sd_r' 或 sd_rb' ≤ 1e−9 → 0
区间超额     excess = cum − benchCum              # 算术差（百分点），展示层直接相减，不进 perf 输出
```

- **ε 守卫（1e−9，复用 `THRESHOLD_EPS`）是口径的一部分，不是纯零判定**：常数收益序列在 f64 下带 ~1e−18 噪声（如 `[−0.02, 96.04/98−1]` 两个「相等」值差 1 ulp），纯零判定会放出 `beta ≈ 4×10¹⁴` 的天文数字；恒定基准/恒定净值无 beta 与相关系数可言，一律取 0。
- 除零一律取 0（`ratio` 守卫语义），不产生 NaN/Inf；`bench` 为空序列时 `benchVol / beta / corr` 全为 0（前端「基准不可用」形态）。
- 等长序列（生产常态：bench 与 nav 按快照日对齐）下 `m = n`，前缀均值/标准差与全序列一致。
- 原型 `diagnosis.html` 夏普缺 ε 守卫的问题随定稿消除：生产实现只认本节的 `+1e−9` 原式（两侧 domain 已带）。

实现：`data.js:161`（原型，冻结参照）。生产基准数据 = **sh000300 价格指数**日线点位归一到区间起点（P5-T2 dashboard/diagnosis）；§7 理想口径为沪深 300 **全收益**，价格指数未含分红的差异在前端图下与免责区如实标注，全收益数据源列后续增强。

## 8. 波动合成假设（再平衡清单的调整前后对比）

```
组合年化波动 = √( Σᵢ Σⱼ wᵢ wⱼ σᵢ σⱼ ρᵢⱼ ) × √244
σᵢ = 该标的近 30 日收益标准差
ρᵢⱼ = 0.35（跨行业） / 0.62（同行业）
```

- ρ 是**写死的假设值**，界面上已标注；接入真实协方差矩阵后应改为估计值并保留假设值回退。
- 该假设只用于「调整前后谁更大」的相对比较，不要在文案里当作绝对风险预测。

## 9. 市场时钟

时段边界与状态名（demo 与生产一致，前端展示文案的唯一来源）：

| 时段 | 状态 | 是否 live |
|---|---|---|
| 交易日 09:15 前 | 盘前 | 否 |
| 09:15–09:30 | 集合竞价 | 是 |
| 09:30–11:30 | 交易中 · 早盘 | 是 |
| 11:30–13:00 | 午间休市 | 否 |
| 13:00–15:00 | 交易中 · 午盘 | 是 |
| 15:00–15:30 | 盘后定价交易 | 否 |
| 15:30 后 | 已收盘 | 否 |
| 非交易日 | 休市 + 下一交易日 | 否 |

### 生产口径（P4-T4 定稿，2026-10-28）

`design/` 原型用「工作日 = 交易日」的演示启发式（`data.js:355`）；生产端服务端时钟（`zimu-market::clock`，随 `/api/market/status` 暴露）按以下口径：

- **时区**：恒 Asia/Shanghai（UTC+8 固定偏移，1991 年后无夏令时；产品范围仅沪深 A 股）。服务端取 UTC 转 +8 判断，**不读系统时区**——部署机时区设错口径也不漂。
- **交易日历**：来自 sidecar `/calendar`（新浪全量表 → baostock 兜底，见 `04` 附录 B）。`trade_dates` 是「今天是否交易日 / 下一交易日是哪天」的唯一依据，取代工作日启发式；调休上班的周六在日历里就是交易日。
- **节假日**：当天不在 `trade_dates` → `休市`，下一交易日 = 日历中晚于当天的最近一日——日历必须能解释「今天休市、下一交易日是 X 月 X 日」（原 ⚠️ 由此消除）。
- **降级**：日历不可用（sidecar 挂 / 全链 502）时退回工作日启发式，响应带 `calendar_degraded: true`——时钟继续走针，界面如实标注「未排除节假日」。降级只影响节假日判断，时段边界永不降级。
- 倒计时文案（「距开盘 X 分」）是显示层格式化：API 只给事实（`next_event` / `next_at` / `next_trade_day`），前端每秒自行走针，与原型 `cd()` 同构。

## 10. 分析结论字段语义

| 字段 | 含义 | 约束 |
|---|---|---|
| `action` | 加仓 / 买入 / 持有 / 减仓 / 卖出 / 定投 | 结论标签，必须与 `score` 同向 |
| `score` | 综合得分 0–100 | ≥70 偏多、55–69 中性、<55 偏空（`ZMC.gauge` 配色据此） |
| `conf` | **样本内信号一致性**，0–1 | 不是胜率，不得写成「有 72% 概率涨」 |
| `horizon` | 持有周期：短线 / 短中线 / 中线 1–3 月 / 长线 6 月+ | |
| `position` | 当前仓位 → 建议仓位 | 当前值必须是计算值 |
| `target` | 目标区间 `[低, 高]`，可为 null（如转债、定投） | |
| `stop` | 止损价，可为 null | 有 `action=买入/加仓` 时必须有 |
| `dims[4]` | 技术面 / 基本面 / 资金面 / 消息面各自分数与观察 | 固定四维，顺序不变 |
| `risks[]` | 这个结论可能错在哪 | **至少一条，不可为空** |

### 生产口径（P4-T6 定稿，2026-10-29）

上表是字段语义；生产端**数字全部由规则函数产出**（ADR-0007：数字归规则，文字归模型），
唯一实现 `zimu-domain::advice::score_advice`（TS 共享层镜像 `src/domain/advice.ts`），
黄金向量组 `advice.json`。模型只写 `reason` / `risks` 与结论组织文案，**文案中出现的
每个数字必须来自工具返回**（B6 生产侧口径）；`dims[].t` 观察文字由规则函数按事实模板
生成（展示层职责，不进 fixture，两边单测只断言关键事实存在）。

**输入**（全部可回溯：`closes/volumes` 来自 sidecar 日线（含当日合成 bar，`04` 附录 B）、
`price` 来自实时行情、`weight/held` 与现金纪律来自 `recalc`+纪律四条）：

```
kind, closes[]（升序日收盘）, volumes[]（同长，可 null）,
price（现价）, held, weight（占总资产 %）, cash_reserve_pass（现金 ≥10% 纪律）
```

**信号票**（每票 +1/0/−1；数据不足则该票**不参与**并如实记降级，不伪造）：

| 票 | 判据 | 最少样本 |
|---|---|---|
| v1 均线 | MA5 > MA20 → +1，< → −1，= → 0 | 20 根 |
| v2 价位 | 收盘 > MA20 → +1，< → −1 | 20 根 |
| v3 动量 | mom20 = close/close[−21] − 1：> +5% → +1，< −5% → −1，其余 0 | 21 根 |
| v4 区间位置 | pos30 = (close − low30)/(high30 − low30)（窗口 = 近 min(n,30) 收盘；high==low → 0.5）：> 0.9 → **−1（追高保护）**，< 0.2 → +1，其余 0 | 5 根 |
| vv 量能 | vol_ratio = 近5日均量/近 min(n,30) 日均量 ≥ 1.2 且 mom5 > 0 → +1；≥ 1.2 且 mom5 < 0 → −1；其余 0（两个均量窗口内任一 bar 缺量 → 不参与） | 量 ≥5 根 + 价 ≥6 根 |

**维度分与总分**：

```
技术面 t = clamp(50 + 12.5 × Σ(v1..v4), 0, 100)
资金面 m = vv 参与且 +1 → 70；−1 → 30；其余（含量能缺失/平量/缩量）→ 60
基本面 f = 60   消息面 n = 60        # 无数据源 → 中性值（中性带 55–69 的中值），接入后另改本节
score = (2t + m + f + n) / 5         # 权重 = 技术 0.4 / 资金 0.2 / 基本 0.2 / 消息 0.2
conf = 0.5 + |Σ全部可用票| / (2 × 可用票数)；可用票 < 3 → conf = 0.5（信号样本不足）
```

全中性（票数全 0）→ score = 56，落在「持有」带；§10 的 70/55 阈值与 gauge 配色**不动**。

**action 决策表**（纪律拦截优先于得分；`held=false` 时 weight = 0）：

| score 带 | 已持仓 | 未持仓 |
|---|---|---|
| ≥ 70 偏多 | weight > 25+ε → `持有`（建议降至 25%，单票纪律）；现金纪律未过 → `持有`（维持）；weight < 15−ε → `加仓`（建议 15%）；否则 → `持有`（仓位已达目标） | 现金纪律过 → `买入`（建议 15%）；否则 → `持有`（观望） |
| 55–70 中性 | ETF → `定投`；其余 → `持有`（维持） | ETF → `定投`；其余 → `持有`（观望） |
| 45–55 偏空 | `减仓`（建议 = weight/2，四舍五入 1 位） | `持有`（观望，不建仓） |
| < 45 偏空 | `卖出`（建议清仓） | `持有`（观望，不建仓） |

**horizon**：加仓/买入 → `中线 1–3 月`；定投 → `长线 6 月+`；持有 → `中线 1–3 月`（ETF `长线 6 月+`）；减仓 → `短中线 2–6 周`；卖出 → `短线`。

**target / stop**（锚 = 现价，按 §1 小数位 half-up：`floor(v × 10^d + 0.5) / 10^d`）：

| 情形 | target `[lo, hi]` | stop |
|---|---|---|
| 加仓 / 买入（股票、ETF） | `[price×1.08, price×1.15]` | `price×0.92` |
| 转债（任何持仓动作） | null（区间目标不适用） | `price×0.95` |
| 定投 | null | null（分批摊成本，无单点止损） |
| 持有 / 减仓 / 卖出（已持仓） | null | 股票/ETF `price×0.92`，转债 `price×0.95`（未执行时的防线） |
| 未持仓观望 / price 缺失 | null | null |

**position 输出**：`{current, suggested, mode}`——mode ∈ `target`（建议到目标仓位）/
`maintain`（维持）/ `dca`（定投）/ `exit`（清仓）/ `flat`（未持仓观望）；文案拼装是展示层
（工具侧格式化为「当前 x% → 建议 y%」写入 `advice_history.position`）。降到 25% 的
**卖出数量**用 §6 `sell_to`（调用方拿 `suggested` 当 target 调，逐位可回溯）。

**留档**：`calc_advice` 工具算完即写 `advice_history` 数字列（score/conf/target/stop/
position/dims + `generated_at` + `data_version`），返回 `advice_id`；模型文案经
`finalize_advice` 回填 reason/risks——**数字永不经过模型转写**（ADR-0007 的结构性保证）。
`data_version` 约定：`adv-v1|k={日线末bar日期|none}|q={行情as_of|none}`。

**降级**：日线获取失败 → 工具拒绝出建议（报错，不出全中性噪声结论）；实时行情失败但
日线可用 → price 取末根收盘并记降级；K 线不足 5 根 → 技术票全不参与（t = 50，观察
如实标注），conf 因可用票 < 3 归 0.5。**空持仓**：`held=false` 全部走「未持仓」列。

## 11. 数字展示规则

- 金额：`money(v)` → `¥1,234.56`（zh-CN 千分位）；大额用 `wan(v)` → `12.34 万`。
- 百分比：`pct(v)` → 正数带 `+`、负数带 `-`、零不带符号，两位小数。
- 涨跌类名：`dirClass(v)` → `up / down / flat`；`up` 红、`down` 绿（A 股惯例）。
- 所有数字 `tabular-nums` 等宽；代码、价格、时间走 `--font-mono`。
- 语义分离（P1-T4 定稿，2026-10-28；原 ⚠️ `up/down` 复用已清理，令牌与组件见 `05`）：
  - `up / down / flat` **只用于涨跌方向的带符号数字与标记**——价格涨跌、盈亏、超额收益、归因、K 线、回撤曲线，以及预警 `level: up/down/warn/flat` 词汇；
  - 合规/质量/反馈等**好坏轴**用 `.ok / .warn / .danger`（绿/黄/红红绿灯）：纪律「已触及/安全」、得分条与得分弧（高分 = ok，中 = warn，低 = danger）、风险指标对比基准、通道状态、反馈标记、资金不足提示；
  - **交易方向**（买入/卖出、加仓/减仓、结论标签）用 `.buy / .sell`——`--up/--down` 的别名令牌，沿用 A 股红买绿卖惯例，但类名与涨跌解耦；
  - 两套语义可同屏出现，禁止混类。

## 12. 预警与调度（P4-T7 定稿，2026-10-29）

预警引擎生产口径：调度器在交易日轮询行情，规则函数判定预警，结果写 `alerts` 表（schema 见 P4-T2）。**数字判定唯一实现** `zimu-domain::alert::eval_alerts`（TS 共享层镜像 `src/domain/alert.ts`），黄金向量组 `alerts.json`；调度编排（何时轮询、去重、落库、触发复盘）在 `zimu-agent::scheduler` 应用层——编排不含数字口径。

### 调度（何时判）

- **分钟级轮询**：每分钟读一次市场时钟（§9 生产口径：恒 Asia/Shanghai + 交易日历优先）。仅**交易日且 `live = true`**（集合竞价/早盘/午盘）时轮询；非交易日与非 live 时段空转，不打 sidecar。
- 每轮 = 全部持仓行情**合并一次请求**（`04` 附录盘中频率结论：新浪 hq 合并请求每分钟一轮可行）→ `apply_quotes` 回写 → `recalc` 重算 → 预警判定。轮询兼任盘中行情持续刷新（`/api/holdings` 数字保持新鲜）。**自选标的（watchlist，B13 定稿 2026-10-29）**：与持仓不重叠的自选代码另行合并一次 `quote`（sidecar 逐标的缓存 + 补齐兜底，`04` 附录 B）——自选行情**不回写库**（`holdings` 表只存持仓），只用于预警判定与界面展示。
- **行情失败不判定**：该轮跳过（不用库存旧价判——不伪造），降级如实记日志，下一轮再试。个别标的行情行状态非 ok 或 `price ≤ 0` → 该标的本轮跳过（`04` 附录 B 回写过滤同款语义）。
- **转债期限类检查每日一次**：强赎/到期字段来自 `/cb_list`（全量在市列表，重请求——不进分钟级轮询），交易日首轮轮询时执行（拉取条件 = 持仓**或自选**含转债）；强赎源降级（redeem 字段全 null）→ 当日强赎/到期类不参与判定，价格类照常。
- **15:05 盘后链**：交易日 ≥15:05 的首轮调度依次执行：
  1. 最终刷新（15:00 后行情冻结为收盘价，何时跑拿到的都是当日收盘数字）；刷新失败 → 整链跳过下一轮重试（**宁缺不写旧数**）；
  2. 写 `equity_snapshots`：每交易日一行，`date` 主键 INSERT OR REPLACE——幂等，服务重启不重复；列直接取 `recalc` 的 `agg` 原值（**不舍入**，逐位可回溯）：`total_assets / total_mv / cash / day_pnl / day_pnl_pct / total_pnl / total_pnl_pct / exposure / hhi / top3_sector`；
  3. 触发盘后复盘：向 agent 会话注入 `/复盘`（走 P4-T6 post-market-review Skill 链）；agent 离线 → 复盘不触发，`push_log` 如实记 `ok=0`，快照照写。
  **补跑**：当日快照不存在且时刻已过 15:05 → 任意一轮补跑整链（含白天宕机、晚间重启的情形）；快照已存在 → 整链跳过。
- 调度器可整体停用（`ZIMU_SCHEDULER_DISABLE=1`，开发/测试用）；停用不影响手动 `POST /api/market/refresh` 与 agent 工具。

### 预警判定（逐标的，固定顺序）

范围 = `holdings` 表全部持仓 **+ `watchlist` 表全部自选**（B13 拍板 2026-10-29：建自选表 schema v2，判定范围扩到自选）。输入 = 本轮实时行情 + 库存事实（cost / limit_pct）+ 最近一条 `advice_history` 留档（stop / target_hi）；`price` 缺失或 ≤0 → 该标的整行跳过。同一标的多条预警**全部产出**，输出数组序 = 下表判定序：

**自选按「无持仓形态」判定**：`cost / stop / target_hi` 一律不喂（None）→ 序 3/4/5 天然不参与——自选是观察性标的，不发持仓类预警；即便 `advice_history` 恰有该标的留档，也按无持仓处理（录入 `holdings` 后即恢复持仓形态全量判定，两种形态不会同时存在：持有中的代码不重复进自选拉取）。序 1/2/6/7/8 对自选与持仓同款：`limit_pct` 由自选表自带（数据层配置，同 `holdings` 语义），量能窗口走同一日级缓存，转债强赎/到期对自选转债同样适用。黄金向量锚：`al-limit-up` 等既有 case 的输入即无持仓形态（cost/stop/target_hi 全 null），判定函数对两种范围**零差别**——范围扩展是编排层（scheduler 喂什么）不是口径层。预警落同一 `alerts` 表、同日 `(code,type)` 去重跨范围共用；行内不记来源（code 即事实：非持仓即自选）。

| 序 | `type`（封闭词汇） | 判据 | `level` | 最少数据（缺 → 不参与判定，不伪造） |
|---|---|---|---|---|
| 1 | 触及涨停 | `price ≥ limit_up`（§2 限价，含边界） | `up` | `limit_up`：`prev > 0` + `limit_pct`（数据层配置优先，未配置按 §2 推断；推不出 → 不参与） |
| 2 | 触及跌停 | `price ≤ limit_down`（含边界） | `down` | 同上 |
| 3 | 跌破成本 | `price < cost`（相等不算跌破——压线算安全，附录规则 4） | `down` | `cost > 0` |
| 4 | 触及止损线 | `price ≤ stop`（含边界） | `down` | 最近一条留档的 `stop`；无留档 → 不参与 |
| 5 | 触及止盈线 | `price ≥ target_hi`（含边界） | `warn`（要动作的提醒，非方向语义） | 最近一条留档的 `target_hi`；无留档/留档为 null → 不参与 |
| 6 | 放量 | `volume_today ≥ 2.0 × vol_avg20`；`vol_avg20` = 近 20 根日线（**不含今日**，调用方截窗传入）中有效样本均量（null / ≤0 不计入样本） | 方向：`(price−prev)/prev ≥ 0` → `up`，`< 0` → `down`；`prev` 缺失或 ≤0（方向不可判）→ `warn` | 有效样本 ≥5 根 |
| 7 | 强赎公告 | cb 且 `redeem_status` 含「强赎」且不以「不」开头（如「已公告强赎」触发；「不强赎」不触发） | `warn` | 集思录强赎字段（源降级 → 不参与） |
| 8 | 临近到期 | cb 且 `0 ≤ last_trade_date − today ≤ 5` 自然日；`last_trade_date` 缺失用 `maturity_date`；已过期（差 < 0）不参与（数据异常，行情侧自会以停牌/退市跳过该标的） | `warn` | 同上 |

- `level` 词汇沿用 §11：`up/down` 只用于价格方向语义；`warn` 是要动作/注意类；引擎不产出 `flat`（盘后复盘不落 `alerts`，落 `push_log`）。
- 阈值压线的浮点噪声由 `THRESHOLD_EPS` 消化（附录比对规则第 5 条同款）。
- **去重**：同一 `(code, type)` 每自然日（`ts` 的上海日期前缀）至多一条——当日后续轮询条件仍成立也不再插入；跨日重新判定。去重需要库状态，属调度器职责；规则函数无状态。
- **`text` 文案**：规则函数按事实模板生成（展示层，与 §10 `dims[].t` 同约定——不进 fixture，数字只取输入值；价格按 §1 小数位、百分比带符号两位小数）。
- `kind` 非 cb 的标的，第 7/8 序不参与判定；非持仓事实（cost/stop/target_hi）对 ETF/转债同规则适用。

### 留档

- 预警新行写 `alerts`（`ts` 上海 `YYYY-MM-DD HH:MM`，`done=0`）；用户归档经 API 置 `done=1`，不删行。
- 复盘触发写 `push_log`：`channel="agent"`、`kind="post_market_review"`、`ok` = 是否成功注入会话、`detail` = 结果说明——`ok=1` 只表示复盘请求已被 agent 受理，模型产出是否完成以会话事件与 `advice_history` 留档为准，`push_log` 不代答。

### 常量（改这里 = 改口径，先动 fixture 再动两边实现）

放量倍数 `VOL_SURGE_MULT = 2.0`；均量窗口 20 根、最少有效样本 5 根；转债期限窗口 `CB_DEADLINE_DAYS = 5`（自然日）；盘后链时刻 `15:05`；轮询间隔 60 秒。

## 13. 持仓直接维护（快照校正，ADR-0009 定稿 2026-11-01）

交易录入（§1–§4）是**事件流**：一笔成交记录 → 费用、已实现盈亏、现金联动、`trades` 留档。直接维护是**快照校正**：初始化映射（券商存量持仓原样搬入）、录错纠正、场外变动（退市 / 债转股 / 红利零股——本就不经过买卖）。两者互补，不互相替代；CSV 批量导入仍是后续增强（ADR-0006）。

### 语义

- **新增 / 调整 = 整行覆盖（upsert）**：「看到什么抄什么」，不做增量 delta；cost 调整**不产生已实现盈亏**（校正不是卖出）。
- **删除 = 整行移除**：`trades` 历史保留（`holding_code` 非外键是既有设计），累计已实现盈亏不回滚——流水与持仓的连续性自校正点断开，界面文案必须如实说明。
- **现金不联动**（用户拍板 2026-11-01）：create / update / delete 均不动 `account.cash`；现金由既有「可用现金 · 调整」单独校正为真实资金——初始化场景钱早已花出去，联动会双重扣款。
- **不伪造流水**：直接维护不写 `trades`（零费用假交易会污染费用与已实现盈亏的审计口径）；审计痕写 `holding_edits` 表（op = create/update/delete + before/after 行快照 + note），与持仓写入**同事务**。
- **幂等**：upsert 与现有行可编辑字段逐位一致 → 不写库、不留痕、不报错。

### 校验（校正路径不套委托口径）

- `code`：6 位数字；`mkt` ∈ {SH, SZ}（显式字段不按代码猜，B7；北交所不在当前范围）；`kind` ∈ {stock, etf, cb}；`name` 非空。
- `shares`：**任意 ≥ 1 整数，允许奇零股**——红利送股 / 转股后的零股真实存在（schema CHECK 本就如此）；§1 整手校验只约束委托路径，卖出侧由 B12 奇零一次性清仓豁免承接。
- `cost`：≥ 0（券商摊薄成本口径，§4）。
- `limit_pct`：可选——留空则新建为 NULL、调整保留原值（§2 数据层配置语义）。
- **跳过 lot 校验、跳过涨跌停校验**（这不是委托）；`price` / `prev` 不可编辑——行情字段归刷新链回写（P4-T4），校正不得清空已回写的行情。

### 派生

recalc（§5）从 holdings 现值全部现算——维护后仪表盘 / 诊断 / 建议 / 纪律 / 15:05 快照即时生效，派生数字零特判（ADR-0008 后端唯一真值）。

## 附录 · 黄金测试向量（P3-T1，2026-10-27）

`fixtures/golden/` 是本手册数字口径的唯一权威样例集：Rust 领域纯函数库（P3-T3）与 TS 共享层（P3-T4）跑**同一份文件**。改任何公式 = 先改 fixture 与对应正文小节，再改实现——两边实现永远不许「各自对拍」，只对 fixture。

### 文件格式（schema_version 1）

每个公式组一个 JSON 文件，文件名 = `group`：

| 字段 | 含义 |
|---|---|
| `group` / `title` | 组 id / 中文名 |
| `spec` | 回指本手册章节 |
| `schema_version` | 结构版本，破坏性变更时递增 |
| `status` | `active` 参与测试；`pending` 口径未定稿，**runner 必须跳过**（机制保留；`price_limit` 已随 P1-T2 于 2026-10-28 定稿转 active，现全部组 active） |
| `tolerance` | 文件级默认容差 |
| `notes` | 组级口径备注（公式摘要、排除项及原因） |
| `cases[]` | `id`（组内唯一）、`fn`（多函数组必填）、`desc`（**手算过程，含精确分数**）、`tags`（`normal` / `boundary` / `empty` 三类；与持仓无关的组可不设 `empty`，如 `fees`）、`tolerance`（可选，覆盖文件级）、`input`、`expect` |

### 比对规则

1. **数值**：`|实现值 − 期望值| ≤ tol`。tol 取 case 级 ?? 文件级。期望值是手算小数：精确小数组用 `1e-9`；含无理数/循环小数且手算舍入到 6 位的组（`perf`、`vol_synth`、个别 case）用 `1e-5` 或 `1e-6`——容差吸收手算误差，但仍能抓住 `244→252`、分母用错这类口径错误。
2. **布尔 / 字符串 / 枚举**：精确相等。错误文案不进 fixture，只断言 `reason` 枚举码（如 `not_lot_multiple`），中文文案是界面层职责（`05`）。
3. **数组**：长度相同、按序对齐。`recalc` 的 `per` 与输入 `holdings` 同序；`sectors` 按市值降序——**排序本身是期望的一部分**。
4. **`null`**：实现必须返回空（如买入无 `realized`、清仓后 `after_cost`）。
5. **阈值判定**（`pass` / `over_limit`）：恰好压线走「含边界」语义（`≤` / `≥` 判 pass，`>` 判超限）；数值与阈值之差在 tol 以内时按「恰好等于阈值」处理，避免浮点噪声翻转边界用例。
6. **`expect` 未列出的字段不校验**（实现可以返回更多，如 UI 辅助字段）。

### 公式组 → 函数映射（P3-T3 / P3-T4 按此表实现）

| 组文件 | `fn` | 正文 | 输入 → 输出概要 |
|---|---|---|---|
| `lot.json` | `lot_size` / `price_decimals` / `validate_qty` / `quick_tiers` | §1 | `kind`(,`side`,`qty`,`held`) → 手数 / 小数位 / `{valid,reason}` / 档位数组 |
| `fees.json` | `fee` | §3 | `kind,side,qty,price` → `amount,commission,stamp_tax,transfer_fee,total` |
| `dilution.json` | `apply_trade` | §4 | `kind,side,qty,price,held,cost,cash,others_mv` → 摊薄成本 / 已实现 / 现金 / 调整后仓位与超限判定 |
| `recalc.json` | `recalc` | §5 | `cash,holdings[]` → `per[]` 逐笔派生 + `agg` 组合汇总 |
| `discipline.json` | `check` / `sell_to` | §6 | 纪律四条 `{value,threshold,pass}` / 降仓卖出数量 |
| `perf.json` | `perf` | §7 | `nav[],bench[]` → `cum,bench_cum,ann_vol,mdd,bench_mdd,sharpe,win_rate,bench_vol,beta,corr`（B11 定稿后十字段） |
| `vol_synth.json` | `vol_synth` | §8 | `items[{code,sector,w,sigma}]` → `ann_vol`（百分数） |
| `price_limit.json` | `limit_pct` / `limit_prices` / `validate_price` | §2 | `kind,code,st` → 涨跌幅比例（ETF/北交所 null）/ `kind,prev,pct` → 涨停与跌停价（整数单元 half-up）/ `price,limit_up,limit_down` → `{valid,reason}` |
| `advice.json` | `score_advice` | §10 生产口径 | `kind,closes[],volumes[],price,held,weight,cash_reserve_pass` → `action,horizon,score,conf,dims[4]{k,s},target_lo/hi,stop,position{current,suggested,mode},stats`（观察文字 `dims[].t` 是展示层，fixture 不校验） |
| `alerts.json` | `eval_alerts` | §12 | `today,item{kind,price,prev,limit_up/down,cost,stop,target_hi,volume_today,vol_hist[],redeem_status,last_trade_date,maturity_date}` → `alerts[]{type,level}`（按判定序；`text` 是展示层模板，fixture 不校验） |

### 本附录固定的口径（新增约定，与正文同级）

1. **`sell_to` 按 lot 泛化**：原型公式写死 100 股；泛化为 `max(lot, ceil((mv − totalAssets×target/100) / price / lot) × lot)`，转债按 10 张取整。§6 公式已同步更新。
2. **`perf` 短序列**：长度 < 2（无相邻收益）→ 全指标返回 0，不报错。
3. **清仓**：`after_shares = 0` 时持仓行移除，`after_cost` 记 `null`。
4. **纪律压线**：恰好等于阈值算安全（见比对规则第 5 条）。
5. **费用输出不舍入**：§3 公式结果保留原始精度（估算口径）；分位舍入是展示层与券商结算的事，领域库不做。

### 已知缝隙（历史存档；两条均已定稿并入正文与 fixtures）

- **B11** ✅ 定稿（2026-10-30，§7「基准侧指标」）：`benchVol` 统一为总体标准差（RMS 变体废除）、`beta`/`corr` 按配对前缀 + `1e−9` ε 守卫（恒定序列的 f64 噪声不得放大成天文 beta）、`区间超额` = 算术差（展示层相减，不进 perf 输出）、`sharpe` ε 守卫两侧 domain 本已带（原型 diagnosis 页缺守卫随冻结作废）。`perf.json` 十字段全校验（新增 `pf-beta-corr` 锚点）。
- **B12** ✅ 定稿（2026-10-30，§1 判定顺序 ①–⑤）：卖出 `qty == held` 清仓豁免整手校验（A 股奇零股须一次性卖出），部分卖出仍须整手倍数、买入恒整手。`lot.json` 的 `vq-sell-odd-lot` 翻为合法，另加部分奇零/留奇零/超持仓顺序/转债奇零四例。
