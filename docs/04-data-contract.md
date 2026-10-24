# 04 · 数据契约

接真实数据时**只改这一层**，页面逻辑不动——这是原型立下的最重要边界，也是后端 API 的形状定义来源。

## 挂载点

| 全局 | 来源 | 职责 |
|---|---|---|
| `window.QW` | `design/assets/data.js` | 数据、派生指标、格式化工具、外壳渲染、市场时钟 |
| `window.QWC` | `design/assets/charts.js` | 纯 SVG 图表原语，不含业务数据 |

页面通过 `<script src="assets/data.js">` + `assets/charts.js` 取用，顺序不能颠倒（图表层无数据依赖，数据层的 `chrome()` 会读 DOM）。

## `QW` 契约全表

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

### 原始数据（真实数据要替换的就是这些）

| 字段 | 形状 | 必填 | 备注 |
|---|---|---|---|
| `ws` | `{ name, init, style, cash, broker, wechat }` | ✅ | 单账户；`cash` 为可用现金 |
| `indices[]` | `{ name, code, v, chg }` | ✅ | 行情条 5 个指数 |
| `holdings[]` | `{ code, name, mkt, kind, sector, shares, cost, price, prev }` | ✅ | `mkt: 'SH'|'SZ'`；`kind: 'stock'|'etf'|'cb'` |
| `watchlist[]` | `{ code, name, sector, price, dayChg, note }` | ✅ | 未持有的自选 |
| `advice{}` | `code → { score, action, horizon, conf, position, target, stop, reason, dims[4], risks[] }` | ✅ | 语义见 `03` 第 10 节 |
| `alerts[]` | `{ time, code, name, type, level, text, done }` | ✅ | `level: up/down/warn/flat`；`done` 为归档态 |
| `qa[]` | `{ q, a:[{t,k}], cites:[{src,txt}], chips:[] }` | ✅ | `k: conclusion/body/plan` |
| `sessions[]` | `{ t, who, text, reply, read }` | ✅ | 微信/定时任务历史 |
| `equity` | `{ dates[], nav[], bench[] }` | ✅ | 三者等长；`nav`/`bench` 归一到同一起点 |

### 派生字段（**由代码计算，永远不要手填**）

| 字段 | 来源 | 口径 |
|---|---|---|
| `holdings[].dayChg / dayVal / mv / pnl / pnlPct / spark / bars` | `data.js:69` | 见 `03` 第 5 节；`spark`/`bars` 是演示用的确定性随机游走 |
| `cash` | `ws.cash` | 直接透传 |
| `mvStocks / mvEtf / mvCb / totalMv` | `data.js:80` | 分类市值 |
| `totalAssets` | `data.js:84` | `totalMv + cash` |
| `dayPnl / dayPnlPct` | `data.js:85` | 当日盈亏；百分比分母为昨日总资产 |
| `totalPnl / totalPnlPct / costTotal` | `data.js:86` | 累计盈亏与投入成本 |
| `exposure` | `data.js:90` | 仓位 = 市值 / 总资产 |
| `sectors[]` | `data.js:95` | `{ name, v, w }`，按市值降序 |
| `hhi` / `top3Sector` | `data.js:98` | 集中度 |
| `P` | `data.js:157` | `{ cum, benchCum, annVol, benchVol, mdd, benchMdd, sharpe, winRate }` |
| `wp(code)` / `wsec(name)` | `data.js:103` | 占总资产 / 占市值（**两个不同分母**） |
| `sellTo(code, target)` / `weightAfter(code, qty)` | `data.js:107` | 减仓测算 |

### 行为函数

| 字段 | 说明 |
|---|---|
| `marketClock(now?)` | → `{ state, dot, next, time, live }`，时段规则见 `03` 第 9 节 |
| `chrome({ active, crumb })` | 注入侧栏 / 顶栏 / 行情条 / 移动底栏 / toast 容器，并绑定快捷键 |
| `stripHTML()` | 行情条 HTML |
| `toast(msg, kind)` | 轻提示（`kind: 'check'` 用勾图标） |
| `tickPrices(manual)` | 演示用行情跳动 |

## `QWC` 契约

| 函数 | 用途 | 关键参数 |
|---|---|---|
| `line(host, cfg)` | 折线/面积图，带十字准星与 resize 重绘 | `cfg.series[{data,color,dash,width,area,endDot}]`、`labels`、`tip(i)`、`yFmt`、`height`、`pad` |
| `spark(data, dir, w, h)` | 表格内联迷你走势线（返回 HTML 字符串） | `dir` 决定红/绿 |
| `bars(data, dir, h)` | 量能柱（返回 HTML 字符串） | |
| `donut(host, items, opts)` | 环形权重图 | `items[{name,v,color}]`，默认 8 色 `DONUT_COLORS` |
| `gauge(host, score, label)` | 得分弧 | ≥70 红、55–69 accent、<55 绿 |
| `hbars(items, opts)` | 水平条组（返回 HTML） | `items[{k,v,color}]`、`fmt`、`id` |
| `smooth/path/scale/esc/color` | 底层工具，页面自定义图形（如 K 线）复用 | `color(varName)` 读 CSS 令牌 |

**图表颜色一律从 CSS 变量读取**（`css('--accent')` 等），所以换主题只改 `ui.css` 的 `:root`，图表自动跟着变。`DONUT_COLORS` 是唯一硬编码在 JS 里的一组色值（`charts.js:120`）——改配色时别漏。

`analysis.html` 的 K 线是页面自己画的（复用 `C.smooth/C.scale/C.color`），没有下沉到 `charts.js`；如果第二处也要用 K 线，就该抽进图表层了。

## 接真实数据时的适配器要求

1. **同名同形状**：真实源返回的数据必须组装成上表的 `原始数据` 结构再交给页面，页面不感知数据源。
2. **派生不能手填**：适配器只产出 `price / prev / shares / cost` 等原始字段，派生字段由契约层重算（这是 `03` 全部公式的执行处）。
3. **行情更新要触发重算** ⚠️ 当前缺口：`tickPrices()` 只更新 `h.price` 与 `h.dayChg`，不更新 `dayVal / mv / pnl / weight`，也不更新 `totalAssets / dayPnl / sectors / hhi` 这些**加载时算一次的常量快照**（各页定时器重算范围不同，现象见 `07-backlog.md` B1）。契约层必须提供 `recalc()`（或把汇总改为函数），并让所有更新入口——手动刷新、自动跳动、真实源推送——都走它。真实数据下这条从「演示瑕疵」升级为「功能正确性」问题。
4. **时间要显式**：`equity.dates` 目前是「从今天往回推交易日」的中文短标签（`data.js:125`），且起点写死为 `2026-10-24`。真实数据要用带时区的日期 + 显示层格式化，`marketClock` 还要接交易日历（节假日/调休）。
5. **代码 → 交易所的推断不要靠首字母**：页面里有 `code[0] === '6' ? 'SH' : 'SZ'`（`analysis.html:281`）这类猜测；契约里 `mkt` 是显式字段，请用它。
6. **建议要可留档**：`advice` 现在只有一个当前快照，而 `analysis.html` 的历史建议表格用的是页面内样本。真实实现里建议必须带生成时间与输入数据版本，否则「回看当时怎么说的」这个卖点无法兑现。
7. **禁止跨页补全局** ⚠️ 现存缺陷：`D.totalMvAll()` 定义在 `holdings.html:196`、`h.weight` 也在该页赋值（`:192`），却被 `analysis.html:369` 调用。两个页面是独立加载的，因此在分析页看「减仓/卖出」标的会直接抛 `TypeError`。**共享派生必须放进 `data.js`。**
8. **不要写死演示用的锚定值**：`walk()` 的末值锚定、`equity` 的 90 日长度、`advice` 文案里被正则覆盖的「当前 X%」（`data.js:232`）都属于样本阶段产物。第 7 条的正则打补丁方式很脆——接入真实数据后应当由模板插值，而不是对写死的中文句子做正则替换。

## 浏览器本地状态

| 键 | 内容 | 出处 |
|---|---|---|
| `qingwu:chat:me` | 问答会话，最多 30 条 | `chat.html` |
| `qingwu:wechat:v2` | 通道开关、订阅、静音标的、绑定状态、推送参数 | `wechat.html` |

约定：键名统一 `qingwu:` 前缀 + `:vN` 版本后缀，结构变更时升版本而不是迁移旧数据。⚠️ `wx-config-card` 允许在浏览器里填 AppID / Token / Webhook —— 这条通道将来接服务端后，密钥必须移到服务端配置，**永远不要继续存在 localStorage**。
