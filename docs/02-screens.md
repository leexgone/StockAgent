# 02 · 界面清单与模块职责

七个页面，共享同一套外壳（侧栏 + 顶栏 + 行情条 + 内容滚动区 + 移动端底部标签栏），外壳由 `ZM.chrome({ active, crumb, onRefresh? })` 注入（`design/assets/data.js:380`）。

**双分区布局（2026-11-28，取代旧的文档滚动 + sticky 浮层）**：`.shell` 高度钉死 `100dvh`、页面不再走文档滚动。`.main` 内部分成两个物理区域——**固定头部区**（顶栏 `--topbar-h: 56px` + 行情条 `--strip-h: 38px`，`flex:none`，不参与滚动、不浮动其上）与**内容滚动区** `.content`（`flex:1; overflow-y:auto`，承载各屏 `<main class="page">`）。滚动条只覆盖内容区、范围恰好等于可滚内容，顶栏/行情条作为独立区域始终占位。换屏由 Chrome 的 `#main-scroll` 归零 `scrollTop`。智能问答的全高弹性链改以 `.content` 为链首（chat.css，旧 `body.chat-fill` 补丁作废）。窄屏单列（<1100px）时内容区整体滚、区块完整展开不内滚压缩。打印态还原为文档流（`.content/.main` 放开高度与裁剪）。

## 全局导航与快捷键

| 入口 | 行为 | 实现 |
|---|---|---|
| 侧栏导航 | 五个主界面 + 助理接入，各项右侧 `kbd` 角标 1–6 与热键一一对应（助理接入补 `6`，2026-11-28）；**原「快捷 → 问一句」分组已移除**（用处不大，且其 ⌘K 角标本指向搜索框属误导——直达问答走热键 `5`） | `NAV` + `WECHAT_NAV`（`routes.ts`）/ `Chrome.tsx` 侧栏 |
| 数字键 `1`–`6` | 直接切页（输入框内不拦截） | `bindChrome()`（`data.js:481`） |
| `⌘K` / `Ctrl+K` | 聚焦全局搜索 | `data.js:482` |
| 全局搜索回车 | 命中持仓/自选 → `analysis.html?code=`；否则 → `chat.html?q=` | `data.js:474` |
| 顶栏刷新按钮 | 手动跳一次行情 + 全量重算 + 重绘行情条，再回调页面 `onRefresh` | `data.js:461` |
| 顶栏铃铛 | toast 汇总待处理预警条数与最近一条 | `data.js:468` |
| 行情条 | 5 个指数 + 我的总资产 / 今日盈亏（读实时快照，每次跳动后重绘）。**固定头部区第二行（2026-11-28 双分区）**：与顶栏同属不滚动区（`flex:none`，高钉死 `--strip-h: 38px`，布局计算引用），滚动只发生在其下的 `.content`；早先的 sticky 吸顶方案已废除（浮层语义不符——用户要求头部与内容物理分区）；窄屏超宽走横向滚动（滚动条隐藏），各响应式档行为一致 | `stripHTML()`（`data.js:431`）/ `frontend/styles/ui.css .strip / .content` |
| 表格区 | **有界滚动 + 表头区内吸顶（2026-11-28）**：`.tbl-wrap` 上限 `max(320px, 100dvh − 顶栏 − 行情条 − 128px 余量)`——短表零变化（自然高），长表（流水/历史建议/排序）在区内滚动，`th` sticky 对区滚动口生效（此前 wrap 无纵高约束，表头吸顶是从未生效的死代码）；窄屏横向滚动照旧 | `frontend/styles/ui.css .tbl-wrap / .tbl th` |
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
- **wechat**：五卡不报错——通道状态如实显示未启用/离线、绑定卡空态引导、push 卡待启动态（订阅体系 C15 另案）、推送日志空态（原「推送长这样」预览卡与页头测试推送按钮 2026-11-28 移除，推送真实形态以 push_log detail 原样为准）。

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

**行业（`sector`）自动补全（P7-T6/ADR-0016）**：持仓表「行业 / 类型」列的行业由刷新链顺带补全（股票=新浪行业板名、ETF=基金、转债=转债），**仅补空串、手工指定值优先不被覆盖**（编辑留空仍清，整行覆盖语义不变）；行业有值后行业集中度纪律（03 §6）、dashboard 行业重叠标注、diagnosis 行业集中度自动生效（前端零改动）。空串（未分类）聚一桶参与集中度（保守 fail-safe，03 §6 定稿）。

## analysis.html — 分析建议

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `analysis-picker` | 标的选择（持仓 + 自选，自选走 `watchOnly` 分支） | `holdings`、`watchlist` |
| `analysis-rank` | 按综合得分排序的建议列表，点行切换 | `advice[].score` |
| `verdict-card` | 结论标签（加仓/买入/持有/减仓/卖出/定投）、得分弧、置信度、目标与止损、现价与当日涨跌 | `advice[code]` |
| `discipline-refs` | 纪律参考注记块——**历史留档查看器**（ADR-0024）：仅 v0.1.8 及更早留档行有值才渲染（逐条 label + 文案，warn 色，固定注「历史留档查看」）；ADR-0024 起新建议恒空、自然不渲染——纪律的测算展示在持仓屏 `rule-card` 与诊断屏再平衡清单（均前端现算） | `advice[code].discipline_refs` |
| `kline-card` | 30 日 OHLC 蜡烛 + MA5/MA20，日K/周K 切换，末根锚定现价 | `ZM` 派生 + 本页 `candle()` |
| `dims-card` | 技术 / 基本面 / 资金 / 消息四维分数与观察 | `advice[].dims[4]` |
| `plan-card` | 分笔执行计划（减仓三步、买入两笔 + 失效条件） | `buildPlan()`（`analysis.html:359`） |
| `risk-card` | 「这个结论可能错在哪」 | `advice[].risks[]` |
| `history-card` | 该标的的历史建议与当时结论 | 本页样本数组 |

支持 `?code=` 直达；「写入持仓试算」按钮（`apply-plan`）**本屏直开 TradeDrawer**（2026-11-28 改，不再跳持仓屏）——候选池走共享 `lib/tradeCandidates`（与持仓屏同口径），未建仓自选 = 买入新建、持仓按动作预填方向与第一笔数量（减仓 40% / 加仓 25%，整手取整），提交后 `recordTrade` 回写 portfolio 使本屏 weight 即时刷新；「就这只问我」仍深链 `/chat?q=`。（原「送到持仓页试算」的分析页 → 持仓页闭环出口改为就地抽屉，`/holdings?code=&side=&qty=` 深链仍保留供其它入口复用。）

页头「刷新留档」按钮（原名「重新分析」，2026-11-20 诚实化改名）= **只读重读**建议留档（`GET /api/advice`）+ 该标的 K 线 + 自选行情，**不触发新分析生成**——建议只由盘后复盘 / 问答「/建议」产出（ADR-0007；本屏是留档只读视图，绝不伪造建议）。留档无新增时重读结果不变，盘中点击通常无可见数字变化（仅 toast 反馈）；与顶栏刷新（拉行情 + recalc 重算组合）互补而非等价。空态「去问答触发 /建议」深链才是生成新建议的入口。

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

页头「把最后一条推送给我」按钮（原名「把最后一条推送到微信」，P7-T8 落地 2026-11-21）=
**手动转发**：`POST /api/chat/push_last`（无请求体）取档案最后一条**已归档** assistant
消息（流式中途点击取上一条完成消息），推给微信 + QQ 全部绑定会话——正文带
「子木问答 · 应主人要求转发最后一条回答」头部行，超长归 robit 官方长度守卫
（robit 0.5.1 起：表格线性化投影 + 微信 ≤3 段分段、QQ 单条截断+省略号，03 §12 问答推送口径）。
结果 toast 如实：无 assistant 消息 400、双通道均未启用 ok=false、
发送失败如实报因；细节留痕 `push_log`（kind `chat_push`，最近会话卡可见）。

计划任务（提醒/周期）**仅 IM 通道可用**（ADR-0022）：Web 会话调用 `schedule_*` 工具
如实报不可用（robit 0.8.0 的 M2 官方 Web 接线件已发布但子木不消费，ADR-0022 补记），人格引导主人去微信/QQ 绑定会话创建。

生产侧深链契约（frontend ChatView/ChatPanel）：`/chat?q=<问题>` 只**预填输入不自动发送**
（真实 LLM，交用户确认——全局搜索回车、analysis「就这只问我」走此形态）；
`/chat?q=<问题>&send=1` 在 agent 上线且空闲时**自动发送一次**（仅限意图明确的斜杠指令
深链——两处出口：analysis 空态「去问答触发 /建议」，`q=/建议 <code>`，代码写法按
generate-advice 技能约定：持仓内裸代码、非持仓带 sh/sz 前缀；diagnosis 页头「生成
诊断报告」，`q=/诊断`，P7-T10）；agent 离线时停留预填态，上线后补发，
每个 nonce 只发一次不重放。

生产侧文件上传口径（ADR-0015，2026-11-14）：输入区 📎 按钮（多选）+ 拖拽文件到对话区 +
输入框粘贴截图/文件，三入口共用一套校验与 chips。选中文件先在输入区上方显示 chips
（文件名 + 大小 + 移除，中性色不占 `--accent` 预算，`data-od-id="chat-attachments"`），
点发送才真正上传——`POST /api/chat` 走 `multipart/form-data`（`text` + `files`），
服务端原子落盘 `media/`（消毒 + 防覆盖 + `max_media_bytes` 缺省 20MB 守卫，超限 413）
后注入 Agent：图像走 vision、非图像标注本地路径由标准工具 read/bash 自动解析（ADR-0014）。
空 text 仅文件可发；发送失败保留 text + files 供重试。对话流 user 气泡渲染附件 chips，
服务端档案水合经 `parseFileAnnotations` 解析标注行还原 chips（失败降级纯文本）——
档案存「原文 + 标注行」、模型收原文 + attachments（QQ 同款，零 schema 改动）。

生产侧展示口径（ADR-0010）：助手消息按 GFM Markdown 渲染（react-markdown + remark-gfm，
原始 HTML 不渲染、`javascript:` 等不安全链接剥除、图片不渲染、链接新标签页 + noopener、
表格外套横向滚动）；用户消息、通知与工具条目保持纯文本。对话流对齐：**用户气泡居右
（含附件 chips）、助手/工具/思考/通知条目居左**（IM 惯例，2026-11-21 拍板）。样式作用域 `.chat-md`，全走
设计令牌，链接不占 `--accent` 预算。流式回复**不出现闪烁光标**（2026-11-03 拍板）——
进行状态由输入框「等待本轮回复…」占位与「取消」按钮承担——**输入框本身不锁**
（2026-11-28 修正：原 `disabled={busy}` 使发送瞬间焦点被浏览器夺走，连续输入受阻；
改为流式期间可预打下条内容，发送仍被 busy 守卫拦截，Enter 无副作用；发送成功
焦点即归位输入框）。「Agent 已连接」**不进对话框**（2026-11-03
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

生产侧工具调用卡片展示口径（2026-11-15）：工具条目按「执行中 / 已完成」两态折叠——
**执行中**（`tool_result` 未到）恒展开，显示工具名 + 参数摘要 + 「⏳ 执行中…」进行指示；
**执行完成**（结果到达）默认折叠为头行「▸ 🔧 名称 ✓/✗」，点击头行展开看参数与完整结果
（结果原文纯文本、限高内滚，不走 Markdown）。折叠/展开是纯 UI 态，不写回会话条目、不入
档案——完成态由 `result` 是否存在派生，水合的历史工具行恒带结果故读回即默认折叠（与思考段
三态同款取舍）。前端展示层改动，wire 契约与会话档案 schema 零改动（`ChatPanel` `ToolView`）。

右栏「它不会做的事」卡自生产界面**移除**（2026-11-03 拍板；`design/chat.html` 原型保留，
仅作验收基准）：四条边界承诺本就与页头描述（只引用本账户数据）与免责脚注（不代下单、
不构成收益承诺）重叠，卡片的静态承诺语义由这两处兜底。

边界自陈口径随 ADR-0012 更新（2026-11-08）：系统不对接任何交易系统、只是一份账户信息
快照账本——「不代下单」是表述纪律（不得让用户误以为能真实执行）；智能体可代主人写账本
（record_trade 成交录入 / adjust_cash 现金校正 / upsert_holding · remove_holding
持仓快照校正 ADR-0013，自动落账 + 读回收据，流水只增不改、错账反向冲正、
删除对话内两步确认），页头与免责脚注语义不变。ADR-0014（2026-11-14）起标准工具面
（文件/shell/生图）启用、确认关闭自动执行——边界自陈补文件与 shell 纪律（禁读凭据、
禁直写账本、破坏性操作须主人明示；工具无沙箱，纪律是唯一软约束）。

## diagnosis.html — 组合诊断

| 模块 | 内容 | 数据依赖 |
|---|---|---|
| `diag-kpi` | 年化波动、最大回撤、夏普、胜率、HHI 等指标条 | `ZM.P`、`hhi` |
| `diag-curve-card` | 组合 vs 沪深 300 滚动对比 | `equity` |
| `diag-conc-card` | 行业集中度：环形图 + 权重条 | `sectors`、`ZMC.donut` |
| `diag-drawdown-card` | 回撤在水下多久（距前高百分比） | `ZM.perf` 结果 |
| `diag-window-table` | 分区间对比（近一周/一月/一季，样本内测算） | `equity` 切片 |
| `diag-fix-card` | 再平衡清单，按「影响大小 × 操作成本」排序，逐条试算调整前后波动 | 03 的波动合成公式 |
| `report-btn`（生产侧，P7-T10） | 页头「生成诊断报告」= 深链 `/chat?q=/诊断&send=1`（agent 上线后自动发送一次，与 analysis 空态 /建议 同契约）；净值样本 <2 不跳、toast 说明态。IM 绑定会话直接说 /诊断 同样触发（能力面渠道无关，ADR-0011）；报告由 diagnosis-report 技能生成，数字取 `calc_diagnosis`（与本屏共用 domain bench/perf 口径），不推送、不另立留档——对话即档案 | `useEquity` 样本数 + chat 段深链契约 |

## wechat.html — 助理接入

屏名与导航文案 2026-11-17 由「微信助理接入」更名「助理接入」（通道无关，QQ 同样受益；`design/` 原型标题冻结不改）。

生产侧落地（2026-11-17，ADR-0017 / C4 关闭）：渠道 = **微信官方 iLink AI Bot**（robit-wechat 0.3.0，1:1 私聊、出站长轮询无需公网回调，非服务号）。六卡生产形态（`WechatScreen`，`data-od-id` 沿用原型名）：

| 模块 | 生产形态 | 数据依赖 |
|---|---|---|
| `wx-channel-card` | QQ / 微信 / Web 三行状态**只读**（在线=ok 绿 / 离线=warn / 未启用=plain，头徽章 N/3 在线）；无 switch、无逐通道「测一下」。启用开关在服务器配置文件 `[zimu.channels]`，界面只读展示（ADR-0011 拍板⑤⑥） | `health.qq_online/wechat_online/agent_online` + `qq_enabled/wechat_enabled`（启动时开关快照，区分「未启用」与「离线」） |
| `wx-push-card` | **待启动态**：上段「当前生效（绑定即推）」如实列两条真实推送（预警推送 / 15:05 盘后数字摘要，无开关）；下段订阅管理禁用占位 + 另案 banner。6 类订阅 + 时段 + 静音落库 = **C15 另案**（先对齐 vision/原型词汇，再 schema v5 + 03 口径）；**不枚举六类标签**（词汇冲突未决，渲染任一即提前站队） | 静态 + push kind 词汇 |
| `wx-config-card` | **iLink Bot 脱敏只读**（服务号 AppID/Token/Webhook 文案全废，ADR-0011 拍板⑤兑现）：渠道形态 / 凭据位置（bot_token 存服务器 `backend/.robit/wechat-state.json`，CLI 扫码换取，**界面不显示任何 token 值**）/ 登录命令 + 停服约定 / 出站长轮询无需公网回调 / 在线状态行。**零输入框、零 localStorage** | health + 静态文案 |
| `wx-bind-card` | **通道无关白名单管理（QQ 同样受益）**：**待绑定会话自动列出**（D3 补记：未绑定来消息被 BoundOnly 丢弃时入观察队列——channel 徽章 + chat_id + 来消息次数/最近时刻/消息摘录 ≤40 字 + 一键「绑定」按钮，kind 由 chat_id 前缀推断；绑定成功即移出队列；内存态重启清零，「刷新」按钮 + 60s 轮询）。**该区可折叠（含下方手工表单与说明文字整块）**（2026-11-28）：无待绑定项默认收缩成标题行 + 一行摘要（`pending-collapsed`），有新会话进来（pending 计数增长，含首载异步到达）自动展开；用户手动开合由 `pendingOpen` 控制、不因轮询复位；展开态 chevron 标题按钮 `pending-toggle`（`aria-expanded`/`aria-controls` 指向 `#pending-list`）；收缩态标题行仍示计数 pill 与「刷新」按钮；活跃绑定列表**不随折叠**；活跃绑定列表（channel 徽章 + chat_id mono + kind + label + bound_at）+ 行内解绑**两步确认弹窗**（Esc/scrim 可关，danger）；手工新增表单兜底（channel 选 qq/wechat、**选 wechat 时 kind 锁死 private**——微信仅 1:1、label 可选；服务端日志仍留 chat_id 案底）。**观察自动化、授权仍人工**：入列 ≠ 放行，绑定点击是主人明示动作（fail-closed 不变）。**扫码不上 Web**（robit 无非交互式登录 API；登录 = 部署侧 `robit-wechat login`，停服约定见 backend/README）。**chat_id 展示脱敏**（2026-11-17 增补）：活跃绑定与待绑定两列表默认掩码（`lib/maskChatId`——保留 private:/group: 前缀与 @domain，纯数字留前 2 后 2、ilink id 留前 4，中段 ****，≤4 字符全掩），逐行「查看/收起」切换（组件态，离屏复位回掩码）；动作 toast 同掩码；掩码只是展示层投影，API/store 与绑定请求仍用完整 chat_id | bindings store（`/api/channel_bindings` GET/POST/DELETE + `/api/channel_bindings/pending` GET，`channel_bindings` 表 + 内存观察队列） |
| ~~`wx-preview-card`~~ | **已移除（2026-11-28）**「推送长这样」样例卡——纯展示无操作、不承担真值留档；推送真实形态以 `wx-log-card` 的 push_log detail 原样为准（分段/长度守卫语义注记仍在 log 卡与 03 §12） | — |
| `wx-log-card` | 最近 6 条**全通道** push_log（channel 徽章 + ok/warn 圆点 + kind 标签 + **detail 原样**不另造文案 + ts）；空态生产文案 | usePushLog（只读 store 复用） |

页头不设动作按钮（原「发送测试推送」禁用占位已移除，2026-11-28——测试推送端点另案，理由本就是「新增契约面 + 无鉴权服务上的真实发送滥用面」，占位无价值）。屏挂载即 load bindings/health/pushLog + 屏内 60s 轮询（卸载清理）。状态色 B5：已绑定/在线一律绿 `--ok`、离线/未绑定 warn，绝不用红；`--accent` ≤2（仅绑定提交 btn-primary）；空账户六卡不报错。

IM 会话命令面（robit 自持，非本子木界面；ADR-0022，2026-11-24）：绑定会话里自然语言创建计划任务（提醒/周期，`schedule_*` 工具，**仅 IM 生效**）+ `/schedule [list|pause|resume|delete <id前缀>]` 直管（robit 0.7.0 M1）；口径见 03 §12 计划任务小节。Web 侧 `schedule_*` 如实报不可用（robit 0.8.0 已发布 M2 Web 接线件，子木不消费，ADR-0022 补记）。技能触发词（/复盘 /盘前 /建议 /诊断）在绑定会话同样生效——能力面渠道无关（ADR-0011「嘴不同手相同」），Web 诊断屏「生成诊断报告」深链与 IM 直接说 `/诊断` 走同一技能（P7-T10）。

**原型语义作废清单**（`design/wechat.html` 冻结参照，生产不实现）：主通道失败自动改备用重试一次（生产推送失败不自动重试，ADR-0011）、服务号参数输入与校验、演示二维码 + TTL 换码（登录 = 部署侧 CLI 扫码）、浏览器桌面通知备用通道、推送卡片按钮（微信纯文本）、「推送内容包含具体价格」开关与 6 类订阅开关（C15 另案）。原型 `localStorage['zimu:wechat:v2']` 状态键不入生产（绑定落 zimu.db、订阅待 C15、屏无本地状态）。密钥类字段永远不进仓库（`.gitignore` 已覆盖 `.env`/`secrets.*`；`wechat-state.json` 含凭据已被 `**/.robit/*` 覆盖，不加豁免）。
