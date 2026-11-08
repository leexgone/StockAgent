# 06 · 路线图与执行计划

原则：**先在原型里把产品想清楚。** 技术路线已定（ADR-0005：Rust + robit 基建 / React + Vite 前端 / Python 行情 sidecar / SQLite）。每阶段都有「完成标准」，达不到就不进下一阶段——不要靠新增依赖来掩盖没做完的事。

**本文档怎么用**：后续工作按阶段推进，任务逐条勾掉；任务有编号（如 `P2-T1`），commit 正文与 ADR 引用编号即可定位。状态标记：⬜ 待做 / 🔨 进行中 / ✅ 完成 / ⏸ 阻塞（注明被什么阻塞）。估算按「单人 + Claude 协作」的可用时间粗算，只用于排节奏，不是承诺；变了就改本文档，不另开周报。

## 0 · 总览

| 阶段 | 目标一句话 | 前置 | 状态 | 估算 |
|---|---|---|---|---|
| P0 | 工程与文档基线 | — | ✅ 2026-10-24 | — |
| P1 | 原型定稿（修完 backlog 前两级） | 无，可与 P2/P3 并行 | ✅ 2026-10-29（T1–T9 全落地，T9 人工验收通过） | 3–4 天 |
| P2 | 技术基座 Spike（验证 0005 每个假设） | ADR-0005 ✅ | ✅ 2026-10-27 | 2–3 天 |
| P3 | 口径可执行化（黄金测试向量） | P2 | ✅ 2026-10-28（`price_limit` 尾巴随 P1-T2 消除，8 组 76 用例两边全绿） | 2–3 天 |
| P4 | 数据管道与存储 | P3 ✅；C5/C8 拍板 ✅（2026-10-28，ADR-0006/0007） | ✅ 2026-10-29（T2–T7 全落地；T1 前端抽屉接线随 P5-T2 holdings 屏于 2026-10-30 收口） | 1–1.5 周 |
| P5 | 前端移植（7 屏 → React） | P4（API 面冻结）；**C4 拍板** | 🔨（T1 ✅ 2026-10-29；T2 六屏 ✅ 2026-10-30，余 wechat 待 C4；T3 ✅ design/ 冻结 2026-10-30） | 1–1.5 周 |
| P6 | MVP 垂直切片（连续 5 个交易日真实跑通） | P4+P5；C4 渠道落地 | ⬜ | ≥5 个交易日 |
| P7 | MVP 后迭代池（任务自 `07` backlog 与新需求登记；首项：模型思考过程展示链路） | P6；robit 侧 R7 发版 | 🔨 2026-11-07（P7-T1 两侧代码落地余走查；P7-T2 智能体接入模块子木侧全量落地，QQ 真机核心链路走查通过 2026-11-08，余项见任务行；P7-T3 agent 写账本工具面 2026-11-08 立项落地，余三端人工走查） | 按任务行 |

**并行规则**：A 轨 = P1（只动 `design/` 与 `docs/`），B 轨 = P2→P6（新目录）；两轨在 P5 之前互不阻塞。P5 完成后 `design/` 冻结为参照。

**决策闸门**（详情见 `07-backlog.md` 待决策表）：

| 决策 | 最晚拍板点 | 卡住什么 |
|---|---|---|
| C5 首笔持仓录入方式 | ✅ 已拍板（2026-10-28，ADR-0006：手工逐笔先行） | SQLite schema、导入端点、P1-T1 空态入口 |
| C8 打分生成方式（规则/模型/混合） | ✅ 已拍板（2026-10-28，ADR-0007：混合——规则打分 + 模型写文案） | Skill 模板与 `score/conf/dims` 的可解释性设计 |
| C4 微信渠道 | P5 移植 wechat 屏前 | PushProvider 配置界面；渠道适配器本身可延到 P6 |
| B6 正则改模板插值 | ✅ 已解（原型 2026-10-28 P4-T5 模板插值；生产侧 2026-10-29 P4-T6 接线——calc_advice 数字先落库、finalize_advice 只回填文案、Skill 硬规则约束数字来源） | — |

## P0 · 工程与文档基线 ✅（2026-10-24）

仓库卫生（`.gitignore` / `.gitattributes` / `.editorconfig`）、根 README、CLAUDE.md、docs/ 规格与口径手册、ADR。完成标准已达成：打开仓库 10 分钟内知道这是什么、改界面前读哪几页、哪些事明确不做。

## P1 · 原型定稿 ✅（2026-10-29）

| # | 任务 | 验收 | 估算 | 状态 |
|---|---|---|---|---|
| P1-T1 | A2 空态：空持仓引导界面 + 「录入第一笔」入口壳（C5 已决：入口 = 手工录入表单抽屉，ADR-0006） | 7 屏在空持仓下不报错、有空态引导 | 1d | ✅ 2026-10-28：`?empty=1` 空态模式（sessionStorage 跨页）+ `ZM.emptyBlock()` 统一引导；7 屏空态分支全接（index 三步引导路径、holdings 主按钮/抽屉「录入第一笔」+ `?add=1` 自动开、diagnosis 指标不伪装 0=安全、wechat 模板占位）；charts 空序列防御。冒烟 file/http × 正常/空态四组全绿、正文无 NaN/-Infinity 泄漏，A2 存档见 `07` |
| P1-T2 | B2 涨跌停：分板块 ±10/±20/±5，转债口径定稿，显示与拦截阈值一致 | `03` 第 2 节 ⚠️ 消除 | 0.5d | ✅ 2026-10-28：`03` §2 重写定稿——比例是标的属性（主板 ±10/ST ±5、创业板与科创板 ±20、转债 ±20 对称；ETF 无法由代码推断 → `limitPct` 数据层配置；北交所超范围）；限价整数单元 half-up（与交易所一致）；显示与拦截同一对限价含边界。`price_limit` 向量转 active（24 用例），Rust/TS 两侧同步补实现全绿（76 向量），原型抽屉接入 |
| P1-T3 | B3+B4 过户费文案统一；小数位按 `kind` 决定并收进 `data.js` | `03` 第 1/3 节 ⚠️ 消除 | 0.5d | ✅ 2026-10-28：文案改「过户费 0.001%（沪深两市均收）」（代码本就对）；小数位收进 `data.js` `priceDec(kind)`（stock 2、ETF/转债 3），7 屏全量替换、analysis 删 `last2`、watchlist 补 `kind`、转债文案字面量转 3 位；`03` §1/§3 ⚠️ 消除 |
| P1-T4 | B5 `up/down` 语义分离：引入 `--ok/--danger` 令牌，逐处替换非涨跌语义 | `05` 令牌表更新、`03` 第 11 节 ⚠️ 消除 | 1d | ✅ 2026-10-28：三类分离——好坏轴 `.ok/.warn/.danger`（得分弧/维度条转红绿灯）、交易方向 `.buy/.sell`（`--up/--down` 别名令牌）、`up/down` 退回涨跌数字与标记；新增 `--ok-soft/--danger-soft/--buy/--sell` 令牌，frontend 镜像同步；`05` 令牌表与规则 3、`03` §11、`04` gauge 行更新 |
| P1-T5 | B7 交易所用显式 `mkt` 字段（analysis 展示 + holdings 新建持仓路径） | 不再出现 `code[0]==='6'` | 0.25d | ✅ 2026-10-28：watchlist 补显式 `mkt`，分析页页头与 holdings 新建持仓改读 `mkt`，顺带删掉 analysis 试仓金额的 `? 100 : 100` 死 ternary（按 `kind` 取 1 手）；`04` 契约同步（watchlist 补 `mkt/kind/limitPct`、工具表补 `priceDec/limitPrices`） |
| P1-T6 | B8 死 ternary 修复（现金规则进度条方向） | 进度条语义与另三条一致 | 0.1d | ✅ 2026-10-28：现金规则进度改 `v/c`（满格 = 降至下限，`c=0` 由 `min` 截断无 NaN），行标签按 `inverse` 显示「下限/上限」；四条统一「满格 = 触及限制」 |
| P1-T7 | B9 原型级处理：历史建议标注为样本、结构对齐未来的 `adviceHistory`（正式解在 P4-T2） | `analysis` 页不再假装是真实留档 | 0.5d | ✅ 2026-10-28：卡片加「演示样本 · 非真实留档」标注与样本 pill；样本字段对齐未来契约（每行带 `generatedAt` / `dataVersion=demo-sample-v0`），「日期」列改「生成时间」；`04` 第 6 条同步；正式解归 P4-T2 |
| P1-T8 | C11 `design/README.md` 收敛：保留「怎么打开」，其余改链接指向 `docs/` | 无双源冲突 | 0.25d | ✅ 2026-10-28：README 重写为「定位 + 打开方式 + docs/ 导引表（01–07）+ 许可」，删除界面表/设计约定/数据契约/业务规则/本地状态/已知边界六段重复内容；backlog C11 关闭 |
| P1-T9 | 人工验收：按 CLAUDE.md 验证方式过 7 屏（`file://` + http.server、窄屏、真实时钟、刷新按钮） | 控制台零报错 | 0.5d | ✅ 2026-10-29：用户按清单人工走查通过——7 屏 × 两种打开方式（file:// + http.server）控制台零报错；窄屏 <900px 抽屉/底部标签、T4 三轴配色审认、真实时钟（盘后态 + 晚间问候）、顶栏刷新重算数字一致、预警→建议→试算→结论链路不离开原型、`?empty=1` 空态逐屏走查均过 |

**完成标准**：✅ 达成（2026-10-29，T9 人工验收）——七个界面在 `file://` 与本地服务两种方式下打开均无控制台报错；从「预警 → 建议 → 试算 → 结论」走一遍不需要离开原型；同一个数字在任意两处出现都相等。

## P2 · 技术基座 Spike ✅（2026-10-27）

四个 spike 验证 ADR-0005 的每个假设；**目录（`backend/` `frontend/` `datafetch/`）随对应 spike 创建**，勿提前半建。

| # | 任务 | 产出 | 估算 | 状态 |
|---|---|---|---|---|
| P2-T1 | `datafetch` spike：uv + FastAPI + akshare，取样本标的（股票/ETF/转债/指数各一）的实时行情与日线，记录字段、盘中频率、限频表现 | 与 `04` 契约的字段对照表（并入 `04` 附录）+ 可运行脚本 | 0.5–1d | ✅ 2026-10-26：三轮探测 + API 冒烟通过；主通道=新浪直连，东财间歇需熔断，baostock 登录失败待 P4 复测，详见 `04` 附录 |
| P2-T2 | robit 集成 spike：`backend/` Cargo workspace + path override 指向本地 robit；最小 agent 会话接阿里云网关 qwen，注册一个 echo 领域工具 | `cargo run` 可对话 | 0.5–1d | ✅ 2026-10-27：robit 0.1.25 已发布 crates.io，改为直接引用（无需 path override）；`zimu-agent-cli` REPL + echo 工具全链路验收通过——qwen 流式对话正常，模型 function-calling 正确调用 echo 返回 `[子木 echo] …`（期间定位并更换失效的 DASHSCOPE_API_KEY；CLI 保留脱敏 api_key 打印便于日后 401 排查） |
| P2-T3 | axum + SSE spike：一个 REST 端点 + 把 robit-agent 事件流经 SSE 转出；同时评估「Web Frontend 桥」该不该进 robit（R 系列第一条候选） | 浏览器/curl 看到流式输出 | 0.5d | ✅ 2026-10-27：Agent 进 zimu-server（与 CLI 同装配，CliFrontend 换成 SSE 桥——broadcast 扇出 + 确认槽位 + 降级 503）；curl 验收通过：connected → tool_call(echo，中文参数无损) → tool_result → 流式 text_delta → turn_complete，keep-alive 正常；首页 ChatPanel 可浏览器对话；「Web 桥进 robit」评估落为 `07` R1（P2-T5 首选素材），另记 R2/R3 |
| P2-T4 | `frontend` spike：Vite + React + TS 脚手架，`ui.css` 设计令牌迁移（oklch、红涨绿跌、tabular-nums、断点），复刻 index 一屏静态版 | 与原型视觉可对照 | 0.5–1d | ✅ 2026-10-27：脚手架 + 令牌/组件类迁移（React 19 / Vite 6 / TS 5.6，对齐 robit-gui）；index 一屏静态复刻完成——Chrome 外壳（侧栏/顶栏/行情条/移动底栏，市场时钟每秒真实走针）+ 问候区/快照卡/六入口卡/时间线/待决事项；`demo.ts` 同口径移植 `data.js`（含确定性净值曲线与 perf），41 项派生数字与原型对照逐位一致；SPA 视图切换承载 P2-T3 会话（智能问答视图），未移植屏为占位（P5 接路由） |
| P2-T5 | 共同演进回路演练：拿一个真实框架级改动（建议就用 T3 的 SSE/Web 桥）走完 改 robit → tag → 子木 pin rev 升级 | robit 有新 tag；痛点已记入 `07` R 系列 | 0.5d | ✅ 2026-10-27：R1 落地——robit 新基线分支 `feat/web-bridge` 新增通用 crate `robit-web`（`WebEvent` 官方 wire schema + `WebFrontend` broadcast 扇出/单槽位确认 + `WebBridge` 消息注入；零 HTTP 框架依赖，单测 9/9），提交 9e3b15d + 36b0149，本地 tag `v0.1.26`；子木 backend pin 切 `git file:///E:/Github/robit tag=v0.1.26`，zimu-server 删除宿主手写桥（SseEvent/SseFrontend/确认槽位约 150 行）改消费 robit-web，curl 冒烟 wire 逐帧兼容（connected → tool_call → tool_result → text_delta → turn_complete，前端零改动）。**未完事项**：robit `feat/web-bridge` 分支稳定后再合并推送（2026-10-27 用户定，不急于回 main）+ tag 推送 + crates.io 发布后，子木 pin 换回 registry 写法（`robit-* = "0.1.26"`），见 `backend/README.md` |

**完成标准**：spike 全部跑通；若任何 0005 假设被证伪（robit API 不适配、akshare 拿不到转债数据等），回 ADR 修订再继续。

## P3 · 口径可执行化（黄金测试向量） ✅（2026-10-28）

| # | 任务 | 产出 | 估算 | 状态 |
|---|---|---|---|---|
| P3-T1 | fixture 设计：JSON 格式（公式 id / 输入 / 期望输出），放 `fixtures/golden/`，前后端共享同一份 | schema 说明进 `03` 附录 | 0.5d | ✅ 2026-10-27：schema v1 定稿进 `03` 附录（比对规则 6 条 / 公式组→函数映射表 / 新增口径约定 5 条：sell_to 按 lot 泛化、perf 短序列归 0、清仓 after_cost=null、压线算安全、费用不舍入）；runner 随 P3-T3/T4 落地 |
| P3-T2 | 手算可对用例集：`03` 每条公式至少 3 组（常规 / 边界 / 空持仓），数字可在纸上复核 | fixture 文件 | 1d | ✅ 2026-10-28：八组 76 个 active 用例（lot 16 / fees 7 / dilution 7 / recalc 4 / discipline 9 / perf 4 / vol_synth 5 / price_limit 24——随 P1-T2 定稿补齐转 active），三类标签齐备、desc 附精确分数手算过程；口径缝隙记入 `07` B11/B12 |
| P3-T3 | Rust 领域纯函数库（backend 内独立 crate）：整手 / 费用 / 摊薄 / 涨跌停 / 仓位与纪律四条 / 净值指标 / 波动合成 / recalc 汇总，`cargo test` 跑 fixture | 全绿 | 1–2d | ✅ 2026-10-28：`zimu-domain` crate——7 个纯函数模块 + `tests/golden.rs` runner（按 03 附录比对规则：容差分级/null/数组按序/未列字段不校验/pending 跳过），`cargo test -p zimu-domain` 全绿（9 单测 + 52 黄金向量）；期间向量抓出一处期望值手算错误（pf-normal 的 mdd 应为 −5.5/110 = −5%）并修正 fixture。涨跌停（`price_limit.rs`：比例推断/整数单元限价/委托价校验）随 P1-T2 于 2026-10-28 补齐，现 11 单测 + 76 向量全绿 |
| P3-T4 | TS 实现（frontend 共享层）：抽屉试算等即时交互用，同一组 fixture 跑测试 | 全绿 | 1d | ✅ 2026-10-28：`frontend/src/domain/`——8 个模块镜像 `zimu-domain`（模块划分/字段名/运算顺序一致，snake_case 字段对 fixture 键）+ `golden.test.ts` runner（比对规则与 Rust 侧同款，另判 NaN/Inf 失败）；引入 vitest 5（frontend 首个测试依赖），`npm run build` 不受影响。`price_limit.ts` 随 P1-T2 于 2026-10-28 补齐，现 `npm test` 84 项全绿（76 向量 + 8 组校验，无跳过）。P5-T2 holdings 抽屉试算直接消费 |

**完成标准**：两边测试全绿；任何人可以用 fixture + 一份持仓表在纸上复算出界面上的每一个数字。

## P4 · 数据管道与存储

| # | 任务 | 产出/验收 | 估算 | 状态 |
|---|---|---|---|---|
| P4-T1 | 录入流程实现：手工逐笔先行（ADR-0006 已决），券商 CSV 导入为后续增强单独立项 | backlog C5 关闭 | 0.5d + 实现 | 🔨 2026-10-28：后端录入完成——`zimu-store` 事务（校验/试算/落库全复用 `zimu-domain`，不重定义口径）+ `zimu-server` 端点 `/api/holdings` `/api/holdings/trade` `/api/trades` `/api/account`；16 项集成测试 + e2e 全绿（首笔建仓/加仓摊薄/清仓移除/整手拦截/涨跌停拦截/现金不足回滚/超上限警告/重启不丢）。C5 关闭（见 `07`）；CSV 后续增强单独立项。**前端抽屉接 API ✅ 2026-10-30（随 P5-T2 holdings 屏，试算走 `src/domain`、提交以后端收据覆盖）** |
| P4-T2 | SQLite schema：holdings / trades / advice_history（带生成时间与数据版本，解 B9）/ alerts / equity_snapshots / push_log，含迁移机制 | 重启不丢数据 | 1d | ✅ 2026-10-28：`zimu-store` crate——v1 schema 六表齐（`account/holdings/trades/advice_history/alerts/equity_snapshots/push_log`）；`PRAGMA user_version` 逐版迁移、每版一事务、只前进；`rusqlite` bundled（自带 libsqlite3，版本对齐 robit-agent 的 0.40 避免 links 冲突）。`holdings.shares` 为普通整数（奇零股存得下，解 B12 存储前提，见 `07`）；`advice_history` 带 `generated_at + data_version`（B9 正式解，字段留待 P4-T6 写入）；`trades` 拆三分项费用可审计；`.db` 已在 `.gitignore`（真实持仓永不入 git）。`restart_does_not_lose_data` 测试验证重启不丢 |
| P4-T3 | `datafetch` 正式化：`/quote` `/kline` `/cb_list` `/calendar`，akshare→baostock 兜底、限频与缓存、错误契约 | 主源故障时降级不崩 | 1–2d | ✅ 2026-10-28：`zimu_datafetch` 包（源链编排+熔断+限频闸+TTL 缓存+`DfError` 契约）。兜底链按 spike 实测重排（新浪系→baostock→东财直连/akshare 东财），**baostock 复测登录恢复**（spike 的 10002007 为暂时故障）进股票/指数日线与日历链；`ZIMU_DF_DISABLE` 故障注入实现验收——主源挂→备份顶上（`degraded`+`attempts` 如实上报）、全挂→502 JSON 契约且服务无恙。B7 落地：symbol 必须显式 `sh/sz/bj` 前缀，裸代码 400。36 项离线单测 + 18 项联网冒烟全绿（退市样本 110074/127022 三态、活券布局补测、东财 fltt=2 缩放与新浪逐位一致）；见 `datafetch/README.md` 与 `04` 附录「正式契约」 |
| P4-T4 | Rust 行情适配器（唯一消费入口，sidecar 挂了主程序降级）+ 服务端 `recalc()` 等价实现 + 交易日历/时区口径 | API 数字与 `03` 口径一致 | 1–2d | ✅ 2026-10-28：`zimu-market` crate（类型化契约 + `MarketApi` trait 注入点 + 错误三分类 Unavailable/Upstream/Contract + 市场时钟纯函数 + 日历缓存 + 当日 bar 合成 + B7 前缀拼装）；`/api/market/*` 六端点进 zimu-server（lib target 拆出 `api_router` 供集成测试）；`POST /api/market/refresh` = 行情回写（`apply_quotes` COALESCE，limit_pct 仅未配置时推断）→ `zimu-domain::recalc` 全量重算。口径落档：`03` §9 生产口径定稿（恒 Asia/Shanghai、日历取代工作日启发式、降级如实标注）+ §5 无行情降级定稿（prev 缺失视为 price，**修掉 P4-T1 遗留的 prev=0 伪造当日盈亏 bug**）+ `04` 附录 B 当日 bar 合成与适配器边界。验收：29（market）+ 8（server 集成，API 数字与 domain 逐位等价 + 手算断言）+ 20（store）全绿离线；联网 e2e——真实行情刷新（600519 现价回写、limit_pct→10、per/agg 手算逐位吻合）、ETF 当日 bar 实盘合成（尾部 10-27→合成 10-28）、停 sidecar 后 refresh/quote 503 契约、holdings 200 旧值照常、时钟靠日历缓存不降级 |
| P4-T5 | `04` 契约 → OpenAPI + serde 结构体（前后端类型同源），顺带解决 B6（模板插值替代正则） | 契约测试：样例响应过 schema 校验 | 1d | ✅ 2026-10-28：类型同源全链落地——契约 serde 结构体在定义处 `#[derive(ToSchema)]`（utoipa 5，domain/store/market/server 四 crate 36 schema），`zimu-server/src/openapi.rs` 汇编 OpenAPI 3.1 → `docs/api/openapi.json`（永不手改：up-to-date 逐字节断言 + `--ignored` 再生成入口）；status/kline/refresh/错误体从 `json!` 拼装改类型化结构体（`MarketStatusResponse`/`KlineResponse` flatten/`RefreshResponse`/`ApiError`）；契约测试 `tests/contract.rs`（jsonschema draft 2020-12）——seeded 服务 16 端点**实时响应**过 schema + `fixtures/api/` 15 份**提交样例**过同一校验（`ZIMU_REGEN_API_SAMPLES=1` 再生成）+ 运行时 `/api/openapi.json` 与仓库文件相等；前端 `npm run gen:api`（openapi-typescript 7）生成 `src/api/schema.d.ts`（build/vitest 全绿），P5-T1 client 直接消费；health/chat 在 bin 以 doc-stub 进契约 + 键集一致单测。B6 原型正式解：建议文案 `{w}/{pnl}/{price}/{drag}` 模板插值（数字只取 recalc 快照），正则改写块删除，沙箱 22 项断言（见 `04` 附录 C、`07` 存档） |
| P4-T6 | **C8 拍板** + agent 领域工具（get_quote / get_holdings / calc_discipline / simulate_trade）+ Skill 模板（盘后复盘 / 盘前关注 / 建议生成，内嵌 `03` 口径与免责边界）；打分按 ADR-0007 混合方式——规则函数出数、模型写文案 | 复盘输出的每个数字可回溯到 P3 公式 | 2–3d | ✅ 2026-10-29：`zimu-agent` 公共 crate（P2 期 echo 两处重复收敛）——8 领域工具（market_status / get_quote 含 §2 涨跌停拼装 / get_holdings / calc_discipline 含 §6 sell_to 降仓提示 / simulate_trade 只试算不落库 / refresh_quotes 与 HTTP refresh 同链（`service::refresh_portfolio` 抽取，契约 wire 零变化）/ calc_advice / finalize_advice；**无下单类工具**——只给建议不代下单）+ Skill 模板 ×3（仓库根 `skills/`，robit SKILL.md 约定，/review /复盘 /watch /盘前 /advice /建议 触发，内嵌 03 口径引用、免责与「数字必须原样取自工具返回」硬规则）。打分规则函数 `zimu-domain::advice::score_advice`（03 §10 生产口径定稿：信号票制 + 中性维 60 + 70/55/45 三带 + 纪律拦截优先；TS 共享层镜像），黄金向量 `advice.json` 10 例两边全绿（现 9 组 86 例）。留档：calc_advice 数字列直接写 `advice_history` 返回 advice_id，finalize_advice 只回填 reason/risks——数字不经模型转写（ADR-0007 结构性保证）；data_version=`adv-v1\|k=…\|q=…`（B9 写入侧）。验收：黄金向量 av-bull-add 同款输入端到端过工具链（日线 24 根 + 当日 bar 实时合成 → 打分 → 留档逐位一致），集成测试 10 项全离线绿 |
| P4-T7 | 调度与预警引擎：盘中分钟级轮询（价格/放量/止盈止损/期限）+ 15:05 复盘触发 | 模拟盘中可触发 | 1d | ✅ 2026-10-29：口径先行——`03` §12 预警与调度生产口径定稿（8 类判定序 + 常量表 + 去重/降级/幂等规则），黄金向量 `alerts.json` 17 例两边全绿（现 10 组 103 例；判定唯一实现 `zimu-domain::alert::eval_alerts` + TS 镜像）。引擎 = `zimu-agent::scheduler`（纯编排：`tick(now)` 注入时刻可测——市场时钟闸门 → 刷新链复用 `service::refresh_portfolio_detailed` → 事实组装（限价 §2 / 最近留档 stop+target / 日线量窗口每日缓存 / cb_list 每日一次）→ domain 判定 → 同日 (code,type) 去重落 `alerts`）；15:05 盘后链 = 最终刷新 → `equity_snapshots`（date 主键幂等，数字 = recalc agg 原值）→ `/复盘` 注入 agent 会话（push_log 留痕；agent 离线快照照写）。新端点 `GET /api/alerts` `POST /api/alerts/{id}/done` `GET /api/equity_snapshots`（契约再生成 + 样例 4 份 + `npm run gen:api`）。验收「模拟盘中可触发」：集成测试 7 项全离线（模拟 10:30 触发跌破成本+止损并去重、非交易日/午休空转不打 sidecar、sidecar 挂降级不判旧值、15:05 快照手算锚点+复盘回调恰一次、agent 缺席 push_log 如实、cb 强赎+放量与日级缓存计数）；实机冒烟——服务启动「预警调度器已启动」、pre_open 空转 debug 轮询如实。顺带修复：robit init_logging 只放行 target crate，宿主 `zimu_agent/zimu_market` 日志被默认底线吞掉 → 附加 directive 跟随 log_level 放行 |

| P4-T8 | 持仓直接维护（ADR-0009，2026-11-01 用户需求增补）：与交易录入互补的**快照校正**入口——直接新增 / 编辑 / 删除持仓 + `holding_edits` 审计留痕 | 初始化映射 / 纠错 / 场外变动可做；校正不污染流水与现金账 | 0.5d | ✅ 2026-11-01：口径先行——`03` §13 定稿（整行覆盖 upsert、**现金不联动**（用户拍板）、不伪造 trades、允许奇零股、跳过 lot/涨跌停（校正不是委托）、price/prev 行情字段不可编辑、幂等 no-op）+ ADR-0009；`zimu-store` **schema v3**（`holding_edits` 表）+ `upsert_holding/remove_holding/holding_edits`（编辑痕与持仓写入同事务，新增 6 项单测，现 34 项）；`zimu-server` `PUT/DELETE /api/holdings/{code}` + `GET /api/holding_edits`（契约再生成 + 样例 3 份 + contract 断言）；前端 `HoldingFormDrawer`（页头「直接录入」+ 行内「编辑」）、行内两步「删除」、**「维护记录」卡**（用户拍板露出留痕）。验收：workspace 全绿；双库冒烟——全新空库全生命周期（奇零股 create / 幂等 / limit_pct 保留 / 现金与流水三不动 / 删除留痕 / 404/400 / 与交易路径交错）+ **真实 v2 库副本自动迁 v3 数据无损**、真实行校正正常，两阶段 Ctrl+C 优雅关闭 rc=0 |

**完成标准**：重启后端不丢持仓与留档；行情更新后 API 数字与 `03` 口径一致；换数据源实现不动 API 面。——✅ 2026-10-29 达成（`restart_does_not_lose_data`、API 数字与 `recalc` 逐位等价、`MarketApi` trait 注入点换实现不动端点；快照 date 主键幂等重启不重复）。T1 的前端抽屉接 API 已随 P5-T2 holdings 屏于 2026-10-30 收口。

## P5 · 前端移植 🔨

| # | 任务 | 产出/验收 | 估算 | 状态 |
|---|---|---|---|---|
| P5-T1 | 工程约定：路由、API client（由 OpenAPI 生成类型）、状态方案、组件策略（手写组件 + 设计令牌，不引 UI 库——要引先补 ADR） | 约定写进 CLAUDE.md | 0.5d | ✅ 2026-10-29（ADR-0008）：react-router v7 路由表 `src/routes.ts`（路径与原型文件名一致，未移植屏 = 占位屏，SPA 深链冒烟通过含 `/analysis?code=` 与未知路径回落）；openapi-fetch 客户端 `src/api/client.ts`（REST 唯一入口，SSE 例外，类型自 `gen:api`）；zustand 状态方案（派生数字后端唯一真值，抽屉试算用 domain TS 库）；组件 = 手写 + 设计令牌（不引 UI/图表库/Tailwind）。约定已写进 CLAUDE.md；npm test 113 + build 全绿 |
| P5-T2 | 逐屏移植：index → dashboard → holdings（抽屉试算用 P3-T4 的 TS 库）→ analysis → chat（SSE 会话流）→ diagnosis → wechat（改为 PushProvider 配置管理，**gate：C4**） | 逐屏与原型对照功能对等、控制台无报错、窄屏断点通过、`data-od-id` 与免责区块保留 | 4–6d | 🔨 index 屏 ✅ 2026-10-29：真实数据接线——8 个 zustand store（portfolio/alerts/market/equity/advice/push_log/health/ui）+ 新读端点 `GET /api/advice` `GET /api/push_logs`（B13 关联项落地）+ 契约缺口修复（mark_alert_done 的 path 参数没进 OpenAPI，类型化客户端暴露）；冒烟：真实行情回写 recalc 逐位一致（290362.30）、指数条 4/5 有数（sh000832 无数据 → B14，前端按 — 防御）、时钟/空数组端点/SPA 全过；demo.ts 删除（mock 数据清零，格式化工具抽 src/lib/）。B13 已拍板 + 后端落地（2026-10-29：watchlist 表 schema v2 + `GET/POST/DELETE /api/watchlist` + §12 范围扩展——自选按无持仓形态判定、判定函数零改动，scheduler 9 项含自选 2 项）；B14 已解（腾讯 qt 指数 spot 源 + quote 逐标的补齐编排，datafetch 41 项测试 + 联网实测 sh000832 补齐）。dashboard 屏 ✅ 2026-10-30：净值 vs 沪深 300 = 手写 SVG 折线（`components/charts/*` 镜像 ZMC.line/spark/donut，十字准星 tooltip + 近一周/月/季切片；区间收益/回撤/跑赢跑输走 `src/domain` perf 纯函数；基准 = sh000300 日线按快照日 carry-forward 对齐后归一到区间起点，日线不可用整条不画不伪装，B11 口径图下如实标注）；钱在哪里 donut + 当日归因条（per 原值仅排序画条）+ 预警卡（逐条/全部归档，结果以 store 行态校验不谎报）+ 持仓表（30 日 spark = `/api/market/kline?with_today`，kline store 缓存键含窗口，60s 轮询不放大 sidecar 调用）+ **自选卡（B13 前端半边收口：展示行情单独 quote 合并且永不回写、卡内增删表单 upsert 幂等、ETF 涨跌停 % 显式可填、行业重叠标注）** + 收盘摘要（15:05 快照原值 + push_log 留痕，不现编文案）；顶栏手动刷新经 `ui.refreshTick` 强制重取屏幕级数据（kline/自选行情）；tsc + build + vitest 113 全绿；真实链路冒烟：两笔录入 + 5 日合成快照 → 基准窗口 `start=` 首日对齐 5/5、spark 日线 262 根、quote sz002415 ok（sina-hq）、refresh 回写后 day_chg +2.81% 与逐位手算一致。holdings 屏 ✅ 2026-10-30（**P4-T1 前端抽屉收口**）：汇总条/持仓表（排序·类型筛选·搜索·仓位条切换）/成交流水/仓位纪律四条全部读后端 recalc 原值（agg/per），纪律无端点故由 agg/per 供分子分母 + domain 阈值常量判定；**加减仓抽屉**（ADR-0006 手工逐笔 + ADR-0008 唯一试算例外）——方向分段/整手·涨跌停·可卖上限·现金余额即时校验 + 费用·摊薄成本·调整后仓位·已实现盈亏预览全部走 `src/domain`（applyTrade/validateQty/validatePrice/limitPrices/quickTiers，黄金向量同款），「我已确认」勾选才可提交，POST `/api/holdings/trade` 后以后端收据覆盖；候选池 = 持仓 + 自选（未持有买入即新建持仓携带 meta，B7 显式 mkt），拒绝体 limit_up/limit_down/decimals/cash_after 就地高亮；深链 `?add=1`/`?code=&side=&qty=` 直接开抽屉（分析页闭环出口），CSV 导出客户端生成，可用现金注资入口（POST /api/account，补 onboarding 缺口）；新增 trades store + portfolio store 的 recordTrade/setCash。tsc+build+vitest 116 全绿；真实链路冒烟：meta 新建 after_cost 1400.364（含费）·加仓摊薄 1405.3653·减仓 realized 4353.27 与客户端 applyTrade 逐位一致（费 36.4=佣金 35+过户 1.4），三类拒绝体附加字段全验（out_of_limit_range 1540/1260/2、insufficient_cash、need_meta）。analysis 屏 ✅ 2026-10-30：结论/四维/得分/目标止损/风险 = `GET /api/advice?code=` 的 AdviceRow 留档原值（ADR-0007 数字归规则、文字归模型；reason 空串如实标注「文案未回填」），**绝不伪造建议**（原型 adviceFor 观察态兜底 / RNG 蜡烛 / 样本历史全删）；K 线 = 真实 `/api/market/kline` 蜡烛（新 `components/charts/Candles` 手写 SVG——A股阳红 hollow/阴绿 solid + MA5/MA20，周K=5交易日聚合）+ 得分弧 `Gauge`（好坏轴红绿灯，B5 得分不是涨跌）；历史 = 同 code 多条留档（generated_at DESC，带 data_version 可回溯 B9，无「当时价格」列——留档不存该值不臆造）；无留档 → 诚实空态 + 生成路径（盘后复盘 / 问答 /建议）；执行计划 = 客户端骨架（分笔 + 失效条件引真实 stop/target + domain lotSize），「写入持仓试算」深链 /holdings 抽屉、「就这只问我」深链 /chat 闭环；advice store 加 byCode/loadForCode + latestByCode（rank）。tsc+build（95 模块）+vitest 116 全绿；真实链路冒烟：600519 两条留档 dims JSON 往返 Object[]（技术面 72）· risks 2 · target 1580-1720 · stop 1420 · position/reason/data_version 齐全，002415 无留档返 0 行走空态，kline sh600519 262 根真实 OHLC（末根 c1392.05）。chat 屏 ✅ 2026-10-30（功能对等）：线程 = 真实 Agent SSE（ChatPanel 流式文本 + 工具调用/确认 + skill）——比原型演示问答库（固定三层脚本 + 假 cites）更强，依据来源 = Agent 实调的行情/持仓工具（tool_call/tool_result 如实展示），不伪造引用；右栏三卡 = context（助理现在能看到什么，读 portfolio/alerts/market clock 原值：总资产/持仓数/现金/仓位/前三大/待处理预警/市场时段）+ limit（能力边界自陈静态四条）+ sessions（最近会话 = push_log 定时任务留痕，微信渠道待 C4 接入如实标注）；建议问题 chips / ?q= 深链（analysis「就这只问我」出口）只预填输入不自动发送（真实 LLM 交用户确认）；ChatPanel 加 prefill/clearNonce/persistKey 三可选 prop（会话持久化 localStorage『zimu:chat:me』上限 30 条，docs/02 约定）；agent_online 走 health store（替换 spike 局部 fetch）；「推送到微信」诚实提示 C4 待拍板。tsc+build（95 模块）+vitest 116 全绿（chat 屏不消费新端点，health/portfolio/alerts/market/push_log 均已冒烟、/api/chat SSE 为 P2-T3 已验证链路）。diagnosis 屏 ✅ 2026-10-30：KPI 与分区间表**只用黄金向量测过的 perf 七字段**（cum/bench_cum/ann_vol/mdd/bench_mdd/sharpe/win_rate）+ 后端 agg 原值（hhi/sectors）——**B11 未定稿项（beta/相关系数/基准侧波动）一律不臆造**，原型那几格删掉、基准波动标「口径待定」，分区间表删相关系数列（domain perf() 的 sharpe 已带 ε 守卫，顺带修掉 B11 记的原型 diagnosis sharpe 缺守卫）；组合 vs 沪深300 曲线 + 回撤水下曲线复用手写 LineChart，基准对齐抽 `lib/bench.alignBench`（dashboard/diagnosis 共用，消除重复）；行业集中度 = 后端 agg.sectors 堆叠条 + 权重条 + HHI 原值；再平衡清单「调整前后波动」走 `volSynth`（03 §8 黄金向量：σᵢ = 各标的近 30 日**真实日线**收益标准差、ρ 0.35/0.62），降仓数量走 `sellTo`（03 §6），纪律阈值取 domain 常量，清单为客户端模板骨架、数字全取后端 recalc 原值，「试算」深链 /holdings 抽屉（diagnosis→holdings 闭环）；空态不伪装 0=安全。tsc+build（98 模块）+vitest 116 全绿；diagnosis 不消费新端点（equity_snapshots/bench+持仓 kline/holdings-agg 均已随前几屏冒烟、perf/volSynth/sellTo 为黄金向量校验的 domain 纯函数、alignBench 与 dashboard 同源已验），故不起 live smoke。全局导航收尾 ✅ 2026-10-30：docs/02「全局导航与快捷键」表补齐——搜索回车命中持仓/自选 → `/analysis?code=`、否则 → `/chat?q=`（规格查持仓+自选，比原型只查持仓更完整；外壳兜底拉自选池）、数字键 1–6 切页（INPUT/TEXTAREA 守卫外补 SELECT——抽屉下拉操作时不误切页）、⌘K/Ctrl+K 聚焦搜索；/wechat 占位屏改如实标注「决策闸门 C4 待拍板」；spike 期过时注释清理（chat.css / health store / ChatPanel 头注）。里程碑全栈验证：cargo test --workspace 18 组 0 failed + datafetch 41 + frontend 116 + build 全绿。余 wechat（gate C4 待拍板）——**T2 除 C4 闸门外全部移植完成** |
| P5-T3 | `design/` 冻结：README 加提示指向 `frontend/`，原型保留不删 | — | 0.1d | ✅ 2026-10-30：`design/README.md` 顶部加冻结横幅——明确本目录为「设计参照与验收基准、不是活实现」，指向 `frontend/`（index/dashboard/holdings/analysis/chat/diagnosis 六屏已接真实数据，wechat 待 C4 拍板后补、原型暂留该屏作参照）；原型只保留不再新增功能，界面/规格改动一律走 `frontend/` 与 `docs/`，演示样本数字以 `frontend/` 真实实现为准；design/ 七个 html 与 assets/ 原样保留（像素/交互对照基准，不删） |

**完成标准**：与原型功能对等；`design/` 冻结为参照。

## P6 · MVP 垂直切片

| # | 任务 | 产出/验收 | 估算 | 状态 |
|---|---|---|---|---|
| P6-T1 | QQ 适配器接入（`robit-qq`，出站 WS）：双向会话 + 推送 | 群里问「某只要不要止损」得到带口径的回答 | 0.5–1d | 🔶 子木侧代码面由 **P7-T2 提前承担**（ADR-0011：`ChatbotManager<QqPlatformAdapter>` 已挂进 zimu-server，缺省停用；真机核心链路走查通过 2026-11-08）；微信通道仍等 C4 与 robit 平台模块 |
| P6-T2 | 微信渠道落地（**gate：C4**，含公网回调/内网穿透评估，若需入站先补安全 ADR） | 同上 | 1–2d | ⏸ 等 C4 |
| P6-T3 | 部署与运维：Windows 服务/计划任务、日志轮转、SQLite 备份策略、密钥管理（`.env`，永不入库） | 无人值守运行一周 | 0.5d | ⬜ |
| P6-T4 | 端到端实跑：录入 → 每日行情 → 盘后复盘与建议 → 条件预警 → 会话/推送 | **连续 5 个交易日**跑通；推送里每个数字可回溯 P3 公式；每条建议带时间与数据版本留档 | 5 个交易日 | ⬜ |

**完成标准**：即 P6-T4 验收列。到这里 MVP 成立，之后的迭代按 backlog 与新需求排——P7 是承接这些迭代的登记位（不排死顺序，准入条件 = MVP 成立）。

## P7 · MVP 后迭代 🔨（2026-11-05 设立）

| # | 任务 | 产出/验收 | 估算 | 状态 |
|---|---|---|---|---|
| P7-T1 | 模型思考过程展示链路（`07` R7；需求文档已移交 robit，唯一真值 `robit/docs/robit-rfc-thinking-output.md`）：robit 三层实施（robit-ai 捕获 `reasoning_content` → `AgentEvent::ThinkingDelta` → robit-web `thinking_delta` wire，顺带收口 R6 空白帧）+ 子木消费（ChatPanel 加 case、docs/02 补展示口径段） | 思考增量在 chat 屏可见（形态 2026-11-06 已拍板：对齐 robit-gui 三态折叠）；robit 侧验收见 RFC §3；子木侧 tsc/build/test 全绿 + 真实 qwen profile 人工走查 | robit 侧另评（实现复杂度较高）；子木侧 0.5d | 🔨 两侧代码均已落地，余真实网关人工走查。**robit 侧 ✅ 2026-11-05 发版 v0.1.28**（`feat/thinking-output` 已合并；实施记录 `robit/docs/robit-design-thinking-output.md`：双路径捕获（模型级 `thinking = auto\|on\|off`）+ `thinking_delta` wire + R6 发射端收口 + robit-gui 三态 ThinkingPanel 参考实现）。**子木侧 ✅ 2026-11-07 消费**：backend 升 0.1.28（rusqlite 0.40→0.41 对齐、zimu-agent-cli 穷举 match 补臂——ANSI 暗体渲染思考流）+ openapi SSE 描述补帧再生成 + ChatPanel 思考段三态折叠（live 流式灰字 → 正文/工具事件折叠「▸ 思考过程」→ 点击回看；不持久化）+ `02` 展示口径段；workspace 测试 / tsc / build / vitest 121 例全绿。**余**：Web chat 屏 💭 三态折叠目视一遍（**模型侧出帧已随 2026-11-08 QQ 真机走查顺带实锤**——日志大量 `ThinkingDelta`，前置 `thinking = "on"` 生效）；不阻塞 P6/MVP |
| P7-T2 | 智能体接入模块重设计（ADR-0011，2026-11-07 立项）：装配线抽取（`zimu_agent::assembly::AgentCore`）+ Web 会话真值上服务端（`chat_messages` v4：先落库再注入、`with_history` 复活、history/reset 端点、前端水合、localStorage 降级缓存）+ QQ 验证通道（`ChatbotManager<BoundOnly<QqPlatformAdapter>>` 挂 zimu-server：`[zimu.channels]` 开关缺省停用、绑定白名单 + `/api/channel_bindings`、调度器 PushSink 预警/盘后摘要推送）+ 子木主人格提示词入库（`prompts/agent.md`）。微信代码不做（等 robit 平台模块，届时只换泛型参数）；robit 缺口移交 R8 清单 | 全离线门禁绿（含档案状态机/复活/分页、白名单 decorator、推送恰一次、契约 35 样例）；QQ 真机走查七项（见计划验证节）；人格「你是谁」问答以子木身份+边界回应 | 1.5–2d | 🔨 2026-11-07 子木侧 T1–T8 全量落地（分支 `feat/agent-channels`，PR1–PR4 四笔），**QQ 真机核心链路走查通过 2026-11-08**（启用开关→连接 `qq_online=true`→未绑定消息静默丢弃+留案底→绑定后应答恢复→人格加载链路存活、思考帧出帧正常但 QQ 投影按设计不渲染）。**余单项**（不阻塞，择日补验）：15:05 盘后摘要推送 + `push_log channel='qq'`（需交易日在场）、Ctrl+C 优雅关闭 + 二次启动档案在、网关 ~30min 轮换不断会话、Web 端 💭 三态折叠目视（P7-T1 余项同源）；wechat 开关词汇就位待模块 |
| P7-T3 | agent 写账本工具面（ADR-0012/0013，2026-11-08 立项）：`record_trade`（成交录入——与界面录入同一 store 事务，note 自动追加来源标记）/ `adjust_cash`（现金绝对值校正）/ `get_trades`（流水只读，缺省 20 上限 50）/ `upsert_holding` · `remove_holding`（持仓快照校正，ADR-0013：初始化映射/定期校准/场外变动——整行覆盖先读后改、删除对话内两步确认、holding_edits 留痕）——**自动落账 + 读回**（逐笔确认被否决，机制健在可一行翻回、启用前置见 `07` C13），写账本三纪律 = 数字来自用户明示 / 先试算后落账并宣读收据 / 流水只增不改错账反向冲正；人格边界段整段重写（快照账本准则）+ docs/03 术语注 + CLAUDE.md/README/mod.rs 头口径同步 | 离线门禁全绿（tools 9 新用例：落账带标记 / 拒绝路径零副作用 / 现金 before-after-delta / 流水 limit / 快照校正留痕·三不动·幂等 / 坏输入零副作用 / 删行流水保留 / 免确认拍板钉死×2；persona 守护断言加「只增不改」「record_trade」「整行覆盖」「remove_holding」）；三端人工走查：试算→落账→宣读收据链路、缺项先问不补数、冲正流程、现金前后两数、初始化搬仓与校准流程、删除两步确认 | 0.5d | 🔨 2026-11-08 代码+测试+文档全落地（分支 `feat/agent-ledger-tools`），**余三端人工走查**（真实网关；⚠️ 走查务必用 `zimu.db` 副本——本轮工具真写库） |

## 之后再说（明确不在近期计划里）

多账户、港股美股、场外基金、自动下单、真实协方差估计、模型效果评估体系、策略回测。（桌面壳**不是目标**：子木的发布形态是常驻 Web 服务 + 微信/QQ 会话入口，见 ADR-0005 第 2 条；这与 robit 的桌面端定位是有意区分。）
