# 03 · 业务口径手册

**这份文档是数字的唯一来源。** 任何界面、后端、推送文案要用到这些数字，都按这里的公式算，分母也必须一致。带 ⚠️ 的是当前原型与规则存在偏差、需要拍板的点（汇总在 `07-backlog.md`）。

## 1. 标的属性与交易单位

| `kind` | 名称 | 1 手 | 可卖数量 | 报价小数位 |
|---|---|---|---|---|
| `stock` | A 股普通股 | 100 股 | 整数股（买入按手） | 2 |
| `etf` | 场内 ETF | 100 份 | 整数份 | 3 |
| `cb` | 可转债 | 10 张 | 整数张 | 3 |

- 数量校验：`qty % lot === 0`，否则报「数量需为 N 的整数倍」（`holdings.html:342`）。
- 卖出不得超过当前持仓：`qty > held` 时报「最多可卖 X」（`holdings.html:343`）。
- 快捷档位：按当前持仓（无持仓时按 10 手）的 10% / 25% / 50% 向下取整到手，卖出额外给「全部清仓」（`holdings.html:334`）。
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

范围外（P4 接真实委托链路时再议，本程序只做记账试算）：新股/退市整理期等无涨跌幅场景、转债盘中临停、报价 tick（0.01/0.001 整数倍）校验。

## 3. 费用估算

```
佣金     = max(5, 成交金额 × 0.00025)          # 万 2.5，单笔最低 5 元
印花税   = 卖出 且 kind === 'stock' 时 成交金额 × 0.0005   # 0.05%，单向卖出
过户费   = kind === 'stock' 时 成交金额 × 0.00001        # 0.001%
总费用   = 佣金 + 印花税 + 过户费
```

实现见 `holdings.html:320`。ETF 与可转债免印花税、免过户费。

过户费对**所有**股票收取：自 2022-04-29 起沪深两市均按成交金额 0.001% 收取——「沪市过户费」文案已随 P1-T3（2026-10-28，B3）统一为「过户费 0.001%（沪深两市均收）」。

可转债「转股」的费用与税务和买卖不同，超出产品范围（子木只记账不代下单）；未来若覆盖，先补本节口径再进实现。

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
日收益      r[i] = nav[i]/nav[i−1] − 1
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

实现：`data.js:161`。基准为**沪深 300 全收益**，指数点位归一。

## 8. 波动合成假设（再平衡清单的调整前后对比）

```
组合年化波动 = √( Σᵢ Σⱼ wᵢ wⱼ σᵢ σⱼ ρᵢⱼ ) × √244
σᵢ = 该标的近 30 日收益标准差
ρᵢⱼ = 0.35（跨行业） / 0.62（同行业）
```

- ρ 是**写死的假设值**，界面上已标注；接入真实协方差矩阵后应改为估计值并保留假设值回退。
- 该假设只用于「调整前后谁更大」的相对比较，不要在文案里当作绝对风险预测。

## 9. 市场时钟

以真实系统时间判断，不含节假日表（`data.js:325`）：

| 时段 | 状态 | 是否 live |
|---|---|---|
| 周一–周五 09:15 前 | 盘前 | 否 |
| 09:15–09:30 | 集合竞价 | 是 |
| 09:30–11:30 | 交易中 · 早盘 | 是 |
| 11:30–13:00 | 午间休市 | 否 |
| 13:00–15:00 | 交易中 · 午盘 | 是 |
| 15:00–15:30 | 盘后定价交易 | 否 |
| 15:30 后 | 已收盘 | 否 |
| 周六 / 周日 | 休市 + 下一交易日倒计时 | 否 |

⚠️ 未排除法定节假日与调休。接真实数据时必须引入交易日历，且日历要能解释「今天休市、下一交易日是 X 月 X 日」。日历源已验证：`ak.tool_trade_date_hist_sina()`（1990 年起全量交易日，见 `04` 附录）。

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
| `perf.json` | `perf` | §7 | `nav[],bench[]` → `cum,bench_cum,ann_vol,mdd,bench_mdd,sharpe,win_rate` |
| `vol_synth.json` | `vol_synth` | §8 | `items[{code,sector,w,sigma}]` → `ann_vol`（百分数） |
| `price_limit.json` | `limit_pct` / `limit_prices` / `validate_price` | §2 | `kind,code,st` → 涨跌幅比例（ETF/北交所 null）/ `kind,prev,pct` → 涨停与跌停价（整数单元 half-up）/ `price,limit_up,limit_down` → `{valid,reason}` |

### 本附录固定的口径（新增约定，与正文同级）

1. **`sell_to` 按 lot 泛化**：原型公式写死 100 股；泛化为 `max(lot, ceil((mv − totalAssets×target/100) / price / lot) × lot)`，转债按 10 张取整。§6 公式已同步更新。
2. **`perf` 短序列**：长度 < 2（无相邻收益）→ 全指标返回 0，不报错。
3. **清仓**：`after_shares = 0` 时持仓行移除，`after_cost` 记 `null`。
4. **纪律压线**：恰好等于阈值算安全（见比对规则第 5 条）。
5. **费用输出不舍入**：§3 公式结果保留原始精度（估算口径）；分位舍入是展示层与券商结算的事，领域库不做。

### 已知缝隙（已记 `07-backlog.md`，fixture 暂不校验对应字段）

- **B11**：基准侧风险指标口径未定——`benchVol`（RMS vs 标准差两处实现不一致）、`sharpe` ε 守卫缺失（diagnosis 页）、beta / 相关系数 / 超额收益未收入 §7。`perf.json` 因此只校验上表列出的 7 个字段。
- **B12**：奇零股卖出——§1「可卖整数股」与原型「买卖一律整手拦截」冲突，`lot.json` 暂按原型行为编码（case `vq-sell-odd-lot`）。
