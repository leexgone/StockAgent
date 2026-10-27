# 04 · 数据契约

接真实数据时**只改这一层**，页面逻辑不动——这是原型立下的最重要边界，也是后端 API 的形状定义来源。

## 挂载点

| 全局 | 来源 | 职责 |
|---|---|---|
| `window.ZM` | `design/assets/data.js` | 数据、派生指标、格式化工具、外壳渲染、市场时钟 |
| `window.ZMC` | `design/assets/charts.js` | 纯 SVG 图表原语，不含业务数据 |

页面通过 `<script src="assets/data.js">` + `assets/charts.js` 取用，顺序不能颠倒（图表层无数据依赖，数据层的 `chrome()` 会读 DOM）。

## `ZM` 契约全表

### 工具

| 字段 | 签名 | 说明 |
|---|---|---|
| `fmt` | `(v, d=2) → string` | zh-CN 千分位，固定小数位 |
| `money` | `(v) → string` | `¥` + 两位小数 |
| `pct` | `(v) → string` | 带符号百分比，两位小数 |
| `wan` | `(v) → string` | 万元显示 |
| `dirClass` | `(v) → 'up'|'down'|'flat'` | 涨跌类名（红涨绿跌） |
| `ICONS` | `{ name: svgString }` | 内联图标，24×24 viewBox |
| `kindLabel` | `{ stock, etf, cb }` | 类型中文名 |
| `priceDec` | `(kind) → 2\|3` | 报价小数位：股票 2、ETF/转债 3（与领域库 `price_decimals` 同源，见 `03` §1） |
| `limitPrices` | `(kind, prev, pct) → { lo, hi }\|null` | 涨跌停价：昨收 × (1 ± pct) 按 `priceDec` 整数单元四舍五入（half-up，`03` §2）；`prev ≤ 0` 或 `pct` 未配置 → `null` |

### 原始数据（真实数据要替换的就是这些）

| 字段 | 形状 | 必填 | 备注 |
|---|---|---|---|
| `ws` | `{ name, init, style, cash, broker, wechat }` | ✅ | 单账户；`cash` 为可用现金 |
| `indices[]` | `{ name, code, v, chg }` | ✅ | 行情条 5 个指数 |
| `holdings[]` | `{ code, name, mkt, kind, sector, shares, cost, price, prev, limitPct }` | ✅ | `mkt: 'SH'|'SZ'`；`kind: 'stock'|'etf'|'cb'`；`limitPct` 涨跌停比例是标的属性（ETF 无法由代码推断，P4 由行情源填充，见 `03` §2） |
| `watchlist[]` | `{ code, name, mkt, kind, limitPct, sector, price, dayChg, note }` | ✅ | 未持有的自选；与 `holdings` 一样带 `mkt / kind / limitPct`，页面不得由代码推断 |
| `advice{}` | `code → { score, action, horizon, conf, position, target, stop, reason, dims[4], risks[] }` | ✅ | 语义见 `03` 第 10 节 |
| `alerts[]` | `{ time, code, name, type, level, text, done }` | ✅ | `level: up/down/warn/flat`；`done` 为归档态 |
| `qa[]` | `{ q, a:[{t,k}], cites:[{src,txt}], chips:[] }` | ✅ | `k: conclusion/body/plan` |
| `sessions[]` | `{ t, who, text, reply, read }` | ✅ | 微信/定时任务历史 |
| `equity` | `{ dates[], nav[], bench[] }` | ✅ | 三者等长；`nav`/`bench` 归一到同一起点 |

### 派生字段（**由代码计算，永远不要手填**）

汇总字段是**活快照**而非加载时常量：每次 `recalc()`（`data.js:82`）算完由 `publish()`（`data.js:80`）写回导出对象，页面渲染时读 `D.totalAssets` 等即最新值。

| 字段 | 来源 | 口径 |
|---|---|---|
| `holdings[].dayChg / dayVal / mv / pnl / pnlPct` | `recalc()`，`data.js:84` | 见 `03` 第 5 节 |
| `holdings[].weight` | `recalc()`，`data.js:109` | `mv / totalAssets × 100`，与 `wp()` 同分母，保留数值供排序与纪律校验 |
| `holdings[].spark / bars` | `recalc()`，`data.js:90` | 演示用确定性随机游走，只生成一次（价格跳动不重生成） |
| `cash` | `ws.cash` | 直接透传；改现金只改 `ws.cash`（交易页提交处），`recalc()` 后同步为 `ZM.cash` |
| `mvStocks / mvEtf / mvCb / totalMv` | `data.js:95` | 分类市值 |
| `totalAssets` | `data.js:99` | `totalMv + cash` |
| `dayPnl / dayPnlPct` | `data.js:100` | 当日盈亏；百分比分母为昨日总资产 |
| `totalPnl / totalPnlPct / costTotal` | `data.js:101` | 累计盈亏与投入成本 |
| `exposure` | `data.js:106` | 仓位 = 市值 / 总资产 |
| `sectors[]` | `data.js:114` | `{ name, v, w }`，按市值降序 |
| `hhi` / `top3Sector` | `data.js:117` | 集中度 |
| `P` | `data.js:183` | `{ cum, benchCum, annVol, benchVol, mdd, benchMdd, sharpe, winRate }`；净值曲线是静态历史，不随盘中跳动变化 |
| `wp(code)` / `wsec(name)` | `data.js:129` | 占总资产 / 占市值（**两个不同分母**） |
| `sellTo(code, target)` / `weightAfter(code, qty)` | `data.js:133` | 减仓测算 |

### 行为函数

| 字段 | 说明 |
|---|---|
| `recalc()` | **唯一重算入口**：逐笔派生 + 汇总快照，算完写回导出对象（`data.js:82`）。价格、持仓、现金任何变动后必须调用；空持仓时各比率取 0，不产生 NaN |
| `totalMvAll()` | 实时 Σ `price × shares`，不经快照，任何时刻调用都一致（`data.js:125`） |
| `marketClock(now?)` | → `{ state, dot, next, time, live }`，时段规则见 `03` 第 9 节 |
| `chrome({ active, crumb, onRefresh? })` | 注入侧栏 / 顶栏 / 行情条 / 移动底栏 / toast 容器，绑定快捷键；`onRefresh` 在顶栏刷新按钮完成跳动与重算后被调用，页面用它重绘自己的区块 |
| `stripHTML()` | 行情条 HTML（读实时快照，`data.js:431`） |
| `toast(msg, kind)` | 轻提示（`kind: 'check'` 用勾图标） |
| `tickPrices(manual)` | 演示用行情跳动；内部调用 `recalc()` 并重绘行情条，**返回价格是否真的变动**（非手动且非交易时段返回 `false` 并跳过，页面据此决定是否重绘）（`data.js:492`） |

## `ZMC` 契约

| 函数 | 用途 | 关键参数 |
|---|---|---|
| `line(host, cfg)` | 折线/面积图，带十字准星与 resize 重绘 | `cfg.series[{data,color,dash,width,area,endDot}]`、`labels`、`tip(i)`、`yFmt`、`height`、`pad` |
| `spark(data, dir, w, h)` | 表格内联迷你走势线（返回 HTML 字符串） | `dir` 决定红/绿 |
| `bars(data, dir, h)` | 量能柱（返回 HTML 字符串） | |
| `donut(host, items, opts)` | 环形权重图 | `items[{name,v,color}]`，默认 8 色 `DONUT_COLORS` |
| `gauge(host, score, label)` | 得分弧 | 质量红绿灯（P1-T4）：≥70 `--ok` 绿、55–69 `--warn` 黄、<55 `--danger` 红 |
| `hbars(items, opts)` | 水平条组（返回 HTML） | `items[{k,v,color}]`、`fmt`、`id` |
| `smooth/path/scale/esc/color` | 底层工具，页面自定义图形（如 K 线）复用 | `color(varName)` 读 CSS 令牌 |

**图表颜色一律从 CSS 变量读取**（`css('--accent')` 等），所以换主题只改 `ui.css` 的 `:root`，图表自动跟着变。`DONUT_COLORS` 是唯一硬编码在 JS 里的一组色值（`charts.js:120`）——改配色时别漏。

`analysis.html` 的 K 线是页面自己画的（复用 `C.smooth/C.scale/C.color`），没有下沉到 `charts.js`；如果第二处也要用 K 线，就该抽进图表层了。

## 接真实数据时的适配器要求

1. **同名同形状**：真实源返回的数据必须组装成上表的 `原始数据` 结构再交给页面，页面不感知数据源。
2. **派生不能手填**：适配器只产出 `price / prev / shares / cost` 等原始字段，派生字段由契约层重算（这是 `03` 全部公式的执行处）。
3. **行情更新要触发重算**：契约层已提供 `recalc()`——自动跳动、手动刷新（`tickPrices()` 内部）与交易页提交都统一走它，重算逐笔派生与汇总快照并重绘行情条；页面通过 `chrome({ onRefresh })` 或定时器回调重绘自身（原 B1 缺口已于 2026-10-24 修复）。真实数据推送只需写入 `price / prev` 后调用 `recalc()`，页面零改动。
4. **时间要显式**：`equity.dates` 目前是「从今天往回推交易日」的中文短标签（`data.js:154`），且起点写死为 `2026-10-24`。真实数据要用带时区的日期 + 显示层格式化，`marketClock` 还要接交易日历（节假日/调休）。
5. **代码 → 交易所的推断不要靠首字母**：契约里 `mkt` 是显式字段（`holdings` 与 `watchlist` 都带），直接用它。原 `code[0] === '6' ? 'SH' : 'SZ'` 猜测已于 P1-T5 删除（B7，2026-10-28）——该猜测对 `5` 开头的深市 ETF 会判错。
6. **建议要可留档**：`advice` 现在只有一个当前快照，而 `analysis.html` 的历史建议表格用的是页面内样本。真实实现里建议必须带生成时间与输入数据版本，否则「回看当时怎么说的」这个卖点无法兑现。原型样本已按此对齐字段（P1-T7：每行带 `generatedAt / dataVersion`，演示期恒为 `demo-sample-v0`，卡片显式标注「演示样本 · 非真实留档」）；真实留档归 P4-T2（`advice_history` 表），届时换数据源并删标注。
7. **禁止跨页补全局**：共享派生只能放在 `data.js`（A1 的教训：`weight` / `totalMvAll()` 曾定义在 `holdings.html` 页内，被 `analysis.html` 调用时直接抛 `TypeError`；已于 2026-10-24 收进契约层）。页面不得给 `ZM` 赋值，也不得定义供其它页面使用的派生函数。
8. **不要写死演示用的锚定值**：`walk()` 的末值锚定、`equity` 的 90 日长度、`advice` 文案里被正则覆盖的「当前 X%」（`data.js:259`）都属于样本阶段产物。这种正则打补丁方式很脆——接入真实数据后应当由模板插值，而不是对写死的中文句子做正则替换。

## 浏览器本地状态

| 键 | 内容 | 出处 |
|---|---|---|
| `zimu:chat:me` | 问答会话，最多 30 条 | `chat.html` |
| `zimu:wechat:v2` | 通道开关、订阅、静音标的、绑定状态、推送参数 | `wechat.html` |

约定：键名统一 `zimu:` 前缀 + `:vN` 版本后缀，结构变更时升版本而不是迁移旧数据。⚠️ `wx-config-card` 允许在浏览器里填 AppID / Token / Webhook —— 这条通道将来接服务端后，密钥必须移到服务端配置，**永远不要继续存在 localStorage**。

## 附录 · 行情源实测与字段对照（P2-T1 spike，2026-10-26）

环境：akshare 1.19.3 / pandas 3.1.0 / Python 3.13.1，周一盘后实测。复现：`cd datafetch && uv run spike_probe.py`（三轮，脚本带接口候选名自动发现，失败本身也是结论）。

### 通道结论

| 用途 | 首选 | 备选 | 实测 |
|---|---|---|---|
| 实时行情（股票/ETF/指数/转债，一次多标的） | 新浪 `hq.sinajs.cn` 直连（需 Referer 头） | 东财 `push2 ulist` 直连（**必须带浏览器 UA**） | ✅ 两者均通 |
| 全市场扫描（低频） | akshare `stock_zh_a_spot`（新浪，全量翻页 ~20s） | 东财 push2 分页 | ✅ |
| 股票日线 | 新浪 `stock_zh_a_daily`（qfq，全历史，含换手率/流通股本） | 东财 `stock_zh_a_hist`（多振幅/换手率列） | ✅ 新浪冒烟稳定（沪/深/创业板均通）；东财间歇被拦 |
| ETF 日线 | akshare `fund_etf_hist_sina`（全历史） | — | ✅ |
| 指数日线 | akshare `stock_zh_index_daily`（新浪，全历史） | — | ✅ |
| 转债日线 | akshare `bond_zh_hs_cov_daily`（新浪，全历史） | — | ✅ |
| 转债实时 | akshare `bond_zh_hs_cov_spot`（新浪，326 只在市） | 新浪 hq 直连 | ✅ |
| 转债强赎/到期预警 | akshare `bond_cb_redeem_jsl`（集思录） | — | ✅ 字段全 |
| 交易日历 | akshare `tool_trade_date_hist_sina`（1990 起 8797 天） | — | ✅ |
| 代码-名称表 | akshare `stock_info_a_code_name`（~17s 分页） | — | ✅ |
| 个股→行业（`sector`） | 新浪 `stock_sector_spot`（49 板块）+ `stock_sector_detail(label)` 成分股 → 建 code→sector 映射，周更存 SQLite | 东财个股信息（被拦，不用） | ✅ 已验证；成分股附带 PE/PB/市值/换手率，可作将来基本面原料 |
| baostock 兜底（日线） | — | — | ❌ 登录失败（错误码 10002007「网络接收错误」）；兜底次序改为 **新浪系 → 东财直连**，baostock 是否保留 P4 复测再定 |

### 关键风险（P4-T3 生产化必须处理）

1. **东财对 akshare 封装做指纹反爬**：spike 期间 akshare 的东财实时系接口（`stock_zh_a_spot_em` / `stock_bid_ask_em` / `stock_individual_info_em` / `fund_etf_spot_em` / `stock_zh_index_spot_em` / ETF与指数日线 em）全部 `RemoteDisconnected`，连打 7 次均拒；而带浏览器 UA 直连 `push2.eastmoney.com` 返回 HTTP 200。**sidecar 的东财通道必须直连 + UA + 重试熔断，不得依赖 akshare 的东财封装。**且带 UA 的直连同样间歇（同一端点第二轮 200、第三轮被拒），熔断要快速触发并切新浪。
2. **雪球源已死**：`stock_individual_spot_xq` 要求 `xq_a_token`（400016），不要规划进通道。
3. **同花顺 ETF 表不适用**：`fund_etf_spot_ths` 是场外净值口径，场内实时价用新浪。
4. **退市/停牌标的返回静默脏数据**：新浪 hq 对已退市转债返回 `open=0、volume=0、price=昨收定格`（110074），对已摘牌标的返回**空串**（127022）。适配器必须输出 `status` 字段区分（`ok / halted_or_delisted / empty_or_delisted`，见 `datafetch/app.py`），不得当正常行情喂给契约层。演示样本受此影响，见 `07-backlog.md` B10。
5. **限频**：东财被拦期间快速失败（~0.4s），熔断可快速触发；新浪 hq 与新浪日线接口在三轮 spike 中多次调用无限频迹象。全市场快照类（20s+/次）只用于低频扫描，禁止进分钟级轮询。

### 实时行情字段 → 契约对照（新浪 hq，逗号分隔位序）

| 位 | 含义 | 契约字段 |
|---|---|---|
| f0 | 名称 | `name` |
| f1 | 今开 | —（可存，用于日内） |
| f2 | 昨收 | `prev` |
| f3 | 现价 | `price` |
| f4 / f5 | 最高 / 最低 | —（预警判定用） |
| f8 | 成交量（股） | —（`bars`/量比来源） |
| f9 | 成交额（元） | —（放量预警来源） |
| f30 / f31 | 日期 / 时间 | 数据时点（适配器要求第 4 条「时间要显式」） |
| 派生 | (f3−f2)/f2×100 | `dayChg` |

`indices[]` 同布局（`sh000001` 等）：`v`←f3、`chg`←派生。东财备份通道字段为 ×100 缩放整数（`f2/100=price`、`f3/100=涨跌幅%`、`f18/100=prev`），指数缩放待第三轮确认。

### 日线字段 → 契约对照

| 源 | 列 | 映射 |
|---|---|---|
| 东财股票日线 | 日期/开盘/收盘/最高/最低/成交量/成交额/振幅/涨跌幅/涨跌额/换手率 | → `date/open/close/high/low/volume`；换手率可作量比原料 |
| 新浪 ETF/指数/转债日线 | date,open,high,low,close,volume(,amount) | 同名直映 |
| `equity.bench` | 指数日线 close 归一到与 `nav` 同起点 | 见 `03` 第 7 节 |

### 转债特有字段（集思录强赎表 → alerts）

`代码 / 名称 / 现价 / 最后交易日 / 到期日 / 转股价 / 强赎触发价 / 正股价 / 强赎天计数 / 强赎状态`（值如「已公告强赎」「临近到期」）——正文 `alerts` 里「强赎公告 / 最后交易日 11-20」这类样本的真实数据源；`bond_zh_cov`（比价表）偏发行期字段，正股实时溢价率需自算：`正股价/转股价×100` 与债现价推转股溢价率。

### spike API 冒烟结果（`datafetch/app.py`，进程内 TestClient）

- `/quote`（7 标的：沪股/深转债/沪转债/ETF/指数/退市转债/摘牌转债）全部归一返回；**状态三态识别在真实退市样本上验证通过**——存活 `ok`、退市定格 `halted_or_delisted`（110074，价停 222.81 零成交）、摘牌 `empty_or_delisted`（127022）。
- `/kline` 四类（股票 600519/000001/300750、ETF 510300、指数 000001、转债 110076）全部 200，最新 bar 均为 2026-10-26 当日。
- `/calendar` 返回含**未来**交易日（至 2026-12-31），`marketClock` 的「下一交易日」计算可直接用。

### 盘中频率结论（P4-T7 预警轮询参数依据）

- 新浪 hq 直连：单请求多标的（5 标的 ~0.3s），**持仓+自选合并成一次请求、每分钟一轮**完全可行。
- 分钟级轮询禁止使用全市场快照类接口。
- 东财备份通道被拦时快速失败，适合做熔断切换而非重试等待。
