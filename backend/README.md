# backend · 子木 Rust 服务

ADR-0005 定案：axum REST + SSE 托管 React SPA，内网部署，默认不开公网入站。
当前包含：`zimu-server`（主服务：`/api/health` + `frontend/dist` 静态托管 +
Agent SSE 会话 + 持仓录入 API + 行情 API + 预警/快照 API + OpenAPI 契约，
P2-T3/P4-T1/P4-T4/P4-T5/P4-T7；P2-T5 起桥体消费 robit `robit-web` crate；bin 侧
启动调度与预警引擎）、`zimu-agent`（P4-T6/T7 agent 应用层：领域工具 14 件（含 ADR-0012/0013 写账本五件）+ robit 标准工具面（ADR-0014，确认关闭自动执行）+
Skill 模板加载 + 与 HTTP API 共用的刷新链服务 + 调度预警引擎 `scheduler`，
server 与 CLI 共同消费）、`zimu-agent-cli`（Agent 调试 REPL：P2-T2 spike 起，
P4-T6 换装与 server 相同的领域工具面与 Skill）、`zimu-domain`（P3-T3 领域纯函数库：
`docs/03` 全部数字口径的 Rust 实现，含 P4-T6 打分模块 `advice` 与 P4-T7 预警
判定 `alert`，`cargo test -p zimu-domain` 跑 `fixtures/golden/` 黄金向量）、
`zimu-store`（P4-T1/T2 存储层：SQLite schema + 迁移 + 录入事务 + 建议/预警/
快照/推送日志读写，校验复用 zimu-domain，`cargo test -p zimu-store`）与
`zimu-market`（P4-T4 行情适配器：datafetch sidecar 的**唯一消费入口**——类型化
契约、降级三分类、市场时钟、当日 bar 合成、行业映射缓存 `SectorMapCache`（P7-T6），
`cargo test -p zimu-market`）。

## 运行 zimu-server

生产形态（单服务单端口，前端构建产物由后端托管）：

```powershell
cd ..\frontend
npm install          # 首次
npm run build        # 产出 frontend/dist
cd ..\backend
cargo run -p zimu-server   # → http://127.0.0.1:8080
```

开发形态（前端热更新，两个进程）：

```powershell
cargo run -p zimu-server               # 后端 :8080
cd ..\frontend; npm run dev            # 前端 :5173，/api 已反代到 :8080
```

浏览器访问 `http://127.0.0.1:5173`（开发）或 `http://127.0.0.1:8080`（生产形态）。
workspace 里有多个 bin，`cargo run` 必须带 `-p` 指明。

停止服务：控制台按 **Ctrl+C** 即优雅关闭——先中止预警调度器，再逐通道停 IM
supervisor（QQ/微信，notify + 等退出 + WAL flush），最后断开全部 SSE 长连接，
等在途请求完成后退出（数据库连接与 Agent 会话随进程释放）。

## 会话端点（P2-T3 SSE 桥）

Agent 随服务启动装配（装配线在 `zimu_agent::assembly::AgentCore`——Web 桥、
QQ/微信通道与 zimu-agent-cli 共享同一 `LlmClient`/`ToolRegistry`/`SkillRegistry`
三 Arc，ADR-0011/T1；`CliFrontend` 换成 robit-web 的 `WebFrontend`：`WebEvent`
broadcast 扇出 + 单槽位确认，P2-T5 起由 robit 提供）。宿主侧只剩 axum 接线
（SSE 编码、`connected` 首帧——P4-T6 起含 `skills` 键、POST → `WebBridge` 路由，
见 `main.rs`）与桥接线（`agent_bridge.rs`）；工具面与 Skill 模板在 `zimu-agent`
crate（见下节）。找不到 robit 配置时服务照常起、静态首页可用，仅 `/api/chat*`
降级 503。会话真值在服务端（T3/T4）：用户消息**先落 `chat_messages` 再注入**、
事件流经 recorder 状态机归档（thinking 不入档）、启动时 `Agent::with_history`
复活回灌；`GET /api/chat/history` + `POST /api/chat/reset` 进契约。边界更新：
Web 通道仍是单会话（多 Web 会话不是本轮义务），IM 通道按 chat_id 独立会话。

Web 文件上传（ADR-0015）：`POST /api/chat` 走 multipart/form-data，`chat_upload.rs`
流式落盘 `working_dir/media/`（与 QQ 同根；消毒文件名 + 防覆盖 `-1/-2` 后缀 +
`[app.context] max_media_bytes` 缺省 20MB 守卫，超限 413 删半截），经
`AgentBridge::submit_user_input` 注入——档案存「原文 + 标注行」、模型收原文 +
attachments（**robit 零改动**：图像本地优先 vision、非图像路径标注由标准工具
read/bash 自动解析，ADR-0014）。落盘卫生规则复刻 robit-agent media.rs 私有 fn
（R12 诉求上游公开）；axum 因此启用 `multipart` feature。

QQ IM 通道（P7-T2/T5，ADR-0011）：`[zimu.channels].qq = true` 时 zimu-server 内
挂 `ChatbotManager<BoundOnly<QqPlatformAdapter>>`（robit-chatbot 0.5.1 库面消费，
robit-qq 二进制不复用）——缺省停用 fail-closed；入站过绑定白名单（`channel_bindings`
表，未绑定静默丢弃 + 日志留 chat_id），出站推送（T6：预警 + 15:05 净值摘要，
文案过 QQ markdown 清洗、逐目标 push_log）；重连归 adapter 内置 supervisor，
宿主循环只兜残余退出。health 透出 `qq_online`。

微信 IM 通道（P6-T2，ADR-0017 / C4 关闭）：`[zimu.channels].wechat = true` 时挂
`ChatbotManager<BoundOnly<WechatPlatformAdapter>>`（robit-wechat 0.5.1 库面消费）——
微信官方 **iLink AI Bot**（1:1 私聊，chat_id 恒 `private:{ilink_user_id}`；出站
长轮询**无需公网回调**）。与 QQ 同 working_dir 共 robit.db，会话按
`(chat_id, source, namespace)` 隔离（R8④ 收口，不拆目录——人格/记忆跨通道同源）；
入站过同一绑定白名单（装饰器零改动换词汇；**待绑定观察队列** D3 补记：未绑定
来消息丢弃时自动入列，`GET /api/channel_bindings/pending` 透出、绑定卡一键绑定，
绑定成功即移除——观察自动化、授权仍人工，内存态重启清零），出站推送共用 **caps 参数化链**
（`channel_push.rs`：通道词汇 = `T::platform_name()`、清洗 = `T::capabilities()`，
**全文透传不自截**——长度守卫归 robit adapter（ADR-0020，微信 ≤3 段分段 / QQ 单条截断+省略号）；
复合 pusher 逐通道顺序 OR，`SchedulerCore` 签名不动）。
health 透出 `wechat_online` + `qq_enabled`/`wechat_enabled`（启动开关快照，
界面只读区分「未启用」与「离线」，拍板⑤⑥）。

**登录 = 部署侧 CLI 扫码（停服约定，硬需求）**：

```powershell
# 先停 zimu-server——WechatState::save 非原子整文件覆盖且运行中无互斥，
# 健康期并发登录写入的新 token 会被 adapter 旧内存快照覆盖（ADR-0017 D3 / R13④）
cargo install robit-wechat --version 0.5.1   # 或 robit 仓 cargo run -p robit-wechat -- login
robit-wechat -w e:\Github\StockAgent\backend login   # 终端二维码 + 配对码，一次性换长期 token
# 0.3.1 起 login 支持 Ctrl+C 取消（不写半成品 state 文件）
# 产物 backend/.robit/wechat-state.json（含 bot_token 凭据，已被 **/.robit/* gitignore
# 覆盖，**勿加豁免**）；再翻 config.toml [zimu.channels] wechat = true → 起服
```

**token 失效（errcode -14）是终态**：无自动重登录。supervisor 停止快速重建
（同 token 重建无意义），error 日志给重扫指引，转 **60s 低频只读探测**
（重解析 `WechatConfig::from_config` 比对 bot_token）——变化即自动重建
（重扫后 ≤60s 零接触恢复；失效态 adapter 静默无 state 写，**不停服重扫安全**）。
局限如实记：同值重登录 / config override 未更新不可检测。凭据闸门：
`[channels.wechat_bot].bot_token` override 含 `${` 残留 → 拒绝并退避（state
文件来的扫码 token 不会含占位符）。robit 缺口批见 `docs/07` R13（入站媒体绕过
`max_media_bytes` 守卫 / silk 语音不可消费 / Image MIME 恒 jpeg / state 非原子），
子木零绕过（白名单限缩风险面 + 停服约定承接）。

| 端点 | 说明 |
|---|---|
| `GET /api/chat/stream` | SSE 事件流：首帧 `connected`，随后 `text_delta` / `tool_call` / `tool_result` / `confirm_request` / `turn_complete` / `error`（形状见 `agent_bridge.rs` 的 `SseEvent`） |
| `POST /api/chat` | multipart/form-data（`text` 字段可空 + `files` 文件字段多选）→ 202，回复经 SSE 流出；附件落盘 media/ 注入 Agent（ADR-0015），超限 413 |
| `POST /api/chat/confirm` | `{"tool_call_id":"…","approved":true}` 回填工具确认 |
| `POST /api/chat/cancel` | 取消当前轮次 |

「智能问答」视图的会话面板（`frontend/src/components/ChatPanel.tsx`，工作台首页点
侧栏「智能问答 / 问一句」或对应入口卡进入，P2-T4 起）即消费这组端点；
它是 spike 级 UI，正式 chat 屏 P5 按 `design/chat.html` 重做。

curl 冒烟——**Windows 终端里中文 payload 必须走 UTF-8 文件**，内联会被编码搞坏
（服务端报 `invalid unicode code point`）：

```powershell
'请调用 echo 工具回显:你好子木' | Out-File -Encoding utf8NoBOM msg.txt
curl.exe -sN http://127.0.0.1:8080/api/chat/stream   # 另开窗口看事件流
# 纯文本（multipart 仅 text 字段；< 从 UTF-8 文件读字段值，避免 Windows 内联编码问题）
curl.exe -s -X POST http://127.0.0.1:8080/api/chat -F "text=<msg.txt"
# 带附件（ADR-0015：text + files，落盘 media/ 后 Agent 自动解析）
curl.exe -s -X POST http://127.0.0.1:8080/api/chat -F "text=<msg.txt" -F "files=@positions.csv;type=text/csv"
```

## zimu-agent（P4-T6/T7 agent 应用层：领域工具 + Skill 模板 + 调度引擎）

server（SSE 会话）与 CLI（调试 REPL）共用的公共 crate——P2 期 echo 工具两处
重复的债在此收敛。边界：**数字口径只在 `zimu-domain`**，工具只做取数
（zimu-store / zimu-market）→ 调领域函数 → 组装 JSON；行情只经 `zimu-market`
（sidecar 唯一消费入口）；**写账本五件（ADR-0012/0013）**：`record_trade`（成交
录入，与界面录入同一 store 事务，note 自动追加来源标记）/ `adjust_cash`
（现金绝对值校正）/ `get_trades`（流水只读）/ `upsert_holding`（持仓快照校正，
整行覆盖 + holding_edits 留痕 + 三不动，03 §13）/ `remove_holding`（删行，流水
保留、删除前对话内两步确认）——自动落账 + 读回纪律（数字必来自用户明示、先试算
后落账并宣读收据、流水只增不改错账冲正、整行覆盖先读后改）；系统不对接交易系统，
「永不代下单」是表述纪律。`simulate_trade` 仍只试算不落库。

| 工具 | 性质 | 说明 |
|---|---|---|
| `market_status` | 只读 | 市场时钟（03 §9）+ sidecar 在线 + 日历可用性；永不失败 |
| `get_quote` | 只读 | 实时行 + 按 03 §2 拼装 `limit_up/limit_down`（domain price_limit）；裸代码查持仓 mkt，非持仓必须带 sh/sz 前缀（B7） |
| `get_holdings` | 只读 | 组合全景（store.portfolio → recalc）+ 纪律四条 |
| `calc_discipline` | 只读 | 03 §6 四条体检；单票超限附 `reduce_hint`（sell_to 降仓股数） |
| `simulate_trade` | 只读 | 试算（费用/摊薄/整手/涨跌停拦截，03 §1–§4），不落库 |
| `refresh_quotes` | 写行情字段 + 补空串行业 | 与 `POST /api/market/refresh` **同一条链**（`service::refresh_portfolio`，P4-T6 抽取；行业补全 P7-T6/ADR-0016，契约 wire 零变化） |
| `calc_advice` | 写 advice_history 数字列 | `zimu-domain::advice::score_advice`（03 §10 生产口径）出数 → 数字列直接留档（B9：`generated_at` + `data_version=adv-v1\|k=…\|q=…`）→ 返回 advice_id 与全部数字；日线拿不到**拒绝出建议** |
| `finalize_advice` | 写 advice_history 文案列 | 模型文案只回填 reason/risks（risks 至少一条，03 §10）——**数字不经模型转写**（ADR-0007 结构性保证） |
| `echo` | 只读 | P2-T2 链路验证保留（网关排查不依赖数据） |
| `send_file` | IM 出站 | 发本地文件/图片给用户（ADR-0017 D9，robit-chatbot SendFileTool）：平台扩展自守卫——QQ 真实 CDN 上传、微信 SDK send_media；Web/CLI 无平台扩展调用如实报错，无副作用 |

Skill 模板在**仓库根 `skills/`**（robit SKILL.md 约定：YAML frontmatter +
Markdown 正文；`/触发词` 命中时正文以临时 system message 注入本轮）。不放
`<workdir>/.robit/skills/` 是因为 `.robit/` 整目录被 gitignore，而模板是
**产品文案资产**（内嵌 03 口径引用、免责边界、「文案数字必须原样取自工具
返回」硬规则），必须入库评审：

| Skill | 触发 | 流程要点 |
|---|---|---|
| `post-market-review` 盘后复盘 | `/review` `/复盘` | market_status → refresh_quotes（失败降级库存值）→ get_holdings → calc_discipline；四段输出 + 免责 |
| `pre-market-watch` 盘前关注 | `/watch` `/盘前` | market_status → get_holdings → get_quote（涨跌停价只取工具返回）→ 关注点 |
| `generate-advice` 建议生成 | `/advice` `/建议 <代码>` | refresh_quotes → calc_advice（拿 advice_id + 全部数字）→ 三段式文案（结论/为什么/怎么做 + 失效条件）→ **finalize_advice 归档后才算完成** |

```powershell
cargo test -p zimu-agent   # 19 项集成（全离线）——tools 10 项：黄金向量 av-bull-add 同款输入端到端
                           #（日线 24 根 + 当日 bar 实时合成 → score 73/conf 0.9/加仓 → 留档逐位一致）、
                           # 文案回填不动数字、risks 空被拒、B7 裸代码拒绝、限价与 domain 同源、
                           # sidecar 挂时 refresh 报错不写库 / market_status 不瘫、Skill 触发词唯一 + tools_required 已注册；
                           # scheduler 9 项见下节
```

注意：工具输出经 JSON 文本往返后与内存值可差 1 ulp（serde_json 浮点解析
fast path 的双舍入），测试比对与 03 附录同款按容差（1e-9），不做 Value 精确相等。

## 调度与预警引擎（P4-T7，03 §12）

`zimu-agent::scheduler`——**纯编排**，预警判定的数字口径全在
`zimu-domain::alert::eval_alerts`（8 类判定序：涨跌停/跌破成本/止损/止盈/
放量/强赎/临近到期，黄金向量 `alerts.json` 17 例），调度器只组装事实、去重、
落库。核心是 `SchedulerCore::tick(now)`——**时刻注入**，测试用模拟盘中直接
驱动（验收「模拟盘中可触发」），生产由 `run()` 薄壳每 60s 调一次
（`now_shanghai()`）。server 的 bin 侧启动（`main.rs`），`ZIMU_SCHEDULER_DISABLE=1`
停用；复盘触发经 `ReviewTrigger` 回调接 `WebBridge::send_user_text("/复盘")`——
本 crate 不依赖 robit-web，CLI/测试传 None。

一轮 tick 的行为（全部降级如实、不伪造）：

| 时机 | 动作 |
|---|---|
| 非交易日 | 空转，不打 sidecar（市场时钟 §9 判定） |
| 交易日 live（集合竞价/早盘/午盘） | 刷新链（`service::refresh_portfolio_detailed`，与 HTTP refresh 同链）→ 判定 → 同日 (code,type) 去重 → 落 `alerts`；**行情失败该轮不判定**（不用库存旧价）。自选（B13）：与持仓不重叠的自选代码另行合并一次 `quote`（**不回写库**）按无持仓形态判定，失败只降级自选侧 |
| 每交易日首轮 | `/cb_list` 一次（持仓**或自选**含转债时；重请求不进分钟级轮询）；持仓与自选各标的日线拉近 20 根量窗口缓存（同一缓存，当日失败 → 放量类今日不参与） |
| 交易日 ≥15:05 且当日无快照 | 最终刷新 → `equity_snapshots`（date 主键 upsert 幂等，数字 = recalc agg 原值不舍入）→ `/复盘` 注入会话 → `push_log` 留痕；刷新失败整链顺延（宁缺不写旧数）；重启后当日缺失自动补跑 |

stop/target_hi 取该标的**最近一条 `advice_history` 留档**（无留档 → 止损/止盈
类不参与）；限价按 §2 由 `price_limit` 拼装（数据层配置优先，未配置推断）。
**自选按「无持仓形态」判定**（03 §12，B13 定稿）：cost/stop/target_hi 一律
不喂 → 序 3/4/5 不参与（即便该标的恰有建议留档）；序 1/2/6/7/8 与持仓同款，
判定函数零差别——范围扩展全在编排层。持有中的代码重复进自选只按持仓形态判一次。

```powershell
cargo test -p zimu-agent --test scheduler   # 9 项（全离线，FakeApi + 注入时刻）：
    # 模拟 10:30 触发跌破成本+止损（判定序）并同日去重 / 非交易日与午休空转不打 sidecar /
    # sidecar 挂降级不判旧值且恢复后自愈 / 15:05 快照手算锚点 + 复盘回调恰一次 + push_log /
    # agent 缺席快照照写 ok=0 / 刷新失败快照顺延补跑 / cb 强赎+放量触发与日级缓存计数 /
    # 自选无持仓形态（跌停+放量触发、留档 stop 不喂、行情不回写、同日去重）/ 持仓重叠自选不双判
```

## 预警与净值快照 API（P4-T7）

内容由调度引擎判定落库，端点只读取与归档（`alerts_api.rs`，不生成不改写）：

| 端点 | 说明 |
|---|---|
| `GET /api/alerts?done=&limit=` | 预警列表，新在前；`done=true/false` 过滤归档态，缺省全部（默认 100，上限 500） |
| `POST /api/alerts/{id}/done` | 归档（done=1，不删行，重复归档幂等）；id 不存在 → 404 `no_alert` |
| `GET /api/equity_snapshots?limit=` | 净值快照，**date 升序**（净值曲线输入形态）；limit 取最近 N 条（默认 120，上限 500） |
| `GET /api/push_logs?limit=` | 推送/触发留痕，新在前（P5-T2 补读端点，B13）；`ok=1` 只表示请求已被受理（03 §12），index 时间线与 wechat 屏推送日志共用 |

## 建议留档 API（P5-T2）

`advice_history` 的读端点（`advice_api.rs`）；写入在 agent 工具链（P4-T6，ADR-0007——数字列 = 规则函数产出直接落库，reason/risks 由 `finalize_advice` 回填模型文案）：

| 端点 | 说明 |
|---|---|
| `GET /api/advice?code=&limit=` | 建议留档行，新在前（generated_at DESC）；`code` 缺省全部标的（默认 50，上限 500）；`reason` 空串 = 文案未回填，前端如实展示 |

## 自选 API（P5-T2，B13 定稿）

`watchlist` 表（schema v2）的 CRUD（`watchlist_api.rs`）。自选 = 观察标的（非持仓，无 shares/cost）；预警判定范围已扩到自选（03 §12 无持仓形态，调度器分钟级拉行情**不回写本表**——展示价前端走 `/api/market/quote`）：

| 端点 | 说明 |
|---|---|
| `GET /api/watchlist` | 自选全量，加入时间序（added_at, code） |
| `POST /api/watchlist` | `{"code","name","mkt","kind","limit_pct"?,"sector"?,"note"?}` → 200 行。按 code **upsert 幂等**（重复添加 = 更新元信息/备注，added_at 保留首次值）；市场码非法/名称空 → 400 `bad_input`（B7 显式 mkt） |
| `DELETE /api/watchlist/{code}` | 移除，返回被移除行；不存在 → 404 `no_watchlist_item`。只动自选表——持仓、预警历史与留档不受影响 |

## 持仓录入 API（P4-T1，手工逐笔先行）

录入流程的后端半边（ADR-0006：手工逐笔先行，CSV 导入是 P4 后续增强单独立项）。
校验与派生**全部走 `zimu-domain` 纯函数**（整手、涨跌停、费用、摊薄、仓位、
现金充足性——即黄金向量锁死的那套口径），`zimu-store` 只负责事务组织：
`BEGIN IMMEDIATE` → 校验 → 试算 → 持仓/流水/现金三写 → `COMMIT`，任一步失败整体回滚。

| 端点 | 说明 |
|---|---|
| `GET /api/holdings` | 组合全景：存储行 + `recalc()` 的逐笔派生（`per`）与汇总（`agg`）、账户现金 |
| `POST /api/holdings/trade` | `{"code","side":"buy\|sell","qty","price","note","meta":{...}}` → 201 收据（trade 行 + 变动后持仓 + 试算预览 + warnings）；卖出与已有标的加仓可不带 `meta` |
| `PUT /api/holdings/{code}` | **直接维护 upsert**（ADR-0009 / 03 §13 快照校正）：`{"name","mkt","kind","sector","shares","cost","limit_pct"?,"note"?}` → 200 维护后组合全景。整行覆盖、允许奇零股、跳过 lot/涨跌停（校正不是委托）；**不动现金、不写 trades**，留痕进 `holding_edits`；与现有行逐位一致 = 幂等 no-op；代码非 6 位/市场码非法/shares<1/cost<0 → 400 |
| `DELETE /api/holdings/{code}` | **直接删除**：→ 200 删除后组合全景。trades 历史保留、累计已实现不回滚、现金不动；不存在 → 404 `no_holding` |
| `GET /api/holding_edits?limit=N` | 维护留痕（最新在前，默认 50 上限 200）：op = create/update/delete + before/after 行快照 + note（「维护记录」卡数据源；交易录入不产生本表记录） |
| `GET /api/trades?limit=N` | 委托/成交流水，最新在前（默认 50，上限 500） |
| `POST /api/account` | 首次配置/调整账户：`{"cash":200000}` 即可（缺失字段保持原值）；现金为负拒绝 |

拒绝的语义分层：`400` 输入问题（数量非整手、超可卖、负现金）；`404` 持仓里没有该
标的；`422` 业务规则拦截（涨跌停越界带 `limit_down/limit_up/decimals` 字段、现金不足带
`cash_after`）；`500` 存储内部错误（细节只进日志）。「单票超 25% 上限」**不是拒绝**，
进收据的 `preview.warnings`（与原型抽屉一致：纪律提示不拦提交）。

```powershell
# 端到端走一遍（现金 → 首笔 → 全景）
$body = '{"code":"600519","side":"buy","qty":100,"price":1600.0,"note":"首笔","meta":{"name":"贵州茅台","mkt":"SH","kind":"stock","sector":"食品饮料","prev":1580.0,"limit_pct":10}}'
Invoke-RestMethod http://127.0.0.1:8080/api/account -Method Post -ContentType "application/json" -Body '{"cash":200000}'
Invoke-RestMethod http://127.0.0.1:8080/api/holdings/trade -Method Post -ContentType "application/json; charset=utf-8" -Body ([Text.Encoding]::UTF8.GetBytes($body))
Invoke-RestMethod http://127.0.0.1:8080/api/holdings
```

前端「持仓管理」抽屉接这组端点是 P5-T2 的事；在此之前 `design/` 原型仍是录入交互的验收基准。

## 行情 API（P4-T4，zimu-market 适配器）

sidecar（`datafetch/`，默认 `http://127.0.0.1:8300`）的**唯一消费入口**：
类型化契约（serde 模型对齐 `datafetch/README.md`）、错误三分类（`Unavailable`
不可达 → 降级信号 / `Upstream` 错误契约透传 / `Contract` 版本漂移 → 502）、
市场时钟（`03` §9 生产口径：恒 Asia/Shanghai、交易日历取代工作日启发式）、
当日 bar 合成（`04` 附录 B）。**sidecar 不在也能起服务**——行情端点 503
`market_unavailable`，持仓/录入/会话/静态托管照常，时钟退工作日启发式并如实
标注 `calendar_degraded`。

| 端点 | 说明 |
|---|---|
| `GET /api/market/status` | 市场时钟（state/dot/live/next_event/next_at/next_trade_day）+ sidecar 在线状态 + 日历可用性；**永不失败**（sidecar 全挂也 200） |
| `GET /api/market/quote?codes=sh600519,…` | 实时行情透传（含 `degraded`/`attempts` 降级可观测字段） |
| `GET /api/market/kline/{symbol}?kind&start&end&with_today` | 日线透传；`with_today=true` 用实时行情合成当日 bar（响应带 `today_synthesized`，盘中该 bar 随行就市非定稿） |
| `GET /api/market/cb_list` | 转债列表（现券 + 强赎）透传 |
| `GET /api/market/calendar?start&end` | 交易日历透传（默认窗口今天 ±180 天，适配层缓存 1h） |
| `POST /api/market/refresh` | 拉全部持仓实时行情 → 回写 `price/prev/name` + `limit_pct` 推断（仅未配置时，ETF 不推断）→ **补空串行业**（P7-T6：遍历全部持仓含停牌退市，股票查 `/sector_map`、ETF=基金/转债=转债，手工值不覆盖、映射失败不阻断）→ `recalc()` 全量重算并返回组合全景；无行情标的（停牌/no_data）保持库存旧值并进 `skipped` |

数字口径链路：`/api/market/refresh` 与 `/api/holdings` 的 `per/agg` 只出自
`zimu-domain::recalc`（黄金向量锁死）；无行情降级按 `03` §5（`prev` 缺失视为
等于 `price`，dayChg/dayVal 取 0，**不伪造盈亏**）。集成测试
`crates/zimu-server/tests/market_api.rs` 断言 API 数字与 domain 输出逐位一致。

```powershell
# 联调：先起 sidecar 再起服务（sidecar 缺席时行情端点降级 503）
cd ..\datafetch; uv run uvicorn zimu_datafetch.server:app --host 127.0.0.1 --port 8300
cargo run -p zimu-server
Invoke-RestMethod http://127.0.0.1:8080/api/market/status          # 时钟 + sidecar 在线
Invoke-RestMethod -Method Post http://127.0.0.1:8080/api/market/refresh  # 行情回写 + 重算
cargo test -p zimu-market -p zimu-server   # 适配器 29 项 + server（集成 8 + 契约 3 + bin 单测 1，全离线）
```

## API 契约（P4-T5，OpenAPI + serde 同源）

**前后端类型同源**：API 响应/请求全部是类型化 serde 结构体（`json!` 临时拼装
已清除），在定义处 `#[derive(ToSchema)]`（zimu-domain / zimu-store /
zimu-market / zimu-server 四 crate），`src/openapi.rs` 的 `ApiDoc`（utoipa 6）
汇编成 OpenAPI 3.1 → **`docs/api/openapi.json`**（52 schema、29 路径；文档
永不手改）。前端 `npm run gen:api`（openapi-typescript）生成
`src/api/schema.d.ts`，P5-T1 的 API client 直接消费。运行时
`GET /api/openapi.json` 提供同一文档。

契约测试（验收「样例响应过 schema 校验」，`tests/contract.rs` + jsonschema
draft 2020-12）三连：

1. 仓库 `docs/api/openapi.json` 与代码生成**逐字节一致**（改契约忘了再生成
   会直接红）；
2. seeded 测试服务（FakeApi 注入，`tests/common/` 共享装置）全端点**实时
   响应**过其声明的响应 schema，含 422/404/400/503 错误契约；
3. **`fixtures/api/` 37 份提交样例**过同一校验（样例永不腐烂，前端可当 mock）。

health/chat 实现在 bin（依赖 robit 装配），契约在 `openapi.rs` 以 doc-stub
声明、类型定义在 lib 供 bin 消费；bin 单测断言 `Health` 序列化键集 == schema
键集（无漂移盲区）。SSE wire schema 归 robit-web（`WebEvent`）。

```powershell
# 改契约结构体/标注后，再生成两个工件（然后 git diff 审样例变化）：
cargo test -p zimu-server --lib -- --ignored regenerate_openapi_json   # docs/api/openapi.json
$env:ZIMU_REGEN_API_SAMPLES='1'; cargo test -p zimu-server --test contract; Remove-Item Env:ZIMU_REGEN_API_SAMPLES
cd ..\frontend; npm run gen:api                                        # src/api/schema.d.ts
```

## zimu-store（P4-T2 存储层）

SQLite 单文件（C9 已决：rusqlite bundled，备份 = 拷文件；`.db` 在 `.gitignore`，
**真实持仓永不入 git**）。v1 schema 六表：`account`（单例行，现金源头）、
`holdings`（`shares` 为普通整数——奇零股存得下，B12 存储前提）、`trades`（费用三分项
留档可审计；`holding_code` **不是外键**——流水必须在清仓后仍留档）、
`advice_history`（`generated_at + data_version`，B9 正式解；**P4-T6 起写入**——
`insert_advice` 数字列 → `finalize_advice` 只回填 reason/risks 文案列，数字
不可改写，ADR-0007）、`alerts` / `equity_snapshots` / `push_log`（**P4-T7 起
写入**——调度引擎落预警行与去重查询 `alert_exists_on`、15:05 盘后链 upsert
快照（date 主键幂等，`from_agg` 只搬运 recalc 原值）与 push_log 留痕）。
**v2（B13 定稿 2026-10-29）**：`watchlist` 自选表——观察标的静态属性
（holdings 静态列同款 + `note`，无 shares/cost；code 主键 upsert 幂等），
03 §12 预警判定范围扩到自选；七表读写齐备。
**v3（ADR-0009 定稿 2026-11-01）**：`holding_edits` 持仓维护留痕表——直接维护
（快照校正）的审计痕（op = create/update/delete + before/after 可编辑字段快照 JSON +
note）；纯增表，v2 老库自动前进、旧数据无损（`watchlist_migrates_v1_db_to_v2` 覆盖）；八表齐备。

迁移机制：`PRAGMA user_version` 逐版前进，每版一个事务；已发布版本的 SQL 永不修改，
新迁移在 `MIGRATIONS` 尾部追加。rusqlite 版本必须与 robit-agent 对齐（0.43，随
robit 0.5.1 从 0.42 升）——`libsqlite3-sys` 的 links 约束全依赖图只允许一份 libsqlite3。

```powershell
cargo test -p zimu-store   # 28 项：录入事务/校验拒绝/回滚/重启不丢数据/行情回写（P4-T4）/建议留档（P4-T6）/
                           # 预警去重与归档、快照 upsert 幂等与升序读取、push_log（P4-T7）/
                           # watchlist upsert-删除-校验 + v1→v2 迁移（合成老库降版重开自动前进，旧数据不丢）
```

`portfolio_input()` 是领域输入的**唯一组装点**（03 §5 无行情降级语义：price
缺失以成本顶替、prev 缺失视为 price）——`recalc`、`discipline::check` 与
agent 工具全部从这里取输入，杜绝第二份降级拼装。

行情回写（P4-T4）：`apply_quotes` 批量更新 `price/prev/name/limit_pct`，
COALESCE 语义——`None` 字段保持原值（数据层配置的 ETF `limit_pct` 永不被覆盖）；
有效性过滤与推断在 server 适配层，存储层不定义口径。

`Connection` 非 `Sync`：zimu-server 用 `Arc<Mutex<Store>>` 串行化，录入是同步短事务，
处理跨 `.await` 持锁被禁止（当前持仓处理器无 await）。

## 运行 zimu-agent-cli（调试 REPL）

robit 全家消费 crates.io 正式版 `0.5.1`（0.5.1 = 子木第七轮移交 R17 落地，
2026-11-22 发版，交接件 `robit/docs/robit-rfc-im-table-projection.md` T1+T2+T3
全收（0.5.0），0.5.1 为维护版（rusqlite 0.42→0.43 上游 use-after-free 修复 +
CI Node 24，无 API/行为变化）：**T1 表格线性化**——`render_table` 空格对齐
伪表格（比例字体原理性错位）替换为 `linearize_table`：两列表逐行 `键: 值`、
N 列表逐记录 `首列 — 表头: 值 · …`（>5 列宽表记录内拆多行、空单元格跳过），
输出无管道残留、二次投影幂等，`features.tables=true` 直通激活死标志；
**T2 纯文本平台也过投影**——`flush_buffer` 删除 `supports_markdown=false`
跳过分支，微信输出与 caps 宣称一致（表格线性化、加粗/栅栏/标题标记去除、
引用去 `> `），降级尊重 features（QQ 字面 `**`/`~~` 去除、链接保留 URL）；
**T3 直调透明投影**——`prepare_outbound` 扩展为「内容投影 + 长度守卫」统一
出站准备（先投影后分段），直调 `send_message` 不调清洗器也自动获得投影，
与 R16 长度守卫完全同构。纯文本快速通道：无 Markdown 触发字符的文本逐字节
直通——预警/摘要 Clip 形态与系统通知行为逐位不变。子木纯升版零适配：推送链
显式清洗器调用保留（adapter 侧二次投影幂等），IM 表格展示缺口修复；
rusqlite 连带 0.43（与 robit 公共 API 交换 Connection/Error 类型须同线）。
上一版 0.4.1（子木第六轮移交 R16 落地，
2026-11-22 发版，交接件 `robit/docs/robit-rfc-outbound-length-guard.md` 方案 A
透明封装：robit-chatbot 公开 `outbound` 模块 `prepare_outbound`（分段/截断规则
单源化，`flush_buffer` 改调同一实现，≤cap 幂等直通）+ robit-wechat
`send_text_inner` 节流前透明分段（≤3 段每段独立过 `SendLimiter`，部分失败
放弃余段返回已发段 msg_id）+ robit-qq `send_message` 截断守卫（编辑平台
单条截断+`…` 省略号，不分段）。`send_message` trait 文档写明长度守卫契约
（实现方按 caps 强制、调用方不应自截）。子木纯升版零适配——P7-T8 chat_push
透传链零 diff 获得长文分段，预警/摘要 Clip 形态 ≤cap 直通行为逐位不变）。
上一版 0.4.0（子木第五轮移交 R14 落地，
2026-11-20 发版，交接件 `robit/docs/robit-rfc-wechat-outbound-ratelimit.md`
W1–W4 全收：robit-chatbot 通用 `throttle` 模块（账号级发送预算 = 最小间隔 +
滑动窗口 + 冷却 + 快照持久化，微信缺省启用：2s 间隔 / 10 条·180s 窗口 /
-2 冷却 120s，快照随 wechat-state.json 跨重启）+ `AgentError::RateLimited`
（-2 瞬态码类型化，`#[non_exhaustive]`）+ `HintPolicy` 进度提示策略（微信缺省
off）+ `send_message_best_effort` + 无编辑平台长回复分段不再静默截断。
v0.3.1 同批：Ctrl+C 优雅退出重做——`ChatbotManager::run` 与双 adapter `connect`
的 shutdown 参数 `Arc<Notify>` → `CancellationToken`（电平触发不丢信号）、退出前
等流式回复收尾最多 15s、`robit-wechat login` 可 Ctrl+C 取消。子木适配面：
`channel.rs`/`qq_channel.rs`/`wechat_channel.rs` 关停管线换 `CancellationToken`
+ `WechatConfig`/`PlatformCaps` 字面构造补新字段（测试）；微信出站节流/-2 语义/
冷却持久化对宿主直调 adapter 自动生效，子木零绕过）。
上一版 0.3.0（微信 iLink Bot 渠道接入，2026-11-17 发版，ADR-0017 / C4 关闭：
新 crate `robit-wechat`（微信官方 iLink AI Bot，协议 SDK weixin-agent 0.3）+
`PlatformCaps::wechat()` / `MarkdownFeatures::wechat()` + manager 附件契约
（local_path 已设跳过通用下载）。全 additive：trait/签名不变、robit-web 零 diff
（wire 不变）、存储 v7 零迁移、rusqlite 0.42 无连带——QQ 消费代码零适配。
子木消费面：`wechat_channel.rs` 双 manager 接线 + `channel_push.rs` caps 参数化
推送；缺口批 R13 登记移交，零绕过）。
上一版 0.2.1（M1 收尾，2026-11-14 同日发版：附件
标注行携带本地保存路径——`describe_with_local` 相对 working_dir 显示，
非图数据文件对模型从「能发现、够不着」变为路径可见；内容读取仍需宿主
注册收读工具，子木另案。纯 additive 无签名变化，子木纯升版）。
上一版 0.2.0（子木第四轮移交全量落地，2026-11-14 发版：R10 附件批 M1–M7 + R8 宿主形态 H1–H6，交接件
`robit/docs/robit-rfc-qq-media-and-host-feedback.md`——附件本地路径注入 +
vision 本地优先、标注恰一份、文件名消毒去重、20MB 缺省上限、media 保留策略、
`inject_message` 注入 API、source 参数化、cleanup 实装 + 僵尸自愈、
`(source, namespace)` 会话隔离、memory 跟随 global_storage、custom_prompt
人格注入；robit-web 零 diff，wire 不变。子木最小适配：Agent 两构造补
`custom_prompt=None`、`MessageData.source=None`、`ToolContext.memory_dir`、
BoundOnly/MockPlatform 补 `platform_name()` 转发；连带 rusqlite 0.41→0.42
对齐。新能力 inject_message/人格注入/保留策略本轮不消费，另案拍板）。
上一版 0.1.29（R9 chatbot 思考日志刷屏收口：ThinkingDelta 每帧一条 trace →
回合收口汇总计数，子木 QQ 真机走查反馈，无 API 变化；含 P7-T2 起新消费的
`robit-chatbot`/`robit-qq`；共同演进回路第三轮：子木 R7 思考过程输出需求文档 →
robit `feat/thinking-output` 落地发布 2026-11-05——robit-ai 双路径捕获
`reasoning_content`（模型级 `thinking = auto|on|off`）、`AgentEvent::ThinkingDelta`、
robit-web `thinking_delta` wire 帧、R6 空白 text_delta 发射端收口；连带
rusqlite 0.40→0.41 对齐，zimu-agent-cli 穷举 match 补臂）。
CLI 的 ThinkingDelta 以 ANSI 暗体流式渲染（调试 REPL 兼作链路人工核验面）。
上一版 0.1.27（2026-10-31，回路第二轮：子木
R2–R5 宿主形态需求 → robit `feat/host-ergonomics-r2-r5` 落地发布）。子木切换情况：
**R3 日志可组合原语已消费**——`build_log_filter(host_targets)` + `log_file_layer` +
`install_panic_hook`，server/cli 单一初始化路径（删「有无 robit 配置」二选一分支，
宿主 workspace 全 crate 日志可见，CLI 顺带修正自身 crate 未被放行的旧 bug）；
**控制台时间戳已对齐本地时间**（2026-11-23：宿主 fmt 层复用 robit 公开
`local_utc_offset()` + 与 robit `local_timer` 同格式的 `OffsetTime`，与文件层逐位
一致——此前文件层已是本地时间而控制台缺省 UTC；NAS 容器 TZ=Asia/Shanghai）；
**R4 额外技能目录已消费**——`load_zimu_skills` 内部切 `load_skills_from_dirs`；
**R5 有意不消费**（存储错误维持脱敏话术不用 cause 链直传）；**R2 经 ADR-0014
整体推翻**（2026-11-14：标准工具面全量消费——`create_tools_from_config_with`
内核 + 领域注册，skill 单源主权保留；确认关闭自动执行 `auto_approve=true`，
信任边界扩张明示于 ADR-0014）——决策记录见 `docs/07-backlog.md` R 系列与代码注释。第一轮回路（R1/robit-web，
v0.1.26，2026-10-30）：git pin → registry 切换收口。后续升级版本前先对齐 robit
发布节奏（约定见 CLAUDE.md）；开发期可临时改回
`git = "https://github.com/leexgone/robit", tag/rev = …` 消费未发布改动。

前置：`~/.robit/config.toml` 已配置 OpenAI 兼容 provider（本机为阿里云网关
DashScope compatible-mode 的 qwen），api_key 可写 `${ENV_VAR}` 并从
`~/.robit/.env` 解析——密钥永不入库。持仓库与 sidecar 地址走与 server 相同的
`ZIMU_DB_PATH` / `ZIMU_MARKET_URL`（sidecar 不在也能起，行情工具如实报错）。

```powershell
cargo run -p zimu-agent-cli
# > /复盘                ← 触发盘后复盘 Skill（P4-T6）
# > /建议 600519         ← 建议生成：calc_advice 出数 → 文案 → finalize_advice 归档
# > 请调用 echo 工具回显：子木集成验证   ← 链路验证（P2-T2 起）
```

P4-T6 起 CLI 与 server 是**同一份工具面与 Skill**（`zimu-agent::assembly::AgentCore`：
robit 标准面内核 + `tools::register_domain_tools` + `skills::load_zimu_skills`，
ADR-0014），差别只在 Frontend（CliFrontend vs WebFrontend）。
运行时会话库/记忆写在 `<cwd>/.robit/memory/`（已在仓库 `.gitignore` 忽略）。

## zimu-domain（P3-T3 领域纯函数库）

数字口径（`docs/03-domain-rules.md` 正文 + 附录）在 Rust 侧的唯一实现处：
手数/数量校验/快捷档位、费用、摊薄试算、recalc 汇总、纪律四条 + 降仓数量、
净值指标、波动合成、涨跌停（比例推断 / 整数单元限价 / 委托价校验，P1-T2
定稿）、**分析结论打分**（`advice`，P4-T6：03 §10 生产口径——信号票制、
中性维 60、70/55/45 三带、纪律拦截优先、target/stop 锚现价 half-up；ADR-0007
「数字归规则」的唯一实现，TS 共享层镜像 `frontend/src/domain/advice.ts`）、
**预警判定**（`alert`，P4-T7：03 §12——8 类判定序、含边界压线语义、数据缺失
不参与不伪造、text 展示层模板；镜像 `frontend/src/domain/alert.ts`，自然日差
用 days_from_civil 纯算不引 chrono）——
全部纯函数无 I/O，输入输出 serde 结构体的字段名与
`fixtures/golden/*.json` 键一一对应。测试即验收：

```powershell
cargo test -p zimu-domain   # 29 个单测 + 黄金向量 runner（10 组 108 用例；pending 组自动跳过，现全部 active）
```

改口径的顺序：**先改 `docs/03` 与 fixtures，再改实现**。TS 共享层
（P3-T4 ✅：`frontend/src/domain/`，`npm test` 跑 vitest）跑同一份
fixtures，两边实现永不互相「对拍」，只对黄金向量。
`price_limit` 组已随 P1-T2 定稿（2026-10-28）**两侧同步**补齐实现与用例。

## 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `ZIMU_ADDR` | `127.0.0.1:8080` | 监听地址；LAN 部署可改 `0.0.0.0:8080`，公网入站默认禁止（ADR-0005） |
| `ZIMU_STATIC_DIR` | `frontend/dist`（按编译期 manifest 定位） | 静态目录，部署时可指向任意构建产物 |
| `ZIMU_DB_PATH` | `zimu.db`（启动目录，即 `backend/`） | 持仓 SQLite 路径；`.db` 已被 .gitignore 覆盖，真实数据永不入库（C9/ADR-0005） |
| `ZIMU_MARKET_URL` | `http://127.0.0.1:8300` | 行情 sidecar 基地址（P4-T4；不可达时行情端点降级 503，服务照常） |
| `ZIMU_MARKET_TIMEOUT_MS` | `10000` | sidecar 请求超时（毫秒） |
| `ZIMU_SKILLS_DIR` | `仓库根/skills`（按编译期 manifest 定位） | Skill 模板目录（P4-T6；每子目录一个 SKILL.md，robit 约定） |
| `ZIMU_SCHEDULER_DISABLE` | 未设置（调度器随服务启动） | 设任意值停用调度与预警引擎（P4-T7，开发/测试用；手动 refresh 与 agent 工具不受影响） |
| `RUST_LOG` | 未设置时按 robit config `log_level`（缺省 info，含 tower_http=debug 与 `zimu_agent`/`zimu_market` 同级放行） | 设置了则优先于 robit 配置；有 robit 配置时日志由 `init_logging` 接管（控制台 + `<cwd>/.robit/logs`），无配置时回落服务自己的 tracing_subscriber。**注意**：robit 的过滤器只放行 target crate 与 robit 系 target，宿主其余 crate 必须走附加 directive 显式放行（P4-T7 已接 `zimu_agent={level}` `zimu_market={level}`——调度器日志否则被默认底线静默吞掉，R3，见 `docs/07-backlog.md`） |

## 目录

```text
Cargo.toml                  workspace（robit crates 消费 crates.io 0.5.1 正式版 + robit-wechat，其余依赖版本与 robit workspace 对齐）
crates/zimu-server/         主服务：health + 静态托管 + Agent SSE 会话 + 持仓录入/直接维护 API + 行情 API + 预警/快照/留痕 API + 建议留档 API + 自选 API + OpenAPI 契约（Ctrl+C 优雅关闭）
                            ├─ lib.rs            /api/* 业务路由装配（api_router + /api/openapi.json，集成测试同一入口）
                            ├─ main.rs           启动接线（store/market 先于 Agent 装配——工具与 HTTP 同句柄；调度器 spawn + 复盘回调接 WebBridge，P4-T7；robit 配置缺失时优雅降级；bin 侧契约键集单测）
                            ├─ agent_bridge.rs   Agent 装配（工具面/Skill 来自 zimu-agent；WebBridge 接线）
                            ├─ channel.rs        ChannelHandle（QQ/微信 supervisor 共享句柄：request_stop + join WAL flush，ADR-0017）
                            ├─ qq_channel.rs     QQ 通道 supervisor（T5：退避重建 + 占位符凭据闸门；ActiveAdapter 泛型槽别名）
                            ├─ wechat_channel.rs 微信通道 supervisor（ADR-0017：iLink Bot + token 失效终态 60s 探测恢复 + 凭据闸门）
                            ├─ channel_push.rs   caps 参数化推送链（lib：ActiveAdapter<T> + push_to_bound<T> + 文案组装 + 复合 pusher，ADR-0017 D5）
                            ├─ channel_adapter.rs BoundOnly 入站白名单装饰器（lib，通道无关；fail-closed）
                            ├─ channel_bindings.rs /api/channel_bindings GET/POST/DELETE（lib，绑定关系界面管理拍板⑤）
                            ├─ testutil.rs       lib 单测共享装置（MockPlatform 宏 + 临时 store，#[cfg(test)]）
                            ├─ holdings_api.rs   /api/holdings* 录入流程（P4-T1）+ 直接维护 PUT/DELETE + /api/holding_edits 留痕读取（ADR-0009）；错误→状态码映射，utoipa::path 契约标注
                            ├─ market_api.rs     /api/market/*（P4-T4：status/quote/kline/cb_list/calendar；refresh 处理器 P4-T6 起薄壳调 zimu-agent::service）
                            ├─ alerts_api.rs     /api/alerts* /api/equity_snapshots /api/push_logs（P4-T7 + P5-T2：调度引擎落库内容的读取与归档，不生成不改写）
                            ├─ advice_api.rs     /api/advice（P5-T2：建议留档读取；写入在 agent 工具链，ADR-0007）
                            ├─ watchlist_api.rs  /api/watchlist*（P5-T2，B13：自选 CRUD，upsert 幂等；行情不回写自选表）
                            ├─ openapi.rs        OpenAPI 3.1 汇编（P4-T5：ApiDoc + ApiError/Health 等共享契约类型 + 再生成入口）
                            └─ tests/            market_api.rs 8 项集成（口径等价 + 降级）· contract.rs 3 项契约（文档新鲜度 + 实时响应/28 份样例过 schema）· common/ 共享装置（FakeApi/seed）
crates/zimu-agent/          agent 应用层（P4-T6/T7）：领域工具 + Skill 加载 + 应用服务 + 调度预警引擎（server/CLI 共用）
                            ├─ lib.rs            AgentDeps（store + market + 日历缓存）与 crate 边界说明
                            ├─ tools/            领域 14 件（mod.rs register_domain_tools；market/portfolio/advice 三组，portfolio 含 ADR-0012/0013 写账本五件；robit 标准面经 assembly 内核叠加，ADR-0014）
                            ├─ service.rs        refresh_portfolio 刷新链（HTTP 与工具同链；P4-T7 增 detailed 变体供调度器复用——多回 quote 原始行，wire 契约不变）+ SkippedRow/RefreshResponse（server 再导出，契约组件名不变）
                            ├─ scheduler.rs      调度与预警引擎（P4-T7 + B13，03 §12 纯编排：tick(now) 时刻注入可测、盘中轮询判定落 alerts（持仓 + 自选无持仓形态）、日级缓存（kline 量窗口/cb_list）、15:05 快照 + /复盘 触发 + push_log）
                            ├─ skills.rs         Skill 目录解析（ZIMU_SKILLS_DIR → 仓库根 skills/）+ robit loader 接线
                            └─ tests/            tools.rs 20 项（黄金向量端到端/留档/降级/Skill 完整性/写账本九例/注册面组成）· scheduler.rs 9 项（模拟盘中触发/去重/空转/降级/盘后链幂等/自选无持仓形态/重叠不双判，全离线）
crates/zimu-agent-cli/      Agent 调试 REPL（P2-T2 起；P4-T6 与 server 同工具面/Skill）
crates/zimu-domain/         领域纯函数库（P3-T3）：lot/fees/dilution/recalc/discipline/perf/vol_synth/price_limit/advice（P4-T6 打分）/alert（P4-T7 预警判定）
                            └─ tests/golden.rs     黄金向量 runner（读仓库根 fixtures/golden/，10 组 108 用例）
crates/zimu-store/          存储层（P4-T1/T2/T7 + B13 + ADR-0009）：SQLite 八表（v2 增 watchlist，v3 增 holding_edits 维护留痕）+ user_version 迁移 + 录入事务 + 直接维护（upsert_holding/remove_holding/holding_edits——快照校正：不动现金、不写 trades、留痕同事务、幂等 no-op）+ 行情回写（apply_quotes）+ 建议留档（insert/finalize/list_advice，P4-T6）+ 预警/快照/推送日志（insert_alert/alert_exists_on 去重/mark_alert_done、upsert_equity_snapshot 幂等/from_agg、insert_push_log，P4-T7）+ 自选 CRUD（add/remove/watchlist，upsert 幂等）
                            └─ tests/store.rs      34 项集成测试（含重启不丢数据、COALESCE 回写、留档数字不可经文案回填改写、预警同日去重与归档、快照 upsert 幂等、watchlist upsert/校验、直接维护六项（奇零股/幂等/行情字段保留/三不动/删除留痕/坏输入）、v1→v3 迁移）
crates/zimu-market/         行情适配器（P4-T4）：sidecar 唯一消费入口
                            ├─ client.rs         MarketApi trait（注入点）+ HttpMarketClient（错误三分类映射）
                            ├─ models.rs         类型化契约（quote/kline/cb_list/calendar/healthz/错误体）
                            ├─ clock.rs          市场时钟纯函数（03 §9 生产口径：恒 +8、日历优先、启发式降级）
                            ├─ calendar.rs       日历缓存（1h；sidecar 挂后退旧缓存再退启发式）
                            ├─ synth.rs          盘中当日 bar 合成（04 附录 B）
                            └─ symbols.rs        B7 前缀拼装（显式 mkt → sh/sz，拼不出即拒绝）
```
