# 07 · 已知问题与待决策

分五级：**阻断**（现在就是坏的）、**一致性**（数字/口径互相打架）、**待决策**（需要人拍板）、**框架反馈**（robit 的重构需求，见 ADR-0005「与 robit 的协作模式」）、**已修复**（存档，保留验证方式）。`06-roadmap.md` 的 P1 就是清掉前两级。

## 阻断

暂无——A2「首笔持仓没有入口」已随 P1-T1 解决（2026-10-28，存档见「已修复」）。

## 一致性

### B10 · 演示样本含已退市/摘牌标的 ✅ 已关闭（2026-10-30，样本替换不再执行）

关闭理由（用户拍板）：① `design/` 已随 P5-T3 冻结——演示样本不再替换、不再演进，退市标的留在原型里作为历史参照无害；② 生产侧（`frontend/` + 后端）不使用任何演示样本，六屏全部真实数据接线；③ 本条的核心风险「接真实行情后退市标的报价为空/定格」已由 sidecar `/quote` 四态（`ok / halted_or_delisted / empty_or_delisted / no_data`）+ 逐标的补齐（B14）+ 前端「status=ok 且 price>0 才显示」防御落地，110074（定格价）/ 127022（空行）/ sh000832（未收录）作为回归样本进 datafetch 单测。原始记录如下。

P2-T1 spike（2026-10-26）实测：`110074 精达转债` 新浪行情定格（open=0、volume=0、价格停在 222.81，已退市强赎）；`127022 恒逸转债` 新浪返回空串（已摘牌）；`113650 博威转债` 待核实。股票/ETF 抽查存活（600519/510300 有正常行情）。影响：接真实行情后这两只的报价为空/定格，`alerts` 里「强赎公告 11-20」等文案与现实脱节；`holdings` 样本还会流入净值与建议的演示数字。修法：P4 数据管道落地时把转债样本换成存活标的（从 `bond_zh_hs_cov_spot` / `bond_cb_redeem_jsl` 现存活名单里选），管道加「标的存活性校验」步骤，`design/` 与未来 seed 同步替换。

进展（P4-T3）：**存活性校验的 sidecar 侧已落地**——`/quote` 输出 `ok / halted_or_delisted / empty_or_delisted / no_data` 四态，110074/127022 作为联网冒烟的回归基准复验通过；且活券转债布局补测（与股票同为 33/34 位）。样本替换本身（`design/` 与 seed）仍未做。

### B11 · 基准侧风险指标口径未定 ✅ 已定稿（2026-10-30，用户拍板）

原缺口（P3-T1 fixture 设计时发现，2026-10-27）：① `data.js:177` 的 `benchVol` 用基准收益的 RMS（不减均值），`diagnosis.html:155` 的 `bvol` 用标准差（减均值）——同名指标两处算法不同；② `diagnosis.html:158` 的夏普没有 `1e-9` 守卫，sd=0 时除零，与 `03` §7 公式不一致；③ 诊断页的 beta、与基准相关系数、超额收益未收入 `03` §7。

**定稿（按既定顺序：先 03 + fixtures，再两边实现）**：`03` §7 增「基准侧指标」——`benchVol` 统一为总体标准差（RMS 变体废除）；`beta = cov/var_rb'`、`corr = cov/(sd_r'×sd_rb')` 按配对前缀 m = min(n, nb) 计算，**sd ≤ 1e−9 ε 守卫是口径的一部分**（恒定收益序列在 f64 下带 ~1e-18 噪声，纯零判定会放出 beta ≈ 4×10¹⁴ 的天文数字——独立脚本推导 fixture 时实测踩中，pf-downtrend 锚点即此场景）；`区间超额` = cum − benchCum 算术差（展示层相减，不进 perf 输出）。`perf.json` 4 例补 bench_vol/beta/corr 期望 + 新增 `pf-beta-corr` 非退化锚点（期望值由 §7 公式独立推导，脚本先复现既有 7 字段手算锚点全对齐再产出）；Rust `perf.rs` 与 TS `perf.ts` 同步实现（两侧黄金向量全绿，Rust 29 单测含空 bench/恒定 bench 守卫 2 例）；前端 diagnosis 屏补「与基准相关系数 + Beta」KPI 格、年化波动子行显基准值、分区间表恢复相关系数列（此前按未定稿纪律摘除）。原型 `design/` 两处旧实现随 P5-T3 冻结作废（不再统一，冻结优先）。

### B12 · 奇零股卖出规则与 §1 冲突 ✅ 已定稿（2026-10-30，用户拍板）

原缺口：`03` §1 表格写股票「可卖数量 = 整数股」（A 股卖出允许奇零股、须一次性卖出），但原型买卖一律按 `qty % lot === 0` 拦截（`holdings.html:342`），且「全部清仓」按钮对奇零股持仓（如送股后 150 股）填入 `held` 后会被自己的校验拒绝。存储侧 P4-T2 已就绪（`holdings.shares` 普通整数，奇零股存得下）。

**定稿（按既定顺序：先 03 + fixtures，再两边实现）**：`03` §1 数量校验改为五步判定序——①`qty < 0` 拒 ②`qty == 0` 未填写合法 ③**卖出且 `qty == held` 清仓豁免整手校验**（A 股奇零股须一次性卖出：150 股可整笔清、不可拆 50+100 分笔卖）④`qty % lot ≠ 0` 拒（买入恒整手；150 股里卖 100 合法、剩 50 奇零下次一次性清）⑤卖出超持仓拒。`lot.json`：`vq-sell-odd-lot` 翻为合法 + 新增部分奇零拒/留奇零合法/超持仓判定顺序/转债奇零清仓 4 例（16→20 例）。Rust `lot.rs` 与 TS `lot.ts` 同步实现（两侧黄金向量全绿，Rust 单测 +`odd_lot_full_sell_is_exempt`）；前端抽屉 lot-hint 对奇零持仓补「全部清仓可一次性卖出」提示，「全部清仓」快捷档对奇零持仓从此可用。原型旧拦截行为随 P5-T3 冻结作废。

### B13 · 自选（watchlist）预警无生产侧载体 ✅ 已解（2026-10-29 后端落地，10-30 前端收口）

原缺口：P4-T7 预警引擎只覆盖 `holdings` 持仓，原型 dashboard 的 `watchlist-card` 在生产侧无存储载体。**用户拍板：建 watchlist 表（schema v2 迁移）+ `03` §12 判定范围扩到自选。** 落地（当日）：§12 正文修订——自选按「无持仓形态」判定（cost/stop/target_hi 不喂 → 序 3/4/5 不参与；序 1/2/6/7/8 同款；判定函数**零改动**，既有 fixture `al-limit-up` 等本就是无持仓形态锚点）；`zimu-store` v2 迁移（watchlist 表 + upsert/remove/list，含 v1→v2 合成老库迁移测试）；调度器扩展（自选行情单独合并拉取**不回写库**、与持仓重叠代码只判持仓形态一次、cb_list 拉取条件扩为持仓或自选含转债，scheduler 测试 7→9 项）；API `GET/POST/DELETE /api/watchlist`（契约 23 路径/42 schema/25 样例）。关联项 `push_log` 读端点已随 index 屏落地（`GET /api/push_logs`）。**前端半边已收口（2026-10-30，随 P5-T2 dashboard 屏）**：自选卡消费 `GET /api/watchlist` + 展示行情单独 quote 合并（`store/watchlist.ts`，永不回写库）；维护入口 = 卡内增删表单（upsert 幂等提示、ETF 涨跌停 % 显式可填——03 §2 数据层配置语义、与持仓行业重叠标注）。B13 全链闭合。

### B14 · 中证转债指数（sh000832）spot 行情无数据 ✅ 已解（2026-10-29）

P5-T2 index 屏冒烟发现：sina spot 不 carry 中证转债指数（回空名零价行，被分类成 `halted_or_delisted` 假状态）。用户拍板「给 sidecar 补指数 spot 源」。落地：探测确认**腾讯 `qt.gtimg.cn` 全量覆盖**（sh000832 价格 452.63 与新浪 sh000001 交叉验证逐位一致）→ datafetch 三处改动——① `sources.py` 增腾讯解析器（14 位时间戳锚点相对取位，量单位手→股/份 ×100、指数原样，与东财约定对齐）+ 新浪解析器「空名零价 = 未收录，不出行」（不再伪装 halted）；② `service.py` quote 编排改**逐标的补齐**（未覆盖标的自动落下一源，502 语义收窄为无任何源成功）；③ 链序 sina → tencent → em。41 项离线测试 + 联网实测（000832 由腾讯补齐，degraded 留痕诚实）。前端行情条「status=ok 且 price>0 才显示」防御保留（04 附录 B）。

### B15 · 空白账户「录入第一笔」死路 ✅ 已修复（2026-11-01，用户实测发现）

**现象**：真实空白账户（holdings 0 / watchlist 0 / cash 0）点「录入第一笔」无法添加——右上角主按钮 toast「没有可用候选标的」不开抽屉；空态引导与 index CTA 走 `/holdings?add=1` 深链时 effect 因 `candidates.length === 0` 直接 return，**抽屉不开且无任何提示**（静默失败）。**根因**：抽屉候选池只有「持仓 + 自选未持有」两个来源，原型设想的「候选来自行情搜索」（chat.html 文案）从未建设，ADR-0006 手工逐笔先行却没有手工输入新标的的路径；另有既存小 bug——`deepLinked` ref 消费后不复位，同一次进屏内 CTA 第二次点击不再开抽屉。**修复**（纯前端，后端 `POST /api/holdings/trade` 的 meta 新建持仓路径零改动）：① TradeDrawer 候选下拉追加「＋ 新标的（手工输入）」——代码/市场/名称/类型/行业/涨跌停 % 与仪表盘自选表单同口径（B7 mkt 显式不猜、`symbolOf` 校验），合法即合成未持有候选（price/prev null → 价格手填、跳过区间拦截，后端仍是最终裁判），提交走既有 isNew meta 路径；② 深链闸门从「候选池非空」改为「组合快照已加载」，池空以 code=null 开抽屉落手工兜底；`closeDrawer` 复位 deepLinked；③ 主按钮永远开抽屉；④ 买入试算 insufficient_cash 时就地提示先经「可用现金 · 调整」设入资金，空态文案同步。**行情搜索候选**（输入代码拉实时行情预填名称/价格/昨收）记为后续增强，不阻断本次闭环。

## 待决策

| # | 问题 | 备注 |
|---|---|---|
| C1 | 后端技术栈 | ✅ 已决（2026-10-25，ADR-0005）：Rust + robit agent 基建，独立仓库 `backend/` workspace，axum API |
| C2 | 前端是否框架化、何时 | ✅ 已决（ADR-0005）：React + TS + Vite（`frontend/`）；`design/` 原型保留为参照直至迁移完成 |
| C3 | 真实行情源 | ✅ 已决（ADR-0005）：akshare 主 + baostock 兜底，经 Python sidecar（`datafetch/`）HTTP 桥接；P2 spike 验证字段、频率与限频 |
| C4 | 微信接入方式 | 🔶 部分已决（ADR-0005）：架构走 robit-chatbot `PlatformAdapter` 可插拔，先落 console/文件适配器；**微信渠道仍未定**，调研必须同时回答：① 能否**双向会话**（产品要求微信/QQ 会话入口，不只是推送）② 是否需要**公网入站**（企业微信自建应用回调需公网地址或内网穿透；群机器人/Server酱只发不收；测试号不长期稳定）。QQ 侧不受此困——`robit-qq` 出站 WS 网关现成，可作第一个真实适配器。P5 前拍板 |
| C5 | 第一笔持仓怎么录入 | ✅ 已决（2026-10-28，ADR-0006）：手工逐笔先行——复用 holdings 抽屉表单，校验口径已在 `zimu-domain` 实现；券商对账单 CSV 列为 P4 后续增强单独立项（敏感文件只本地解析、永不入库）；截图识别不做。**关闭（2026-10-28，P4-T1）**：后端录入流程落地（`zimu-store` 事务 + `/api/holdings/trade` 等端点，校验全部复用 zimu-domain），16 项集成测试 + e2e 走查通过；前端接线随 P5-T2 |
| C6 | 仓位纪律阈值是否随账户规模浮动 | 25% / 35% / 10% / 12% 目前写死；小账户的单票上限是否该更宽 |
| C7 | 波动合成的相关系数是否用估计值 | 现在是 0.35 / 0.62 假设值；估计需要更长样本与行业分类稳定性 |
| C8 | 分析结论由什么生成 | ✅ 已决（2026-10-28，ADR-0007）：混合——数字归规则（`score`/`dims`/`conf` 与目标价/止损/失效条件由领域规则函数产出，口径进 `03` 与黄金向量），文字归模型（agent 调领域工具取数，模型只写结论/理由/风险自陈，文案中数字必须来自工具返回） |
| C9 | 数据存储位置 | ✅ 已决（ADR-0005）：SQLite（rusqlite bundled），单文件、gitignored，备份 = 拷文件；持仓仍是敏感数据，永不入库（git 意义上的库） |
| C10 | 许可与版权 | `LICENSE` 是 Apache-2.0 正文，缺少先常见的版权声明附录段落；要不要补 `NOTICE` |
| C11 | `design/README.md` 与 `docs/` 收敛 | ✅ 已决（2026-10-28，P1-T8）：README 只保留原型定位、打开方式、指向 `docs/` 的导引表与许可；业务规则/数据契约/设计约定/本地状态/已知边界全部删除，规格单一来源 = `docs/` |

## 框架反馈（R 系列）

子木消费 robit 的 trait/API 时发现的形状痛点记录在这里，作为 robit 重构的需求来源（协作原则见 ADR-0005：框架级缺口先改 robit，不在子木侧写绕过 trait 的补丁）。每条落定时同步到 robit 仓库自己的 issue/roadmap。

| # | 痛点 / 需求 | 状态 |
|---|---|---|
| R1 | **Web/SSE 桥缺官方实现**（P2-T3）：robit 的 `Frontend` trait 只有 CLI/TUI 形态的参考实现；Web 宿主需要自己写 AgentEvent→wire 类型投影、确认槽位（`request_tool_confirmation` 挂起等 HTTP 回填）、broadcast 多客户端扇出。子木的实现见 `backend/crates/zimu-server/src/agent_bridge.rs`——确认语义与事件投影都是宿主无关的重复劳动。诉求：robit 提供可选 web 桥 crate（SSE/WS 的 Frontend 通用实现 + 消息注入约定），或至少给 `AgentEvent` 一个官方序列化 schema | ✅ 2026-10-27 落地（P2-T5）+ **2026-10-30 全链收口**：robit v0.1.26 新增 `robit-web` crate——`WebEvent` 官方 JSON schema、`WebFrontend`（broadcast 扇出 + 确认槽位）、`WebBridge`（消息注入）；子木 zimu-server 消费、宿主手写桥删除。收口：`feat/web-bridge` 已合并推送 robit `main`（fast-forward）、tag `v0.1.26` 已推送、crates.io 已发布（`publish.ps1` 六 crate 按依赖序）、子木 `backend/Cargo.toml` 已从 git pin 切换为 registry 写法（`robit-ai/robit-agent/robit-web = "0.1.26"`），`cargo build` + 全量测试（18 组 0 failed）在 registry 源下全绿，Cargo.lock source = `registry+crates.io-index`。R1 闭环 |
| R2 | **ToolRegistry 装配后不可扩展**（P2-T2）：`bootstrap()` / `create_tools_from_config()` 返回不可变 `Arc<ToolRegistry>`，领域工具无法追加进内置工具面；只能绕过 bootstrap 手动 `ToolRegistry::new()` + `register()`（代价：丢掉 read/bash 等通用工具）。诉求：`bootstrap_with(extra_tools)` 或让 registry 支持装配后注册 | 🔶 robit 0.1.27 已落地（`bootstrap_with` + `BootstrapOptions`：extra_tools 追加 / standard_tools 白名单 / extra_skill_dirs；领域工具名自动并入 `tools_required` 校验清单）。**子木有意不消费**：0.1.27 白名单语义下系统工具（read/load_skill/search_history/query_task）始终注册，而 server 工作目录可能存在 `.env`（密钥）——`read` 暴露给 LLM 是安全面；子木工具面维持纯领域装配（`build_registry`，决策注释在 agent_bridge.rs）。Skill 触发不依赖 load_skill 工具（触发词注入是 robit-agent 内部机制）。若未来要系统工具能力再评估 |
| R3 | **init_logging 独占全局 tracing subscriber**（P2-T3）：宿主服务自带日志初始化时二者只能选其一——zimu-server 现按「有 robit 配置就交给 init_logging（靠 additional_directives 补 tower_http），否则回落自建 fmt」分支处理，且失去对日志格式的自主权。诉求：提供 tracing `Layer` 形态，由宿主组合进自己的 registry。**P4-T7 又踩一形态**：build_filter 只放行 target_crate 与 robit 系 target，宿主 workspace 其余 crate（zimu_agent 调度器日志）落在默认底线被静默吞掉——调度器在跑但日志不可见，排查成本高；现靠 additional_directives 显式放行（`zimu_agent={level}` `zimu_market={level}`），诉求同上（Layer 化后宿主自管 target） | ✅ robit 0.1.27 落地 + 子木已切换（2026-10-31）：`build_log_filter(app_config, host_targets, additional_directives)` + `log_file_layer(...) -> Option<Box<dyn Layer>>` + `install_panic_hook()` 公开；zimu-server main.rs 与 zimu-agent-cli 删「二选一」分支改单一初始化路径，host_targets 声明 `zimu_server/zimu_agent/zimu_market`（CLI 侧顺带修正旧写法 bug：原来只传 "zimu_agent"，CLI 自身 crate 反而没被放行）。无 robit 配置时 app_config=None 自然退化（filter 默认 info、文件层 None），服务照常起 |
| R4 | **skill 搜索目录写死**（2026-10-31 整理需求时新记）：`load_all_skills` 只认 `~/.robit/skills` 与 `<working_dir>/.robit/skills`；技能作为产品资产随仓库版本化的宿主（子木 `skills/` 入库评审）没有官方入口，只能借道 `load_skills(global, project)` 或自写 loader。诉求：额外技能目录参数/配置 | ✅ robit 0.1.27 落地 + 子木已切换（2026-10-31）：`load_all_skills_with(working_dir, extra_skill_dirs)` / `load_skills_from_dirs(dirs)`（后目录覆盖同名，坏文件非致命逐条上报）；`bootstrap_with` 经 extra_skill_dirs 直通。子木 `load_zimu_skills()` 内部切到 `load_skills_from_dirs`（保留 ZIMU_SKILLS_DIR/manifest 定位的目录策略包装），单源语义不变（不叠全局目录） |
| R5 | **ToolResult 与 anyhow 互操作**（2026-10-31 整理需求时新记）：宿主工具 execute 里不能直接 `?` 返回 anyhow 错误（`AgentError` 是 robit 的类型，宿主跨 crate impl From 违反孤儿规则），子木被迫写 `store_try!` 转换宏。诉求：robit 侧提供官方 From 或转换入口 | 🔶 robit 0.1.27 已落地（`AgentError::External(#[from] anyhow::Error)` 解锁 execute 内 `?`；`From<anyhow::Error> for ToolResult` 含 `{:#}` cause 链）。**子木有意不用**：官方 From 会把整条 cause 链（可能含 SQL 原文/路径）喂给模型，子木存储错误策略是「细节进 tracing 日志、模型只见脱敏固定话术」——正是 robit R5 文档认可的第二种推荐形态（sanitize what the model sees）；`store_try!` 宏保留为脱敏话术载体（决策注释在 tools/mod.rs） |
| R6 | **robit-web `text_delta` 会发空白内容帧**（2026-11-02，qwen profile 实测）：模型流每段的角色帧/思考模式空内容帧被原样投影成 `text_delta`（`text` 为空或纯空白），宿主若「见帧即建流式段」就得到空白气泡 + 常驻光标。诉求：wire 层过滤空白帧，或在 `WebEvent` 文档里写明「宿主必须容忍空白 text_delta」。子木侧已做渲染层守卫（ChatPanel：空白帧不建段、turn_complete 收口全部未闭段并丢弃整段空白），wire 本身未动 | ✅ robit 0.1.28 收口（2026-11-05 发版，随 R7 同批）——**发射端**过滤：robit-agent 空串 content 不再发 TextDelta，一处修改全前端（TUI/GUI/chatbot/web）一次性生效；wire 投影层保持直通不加二次过滤。子木 ChatPanel 守卫保留为惰性兼容（行为不变）。R6 了断 |
| R7 | **模型思考过程（reasoning 增量）在 wire 上没有通道**（2026-11-05，用户问「思考过程怎么没显示」排查定位）：思考型模型（qwen DashScope 兼容面的 `delta.reasoning_content`，DeepSeek-R1/Kimi 同字段惯例）的增量文本在**第一站就被丢弃**——robit-ai 依赖的 async-openai 0.42（backend/Cargo.lock 锁 0.42.2）流式 delta 结构只有 `content/tool_calls/role/refusal` 等字段，既无 `reasoning_content` 也无 `#[serde(flatten)]` 兜底，serde 静默忽略未知字段；robit-agent `AgentEvent` 与 robit-web `WebEvent` 同样没有 thinking 变体，前端无从展示。R6 观察到的「空白 text_delta」正是携带思考文本的那批帧被剥空后的空壳——内容已在半程，缺的就是这个字段。诉求：robit-ai 捕获 → robit-agent 加 `AgentEvent::ThinkingDelta` → robit-web 加 `thinking_delta` wire 事件的三层联动（含请求侧 thinking 开关、R6 顺带收口、验收标准见需求文档） | ✅ robit 0.1.28 落地（2026-11-05 发版，`feat/thinking-output`；需求真值 `robit/docs/robit-rfc-thinking-output.md`，实施记录 `robit/docs/robit-design-thinking-output.md`）：robit-ai 双路径捕获 `reasoning_content`（模型级 `thinking = auto\|on\|off`，on/off 走自建 SSE 旁路注入 `enable_thinking`；auto 默认路径与旧版字节级一致）、`AgentEvent::ThinkingDelta`（不写回 history）、robit-web `thinking_delta` wire 帧（additive）、R6 顺带收口；渲染启用面仅 robit-gui（三态折叠 ThinkingPanel 参考实现）。**子木已消费（2026-11-07，P7-T1）**：backend 升 0.1.28（连带 rusqlite 0.40→0.41 对齐）+ zimu-agent-cli 补臂（ANSI 暗体渲染）+ ChatPanel 思考段三态折叠（展示形态 2026-11-06 拍板对齐 robit-gui；口径见 `02` chat 段）。余真实 qwen profile 人工走查（需 `~/.robit/config.toml` 模型级 `thinking = "on"`）——见 `06` P7-T1。R7 闭环 |

## 已修复（存档）

### B6 · 演示数字靠正则改写中文句子维持一致 —— 2026-10-28 修复（P4-T5 原型正式解）

**原现象**：`data.js` 用 `replace(/当前 [\d.]+%/g, …)` 把写死在建议文案里的百分比覆盖成计算值，但只匹配「当前 X%」「占总资产 X%」两种句式——「不建议在 28% 集中度上继续加仓」「单笔浮亏已达 -4.04%」「价格 105.350」等句子不受保护，行情跳动后文案里的数字陈旧，与表格/图表打架。

**修复**：建议文案的持仓派生数字改**模板占位符**——`{w}` 仓位% / `{pnl}` 浮动盈亏% / `{price}` 现价 / `{drag}` 正股再跌 10% 对总资产的拖累%，init 时经 `fill()` 统一插值，数字只取 `recalc()` 快照；正则改写块整体删除。纪律阈值（25% 上限）与目标价/止损是建议参数不是派生数字，保持字面量。`04` 适配器要求第 8 条同步；生产口径按 ADR-0007——模型写文案、文案中数字必须来自工具返回。**生产侧已随 P4-T6 接线（2026-10-29）**：`calc_advice` 规则函数出数并先写 `advice_history` 数字列，模型文案经 `finalize_advice` 只回填 reason/risks——数字结构性地不经过模型转写；`generate-advice` Skill 模板内嵌「文案数字必须原样取自工具返回」硬规则。

**验证**：`.file-versions/check-p4t5.js` vm 沙箱 22 项断言——10 条建议 position 仓位 == 计算值、无占位符残留、600519 reason/risks、002594 浮亏/拖累、127022 现价（3 位小数）全部出自快照、7 页内联脚本编译通过。

### B9 · 历史建议与 `advice` 无关 —— 2026-10-28 原型级修复（P1-T7；正式解归 P4-T2）

**原现象**：`analysis.html` 的 `buildHistory()` 用页面内生成的样本，没有数据层的建议留档结构，卡片副标题却写「每次数据更新后重算，可回看当时怎么说的」——假装是真实留档。

**修复（原型级）**：卡片显式标注「演示样本 · 非真实留档」（head 加「样本」pill、副标题说明正式版需建议留档、P4 落地）；样本字段对齐未来 `adviceHistory` 契约——每行带 `generatedAt`（生成时间）与 `dataVersion`（输入数据版本，演示期恒为 `demo-sample-v0`），表格「日期」列改「生成时间」并在行内展示版本号；`04` 第 6 条同步。**正式解**（真实留档、来源与文案重做）归 P4-T2 的 `advice_history` 表，届时换 `buildHistory` 数据源并删样本标注。**写入侧已随 P4-T6 落地（2026-10-29）**：`calc_advice` 工具写数字列（`generated_at` + `data_version=adv-v1|k=…|q=…`），`finalize_advice` 回填文案列；前端读留档的 REST 端点随 P5 analysis 屏接线。

**验证**：vm 沙箱内容断言——样本每行带 `generatedAt / dataVersion`、无 `${x.date}` 残留、卡片带样本标注；7 页内联脚本编译通过。

### B5 · `up / down` 类被复用于非涨跌语义 —— 2026-10-28 修复（P1-T4）

**原现象**：`holdings.html` 用 `up` 表示「已触及纪律上限」、`up/down` 表示买卖流水；`analysis.html` 用它们表示维度得分高低、建议排序与结论标签；`charts.js` gauge 用它们表示分数；`diagnosis.html` 用它们表示优先级与风险指标好坏；`wechat.html` 用它们表示通道正常/已绑定；`index.html` 用它们表示待决事项「限期/减仓」——在红涨绿跌的产品里，「红色 = 危险」和「红色 = 上涨」同屏出现且互相冲突。

**修复**：三类语义分离（`03` §11 定稿、`05` 令牌表与三条规则更新）——① **好坏轴**用 `.ok/.warn/.danger` 类与同名 pill（新增 `--ok-soft/--danger-soft` 浅底令牌），替换纪律「已触及/安全」、维度得分条、得分弧（≥70 ok / 55–69 warn / <55 danger 红绿灯）、风险指标对比基准、通道状态、反馈标记、资金不足提示、优先级（p1 = danger）；② **交易方向**用 `.buy/.sell`（新增 `--buy/--sell` = `--up/--down` 别名令牌，A 股红买绿卖视觉不变、类名解耦），替换流水买卖、历史建议动作、结论标签、待决事项「减仓」；③ `up/down` 退回涨跌数字/标记与预警 `level` 词汇。顺带：删除 inviting 复用的 `.tl-item.up/.down` 死样式；纪律安全态进度条由 `--accent` 改 `--fg-2`（accent 只用于交互态，05 规则 2）；`.btn-danger:hover` 底色改用 `--danger-soft`。frontend `tokens.css / ui.css / HomeScreen.tsx` 镜像同步（兑现 P2-T4 注释里「届时两边一起改」的约定）。

**验证**：vm 沙箱断言两份 ui.css/tokens.css 含新令牌与类、7 页 + charts.js 无遗留复用点（dashboard 超额收益/归因与预警 `level` 属合法涨跌语义，保留）；`npm run build` 通过。视觉变化（有意为之）：得分弧/维度条/排序 pill 由「高分红」转红绿灯（高分绿）、中档由 accent 蓝转 warn 黄；微信「正常/已绑定」由红转绿；纪律安全条由蓝转灰。

### B8 · 死代码：现金规则 ternary 两分支相同 —— 2026-10-28 修复（P1-T6）

**原现象**：`holdings.html` 仓位纪律的 `r.inverse ? c / r.v * 100 : c / r.v * 100` 两个分支完全相同，「现金最低保留」的进度条按「越大越好」画（现金充裕时满格），与另外三条上限类规则（满格 = 触及上限）的视觉语义相反；行标签也不分方向一律写「上限」。

**修复**：inverse 规则的进度改为 `min(100, v / c × 100)`——现金降到下限时满格，与另三条「满格 = 触及」语义一致（`c = 0` 时 `Infinity` 由 `min` 截为 100，无 NaN；压线 `c = v` 满格但 `hit = c < v` 为假，沿用纪律口径「压线算安全」）；行标签按 `inverse` 显示「下限 10%」/「上限 N%」。

**验证**：vm 沙箱断言公式 `c=20 → 50%`、`c=10 → 100%（压线安全）`、`c=8 → 截断 100（hit）`、`c=0 → 100 无 NaN`；7 页内联脚本编译通过；人工：四条进度条方向语义一致。

### B7 · 交易所靠代码首字母猜 —— 2026-10-28 修复（P1-T5）

**原现象**：`analysis.html` 用 `code[0] === '6' ? 'SH' : 'SZ'` 展示交易所——`588000`（科创板 ETF，SH）与 `002415`（SZ）被猜对是巧合，`5` 开头的深市 ETF 会猜错；`holdings.html` 交易提交新建持仓的 `mkt` 也按同样猜测写入；`analysis.html` 试仓金额里还有一处 `code[0] === '6' ? 100 : 100` 的死 ternary。

**修复**：`data.js` watchlist 4 项补显式 `mkt` 字段（holdings 本就有）；分析页页头改读 `item.mkt`（`all()` 随 holdings/watchlist 带出），试仓金额删除死 ternary、按 `kind` 取 1 手（转债 10 / 其余 100）；holdings 新建持仓继承 `cur.mkt`。`04` 契约同步：`watchlist` 行补 `mkt / kind / limitPct`、`holdings` 行补 `limitPct`、工具表补 `priceDec` 与 `limitPrices`、适配器要求第 5 条改为已删除。

**验证**：`grep -rn "code\[0\]" design/` 无命中；vm 沙箱断言 holdings/watchlist 每项 `mkt` 与代码真实交易所一致；7 页内联脚本编译通过。

### B2 · 涨跌停口径三处偏差 —— 2026-10-28 修复（P1-T2）

**原现象**：板块未区分（创业板/科创板实际 ±20%，ST ±5%，原型 ±10% 一刀切）、转债上下限不对称（−20%/+10%）、提示文案与拦截阈值差 0.1%（`0.9 — 1.1` vs `0.899 — 1.1001`）。

**修复**：`03` §2 重写为定稿口径——比例是**标的属性**：股票/转债按代码前缀推断（沪深主板 ±10、主板 ST ±5、创业板/科创板 ±20 且无 ST 制度、沪深转债 ±20 对称），ETF 无法由代码推断 → 数据层显式配置 `limitPct`（P4 由行情源自动填充），北交所 ±30% 超出产品范围；限价 = 昨收 × (1 ± 比例) 按报价小数位**整数单元四舍五入**（half-up，与交易所一致，消除裸浮点 `9.85 × 1.1 = 10.834999…` 错舍成 10.83 的噪声）；显示与拦截同一对限价、含边界，0.1% 容差废除。`price_limit` 黄金向量转 active（24 用例：limit_pct / limit_prices / validate_price），Rust `price_limit.rs` 与 TS `price_limit.ts` 两侧同步；原型 holdings 抽屉接入（`data.js` `limitPrices()`，演示标的配置 `limitPct`）。

**验证**：`cargo test -p zimu-domain`（11 单测 + 76 向量）与 `npm test`（84 项）全绿；vm 沙箱断言原型 `limitPrices` 与 fixture 同源手算点一致；人工：抽屉限价显示与拦截边界一致（压线合法）。

### B3 · 过户费文案过时 —— 2026-10-28 修复（P1-T3）

**原现象**：代码对所有股票收取过户费（`holdings.html` fee()），注释与界面文案却写「沪市过户费」。2022-04-29 起沪深两市均按成交金额 0.001% 收取——代码对、文案错。

**修复**：`holdings.html` 尾注与 `design/README.md` 改为「过户费 0.001%（沪深两市均收）」，代码注释同步；`03` §3 ⚠️ 消除。

**验证**：`grep -r 沪市过户费 design/ docs/` 无命中。

### B4 · 可转债小数位不统一 —— 2026-10-28 修复（P1-T3）

**原现象**：抽屉按 `kind === 'etf' ? 3 : 2`（`holdings.html:278`），流水按 `price < 10 ? 3 : 2`（`:389`），持仓表 / dashboard / 分析页各自为政；转债在抽屉里显示 `126.72`、在流水里显示 `122.800`。

**修复**：小数位收进 `data.js` 的 `priceDec(kind)`（stock 2、ETF/转债 3，与领域库 `price_decimals` 黄金向量同源），全部显示处替换：holdings 抽屉/流水/持仓表/toast、dashboard 持仓与自选、analysis（删除 `last2` 与 `code[0]==='5'` 的 kind 猜测）、chat 引用、wechat 卡片预览、diagnosis 止损建议、`tickPrices` 闪动；`watchlist` 补 `kind` 字段；转债文案字面量同步 3 位（`132.400` / `127.150` / `105.350`）。`03` §1 增补小数位约定、⚠️ 消除。

**验证**：全仓不再出现 `? 3 : 2` / `price < 10` 变体；7 页内联脚本语法编译通过，vm 沙箱断言 `priceDec('cb') === 3` 且已导出到 `ZM`。

### A1 · `analysis.html` 打开即报错，后半屏空白 —— 2026-10-24 修复

**原现象**：共享数据层不产出 `weight`，也没有 `totalMvAll()`，两者都是 `holdings.html` 页内自己补的全局（旧版 `:192`/`:196`）。分析页独立加载时 `render()` 在构建执行计划处抛 `TypeError`，**执行计划、风险自陈、历史建议、30 日 K 线、建议排序表全部不渲染**；首页「今天需要你决定」第一条恰好链到 `analysis.html?code=002594`。

**修复**：收进契约层——`recalc()` 统一派生 `h.weight`（`data.js:109`，与 `wp()` 同分母），`totalMvAll()` 由 `data.js:125` 导出；删除 `holdings.html` 的本地定义。分析页**没有**加 `|| 0` 兜底（那会把报错变成静默的 0.0%）。

**验证**：vm 沙箱加载 `data.js`，复现原崩溃表达式可正常求值；每只持仓 `weight` 存在且 `Σweight === exposure`、`weight.toFixed(1) === wp(code)`；七个页面内联脚本全部通过语法编译。

### A2 · 首笔持仓没有入口 —— 2026-10-28 修复（P1-T1）

**原现象**：所有界面假设账户里已有 10 只标的。真实用户第一次打开时持仓为空、`totalAssets = cash`，但空态引导与「录入第一笔」路径缺失；`dashboard.html` 头图与收盘摘要、`index.html` 问候语在空持仓下取 `holdings[0]`/`find(...).name` 直接抛错。除零防护此前已随 A1/B1 做进 `recalc()`。

**修复**：`data.js` 增加空持仓演示模式——任意页 `?empty=1` 进入（sessionStorage `zimu:demo:empty` 跨页保持）、`?empty=0` 退出；空态清空持仓 / 预警 / 问答 / 会话 / 建议库 / 净值序列，`watchlist` 保留为「可录入候选」演示池（真实实现候选来自行情搜索，P4 边界）。各屏分支渲染空态引导（共享 `ZM.emptyBlock()`，样式 `.empty-hero` 进 `ui.css`）：index 问候 / 动作按钮 / 导航统计 / 时间线 / 待决事项换三步引导路径；dashboard 头图、归因、预警、持仓表、收盘摘要各归各位的引导；holdings 表内引导 + 主按钮变「＋ 录入第一笔」+ 抽屉标题随语境切「录入第一笔」（入口 = 复用录入表单抽屉，ADR-0006）+ `?add=1` 自动开抽屉 + 流水 / 纪律空态；analysis 范围内无标的时隐藏结果卡显示引导卡（可切「自选」看观察类结论）；chat 空态专属欢迎与「第一笔怎么录入 / 今天该做什么」问答、会话历史按模式分 key（`zimu:chat:me:empty`）；diagnosis 指标不伪装「0 = 安全」全部改说明态；wechat 卡片模板留结构换占位文案、按钮禁用、推送日志空态。`charts.js` 对空序列加防御（`line/spark/bars` 留空不抛）。

**验证**：headless Edge 冒烟四组全绿——`.file-versions/smoke-headless.ps1`（file + http 正常模式回归）与新脚本 `.file-versions/smoke-headless-empty.ps1`（file + http，7 页 `?empty=1`：0 console 错误、空态标记齐全、正文无 `NaN`/`-Infinity`/`undefined` 泄漏）；`check-p1t1-add.ps1` 验证 `?empty=1&add=1` 自动打开抽屉且标题为「录入第一笔」。T9 人工验收补充项：空态下逐页走查引导话术与 CTA 落点。

### B1 · 行情跳动后同一屏内数字自相矛盾 —— 2026-10-24 修复

**原现象**：`tickPrices()` 只改 `price/dayChg`；`totalAssets / dayPnl / sectors / hhi / exposure` 等是加载时算一次的常量；各页定时器重算范围不同（holdings 每 6s 重算逐笔、dashboard 每 5s 只重绘净值、其余页面不动），行情条「我的总资产」永远是加载值 → 同一屏多个模块互相对不上。

**修复**：

- `data.js`：汇总改为 `agg` 快照，`recalc()` 统一重算（含逐笔派生与 `weight`）并 `publish()` 回 `ZM`（`data.js:76-123`）；`tickPrices()` 内部调用 `recalc()`、重绘行情条，并**返回价格是否真的变动**（`data.js:492`），休市时自动跳动直接跳过；`chrome()` 新增 `onRefresh` 回调，顶栏刷新按钮跳动后通知页面重绘。
- 页面：dashboard 收拢出 `renderAll()`（头条/净值/配置/归因/持仓表/摘要一起重绘，删掉了只重绘净值的冗余 60s 定时器）；holdings 定时器与刷新回调走同一组渲染函数；analysis `render()`、index `renderHead()`、chat `renderCtx()`、diagnosis `renderAll()` 接入 `onRefresh`。wechat 页不接（其卡片是历史推送样式，非实时视图）。
- 现金改动的唯一源头是 `ws.cash`（交易页提交处），`recalc()` 后重新发布为 `ZM.cash`，不再直接写 `D.cash`。

**验证**：vm 沙箱断言「交易改动 → `recalc()` → 导出快照同步」「手动 tick → 聚合与逐笔自洽」「休市自动 tick 返回 `false` 且价格不动」「空持仓无 NaN」，25 项全部通过（脚本在 `.file-versions/check-contract.js`，该目录被 gitignore，不入库）。
