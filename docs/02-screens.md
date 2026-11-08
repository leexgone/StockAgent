# 02 · 界面清单与模块职责

七个页面，共享同一套外壳（侧栏 + 顶栏 + 行情条 + 移动端底部标签栏），外壳由 `ZM.chrome({ active, crumb, onRefresh? })` 注入（`design/assets/data.js:380`）。`onRefresh` 是页面重绘回调：顶栏刷新完成跳动与重算后被调用，各页面用它把读汇总快照的区块重画一遍（见 `04` 行为函数表）。

## 全局导航与快捷键

| 入口 | 行为 | 实现 |
|---|---|---|
| 侧栏 1–5 + 微信 | 五个主界面 + 接入设置 | `NAV`（`data.js:372`） |
| 数字键 `1`–`6` | 直接切页（输入框内不拦截） | `bindChrome()`（`data.js:481`） |
| `⌘K` / `Ctrl+K` | 聚焦全局搜索 | `data.js:482` |
| 全局搜索回车 | 命中持仓/自选 → `analysis.html?code=`；否则 → `chat.html?q=` | `data.js:474` |
| 顶栏刷新按钮 | 手动跳一次行情 + 全量重算 + 重绘行情条，再回调页面 `onRefresh` | `data.js:461` |
| 顶栏铃铛 | toast 汇总待处理预警条数与最近一条 | `data.js:468` |
| 行情条 | 5 个指数 + 我的总资产 / 今日盈亏（读实时快照，每次跳动后重绘） | `stripHTML()`（`data.js:431`） |
| 市场时钟 | 每秒重绘时段与倒计时 | `marketClock()`（`data.js:325`） |

每屏底部固定一个免责区块（`*-disclaimer`），新增界面不能省。

---

## 空持仓态（P1-T1，全屏幕通用）

真实用户第一次打开时持仓为空，7 屏必须不报错并给出引导路径（原型走查：任意页 `?empty=1`，`?empty=0` 退出，机制与存档见 `07` A2 / `04` 本地状态）：

- **index**：问候语与快照如实显示「总资产 = 现金」，动作按钮、待决事项与时间线换成引导（录入第一笔 → 问答求助 → 切回演示参照）。
- **dashboard**：净值图、归因、预警、持仓表、收盘摘要各显示对应引导，不出伪 0 值。
- **holdings**：**「录入第一笔」入口页**——表内引导 + 主按钮直接打开录入表单抽屉（ADR-0006 手工逐笔先行，校验口径见 `03`）；流水与纪律卡显示待启动态。抽屉候选 = 持仓 + 自选未持有 + **「＋ 新标的（手工输入）」兜底**（代码/市场/名称/类型/行业/涨跌停 %，字段口径同自选表单；B15：空白账户候选池为空时主按钮与 `?add=1` 深链也必须开抽屉，不得静默失败），无行情参考价则价格手填、跳过涨跌停拦截（后端仍是最终裁判）；现金为 0 时买入试算就地提示先经「可用现金 · 调整」设入资金。
- **analysis**：持仓范围为空时隐藏结果、显示引导卡；可切「自选」看观察类结论形态。
- **chat**：空态专属欢迎消息与「第一笔怎么录入 / 今天该做什么」问答；会话历史按模式分键。
- **diagnosis**：指标不以 0 冒充「安全」，KPI/曲线/清单/分区间表全部为说明态。
- **wechat**：卡片模板保留结构、文案占位、按钮禁用；推送日志为空态。

## index.html — 工作台

进入任何具体功能之前的「今天怎么了」层。

| 模块（`data-od-id`） | 内容 | 数据依赖 |
|---|---|---|
| 问候区 | 按时段变化的称呼 + 日期 + 市场状态 + 一句话摘要 | `marketClock()`、`holdings`、`advice` |
| `home-snapshot` | 总资产（含现金）、当日盈亏、累计盈亏、待处理预警数 | `totalAssets / dayPnl / totalPnl / alerts` |
| `home-screens` | 六张界面入口卡，各带一行实时统计 | 全量 |
| `home-timeline` | 助理最近的动作（微信问答、定时复盘） | `sessions` |
| `home-attention` | 今天需要你决定的 3 件事，按紧迫度排序 | `advice`、`holdings` |

摘要句里的数字必须来自计算（`renderHead()`，`index.html:100`），这是「不写死数字」约定的样板位置；顶栏刷新会连同指标卡一起重绘。

---

## dashboard.html — 总览仪表盘

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `equity-card` | 净值 vs 沪深 300 折线，十字准星 tooltip，近一周/一月/一季切换 | `equity.dates/nav/bench`、`ZMC.line` |
| `allocation-card` | 钱在哪里：股票 / ETF / 转债 / 现金拆分 | `mvStocks / mvEtf / mvCb / cash` |
| `attribution-card` | 当日盈亏归因：谁在拖、谁在拉 | `holdings[].dayVal` |
| `alerts-card` | 待处理预警，可逐条归档（归档后不再出现在首屏） | `alerts[].done` |
| `dash-holdings-card` | 持仓表 + 迷你走势线，点行跳到该标的的分析页 | `holdings[].spark`、`ZMC.spark` |
| `watchlist-card` | 自选观察（未买入，仅条件触发时提醒） | `watchlist` |
| `daily-brief` | 收盘摘要，15:05 自动生成并推微信 | `sessions`、派生指标 |

## holdings.html — 持仓管理

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `holdings-summary` | 市值、当日浮动、累计盈亏、可用现金、盈利标的数 | `recalc()` 快照统一重算（见 04） |
| `holdings-table-card` | 全部持仓，排序 / 类型筛选 / 搜索，行内 sparkline 与仓位条 | `holdings` |
| `trade-drawer` | **加减仓抽屉**：方向分段、数量与价格输入、快捷档位、整手与涨跌停校验、费用与摊薄成本预览、调整后仓位与超上限提示、「我已确认」勾选后才可提交 | `lots`、`fee()`（`holdings.html:320`） |
| `order-log-card` | 委托 / 成交流水，提交试算后追加一条 | 本地 `LOG` 数组 |
| `rule-card` | 仓位纪律四条进度与「已触及 / 安全」 | 见 03 |
| `holding-form-drawer`（生产侧，ADR-0009） | **直接维护表单**：新建 / 编辑持仓行（名称/市场/类型/行业/数量/成本/涨跌停 %/备注）；无试算预览——校正没有试算语义；允许奇零股、跳过 lot 与涨跌停校验（03 §13） | `PUT /api/holdings/{code}` |
| `holding-edits-card`（生产侧，ADR-0009） | **维护记录**：最近直接维护留痕（时间/操作/标的前后值/备注），只读；现金不联动、流水不生成的口径见 03 §13 | `GET /api/holding_edits` |

抽屉是本产品的核心交互：任何后续实现都必须保留「试算 ≠ 下单」这一层。生产侧（`frontend/`）自 ADR-0009 起三入口分工：加减仓抽屉 = 事件流（费用/已实现/现金联动）、**直接维护 = 快照校正**（持仓表操作列「编辑 / 删除」+ 页头「直接录入」，删除行内两步确认、流水保留如实提示）、现金「调整」= 现金侧校正。

## analysis.html — AI 分析建议

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `analysis-picker` | 标的选择（持仓 + 自选，自选走 `watchOnly` 分支） | `holdings`、`watchlist` |
| `analysis-rank` | 按综合得分排序的建议列表，点行切换 | `advice[].score` |
| `verdict-card` | 结论标签（加仓/买入/持有/减仓/卖出/定投）、得分弧、置信度、目标与止损、现价与当日涨跌 | `advice[code]` |
| `kline-card` | 30 日 OHLC 蜡烛 + MA5/MA20，日K/周K 切换，末根锚定现价 | `ZM` 派生 + 本页 `candle()` |
| `dims-card` | 技术 / 基本面 / 资金 / 消息四维分数与观察 | `advice[].dims[4]` |
| `plan-card` | 分笔执行计划（减仓三步、买入两笔 + 失效条件） | `buildPlan()`（`analysis.html:359`） |
| `risk-card` | 「这个结论可能错在哪」 | `advice[].risks[]` |
| `history-card` | 该标的的历史建议与当时结论 | 本页样本数组 |

支持 `?code=` 直达；「送到持仓页试算」是分析页 → 持仓页的闭环出口。

## chat.html — 智能问答

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `chat-thread-card` | 对话流：打字指示 → 分段呈现，回答固定「结论 / 为什么 / 怎么做」三层 | `qa[]`（`k: conclusion / body / plan`） |
| `chat-context-card` | 「助理现在能看到什么」：账户、总资产、持仓数、现金、时段 | `ws`、汇总快照（`renderCtx()`，顶栏刷新时重绘） |
| `chat-limit-card` | 「它不会做的事」：能力边界自陈 | 静态文案 + 免责 |
| `chat-sessions-card` | 最近会话，含微信里的语音与文字 | `sessions`（生产侧读 `/api/push_logs`；通道独立：QQ/微信会话不与网页合流——ADR-0011 拍板①） |
| 依据来源 | 每条回答下方列出引用的界面与原文（持仓明细 / 组合诊断 / 行情快照 / 公告） | `qa[].cites` |
| 追问 chips | 三个后续问题快捷入口 | `qa[].chips` |

会话真值在服务端档案 `chat_messages`（ADR-0011，2026-11-07 T3/T4）：进入 chat 屏即
`GET /api/chat/history` 水合；「清空会话」走 `POST /api/chat/reset`（服务端档案 +
Agent 内存历史一并清，服务端失败不清本地）。`localStorage['zimu:chat:me']`
**降级为服务端不可达时的只读展示缓存**（上限 30 条，thinking 段两处都不入）。

生产侧深链契约（frontend ChatView/ChatPanel）：`/chat?q=<问题>` 只**预填输入不自动发送**
（真实 LLM，交用户确认——全局搜索回车、analysis「就这只问我」走此形态）；
`/chat?q=<问题>&send=1` 在 agent 上线且空闲时**自动发送一次**（仅限意图明确的斜杠指令
深链——analysis 空态「去问答触发 /建议」，`q=/建议 <code>`，代码写法按 generate-advice
技能约定：持仓内裸代码、非持仓带 sh/sz 前缀）；agent 离线时停留预填态，上线后补发，
每个 nonce 只发一次不重放。

生产侧展示口径（ADR-0010）：助手消息按 GFM Markdown 渲染（react-markdown + remark-gfm，
原始 HTML 不渲染、`javascript:` 等不安全链接剥除、图片不渲染、链接新标签页 + noopener、
表格外套横向滚动）；用户消息、通知与工具条目保持纯文本。样式作用域 `.chat-md`，全走
设计令牌，链接不占 `--accent` 预算。流式回复**不出现闪烁光标**（2026-11-03 拍板）——
进行状态由输入框「等待本轮回复…」占位与「取消」按钮承担。「Agent 已连接」**不进对话框**（2026-11-03
拍板）：model/profile/工具面作为「Agent 在线」徽章的悬停 hint 展示（在线期间才有，
离线不给陈旧信息），历史存档中旧版连接通知读回时清除。

生产侧思考过程展示口径（R7，robit 0.1.28 起 wire 有 `thinking_delta` 帧；2026-11-06
拍板对齐 robit-gui 三态）：思考段是对话流中的独立条目、灰字弱样式——**思考中**展开
流式渲染（头行「💭 思考中…」，限高内滚、自动滚尾）；**首个正文/工具事件到达自动折叠**
为「▸ 思考过程」头行；**回合定稿**（turn_complete 兜底闭合）后点击头行可回看本轮思考。
思考文本按纯文本渲染不走 Markdown（流式中半截语法会抖动，GUI 参考实现同款取舍）。
展示性单向瞬态：不入服务端会话档案、也不入 localStorage 缓存（会话重开即消失，与
框架「thinking 不写回 history」语义一致）、不占缓存 30 条额度。前置条件：模型级配置 `~/.robit/config.toml` 的
`thinking = "on"`（DashScope 兼容面拼写，注入 `enable_thinking`）才有思考帧下发；
默认 `auto` 走传统路径无思考帧，界面行为与旧版一致。空白 `text_delta` 帧自 robit
0.1.28 起已在发射端收口（R6 了断），ChatPanel 的空白帧守卫保留为惰性兼容。

右栏「它不会做的事」卡自生产界面**移除**（2026-11-03 拍板；`design/chat.html` 原型保留，
仅作验收基准）：四条边界承诺本就与页头描述（只引用本账户数据）与免责脚注（不代下单、
不构成收益承诺）重叠，卡片的静态承诺语义由这两处兜底。

边界自陈口径随 ADR-0012 更新（2026-11-08）：系统不对接任何交易系统、只是一份账户信息
快照账本——「不代下单」是表述纪律（不得让用户误以为能真实执行）；智能体可代主人写账本
（record_trade 成交录入 / adjust_cash 现金校正，自动落账 + 读回收据，流水只增不改、
错账反向冲正），页头与免责脚注语义不变。

## diagnosis.html — 组合诊断

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `diag-kpi` | 年化波动、最大回撤、夏普、胜率、HHI 等指标条 | `ZM.P`、`hhi` |
| `diag-curve-card` | 组合 vs 沪深 300 滚动对比 | `equity` |
| `diag-conc-card` | 行业集中度：环形图 + 权重条 | `sectors`、`ZMC.donut` |
| `diag-drawdown-card` | 回撤在水下多久（距前高百分比） | `ZM.perf` 结果 |
| `diag-window-table` | 分区间对比（近一周/一月/一季，样本内测算） | `equity` 切片 |
| `diag-fix-card` | 再平衡清单，按「影响大小 × 操作成本」排序，逐条试算调整前后波动 | 03 的波动合成公式 |

## wechat.html — 微信助理接入

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `wx-channel-card` | 接入通道开关，主通道失败自动改用备用通道重试一次 | 本地状态（生产侧：启用开关在服务器配置文件 `[zimu.channels]`，界面只读展示——ADR-0011 拍板⑤⑥） |
| `wx-push-card` | 6 类推送订阅 + 推送时段 + 静音标的 | 本地状态（生产侧界面管理、落 zimu.db——P7-T2 后具备接线条件） |
| `wx-config-card` | 服务号 / 推送参数（AppID、Token、Webhook），**仅存本机浏览器**（原型语义） | `localStorage`。**生产侧改口径（ADR-0011 拍板⑤）**：连接参数在服务器配置文件编辑（`backend/.robit/config.toml` + `.env` 占位，凭据永不入库），卡片仅做**脱敏只读状态展示**；产品定位 = 个人微信助理（OpenClaw 形态），非服务号——本卡文案随 C4 落地重写 |
| `wx-bind-card` | 绑定二维码、绑定状态、解绑二次确认弹窗 | `sessions`（生产侧契约已就位：`/api/channel_bindings` GET/POST/DELETE，`channel_bindings` 表；QQ 本轮以端点操作绑定，二维码流程随微信模块到货） |
| `wx-preview-card` | 推送卡片模板预览（可点按钮） | `ZM` 样本 |
| `wx-log-card` | 最近 6 条推送日志 | 本地状态 |

状态存 `localStorage['zimu:wechat:v2']`。参数输入要做校验，密钥类字段永远不进仓库（`.gitignore` 已覆盖 `.env`/`secrets.*`）。
