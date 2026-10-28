# backend · 子木 Rust 服务

ADR-0005 定案：axum REST + SSE 托管 React SPA，内网部署，默认不开公网入站。
当前包含：`zimu-server`（主服务：`/api/health` + `frontend/dist` 静态托管 +
Agent SSE 会话 + 持仓录入 API + 行情 API + OpenAPI 契约，P2-T3/P4-T1/P4-T4/P4-T5；
P2-T5 起桥体消费 robit `robit-web` crate）、`zimu-agent`（P4-T6 agent 应用层：
8 个领域工具 + Skill 模板加载 + 与 HTTP API 共用的刷新链服务，server 与 CLI
共同消费）、`zimu-agent-cli`（Agent 调试 REPL：P2-T2 spike 起，P4-T6 换装与
server 相同的领域工具面与 Skill）、`zimu-domain`（P3-T3 领域纯函数库：`docs/03`
全部数字口径的 Rust 实现，含 P4-T6 打分模块 `advice`，`cargo test -p zimu-domain`
跑 `fixtures/golden/` 黄金向量）、`zimu-store`（P4-T1/T2 存储层：SQLite schema +
迁移 + 录入事务 + 建议留档，校验复用 zimu-domain，`cargo test -p zimu-store`）与
`zimu-market`（P4-T4 行情适配器：datafetch sidecar 的**唯一消费入口**——类型化
契约、降级三分类、市场时钟、当日 bar 合成，`cargo test -p zimu-market`）。

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

## 会话端点（P2-T3 SSE 桥）

Agent 随服务启动装配（与 zimu-agent-cli 同一链路，`CliFrontend` 换成 robit-web
的 `WebFrontend`：`WebEvent` broadcast 扇出 + 单槽位确认，P2-T5 起由 robit
提供）。宿主侧只剩 axum 接线（SSE 编码、`connected` 首帧——P4-T6 起含 `skills`
键、POST → `WebBridge` 路由，见 `main.rs`）与 Agent 装配（`agent_bridge.rs`）；
工具面与 Skill 模板在 `zimu-agent` crate（见下节）。找不到 robit 配置时服务
照常起、静态首页可用，仅 `/api/chat*` 降级 503。spike 边界：进程级单会话，
多会话与持久化留给后续（robit-web 约定单会话一桥）。

| 端点 | 说明 |
|---|---|
| `GET /api/chat/stream` | SSE 事件流：首帧 `connected`，随后 `text_delta` / `tool_call` / `tool_result` / `confirm_request` / `turn_complete` / `error`（形状见 `agent_bridge.rs` 的 `SseEvent`） |
| `POST /api/chat` | `{"text":"…"}` → 202，回复经 SSE 流出 |
| `POST /api/chat/confirm` | `{"tool_call_id":"…","approved":true}` 回填工具确认 |
| `POST /api/chat/cancel` | 取消当前轮次 |

「智能问答」视图的会话面板（`frontend/src/components/ChatPanel.tsx`，工作台首页点
侧栏「智能问答 / 问一句」或对应入口卡进入，P2-T4 起）即消费这组端点；
它是 spike 级 UI，正式 chat 屏 P5 按 `design/chat.html` 重做。

curl 冒烟——**Windows 终端里中文 payload 必须走 UTF-8 文件**，内联会被编码搞坏
（服务端报 `invalid unicode code point`）：

```powershell
'{"text":"请调用 echo 工具回显:你好子木"}' | Out-File -Encoding utf8NoBOM payload.json
curl.exe -sN http://127.0.0.1:8080/api/chat/stream   # 另开窗口看事件流
curl.exe -s -X POST http://127.0.0.1:8080/api/chat -H "Content-Type: application/json" --data-binary "@payload.json"
```

## zimu-agent（P4-T6 agent 应用层：领域工具 + Skill 模板）

server（SSE 会话）与 CLI（调试 REPL）共用的公共 crate——P2 期 echo 工具两处
重复的债在此收敛。边界：**数字口径只在 `zimu-domain`**，工具只做取数
（zimu-store / zimu-market）→ 调领域函数 → 组装 JSON；行情只经 `zimu-market`
（sidecar 唯一消费入口）；**没有下单/录入类工具**——程序只给建议不代下单，
`simulate_trade` 只试算不落库。

| 工具 | 性质 | 说明 |
|---|---|---|
| `market_status` | 只读 | 市场时钟（03 §9）+ sidecar 在线 + 日历可用性；永不失败 |
| `get_quote` | 只读 | 实时行 + 按 03 §2 拼装 `limit_up/limit_down`（domain price_limit）；裸代码查持仓 mkt，非持仓必须带 sh/sz 前缀（B7） |
| `get_holdings` | 只读 | 组合全景（store.portfolio → recalc）+ 纪律四条 |
| `calc_discipline` | 只读 | 03 §6 四条体检；单票超限附 `reduce_hint`（sell_to 降仓股数） |
| `simulate_trade` | 只读 | 试算（费用/摊薄/整手/涨跌停拦截，03 §1–§4），不落库 |
| `refresh_quotes` | 写行情字段 | 与 `POST /api/market/refresh` **同一条链**（`service::refresh_portfolio`，P4-T6 抽取，契约 wire 零变化） |
| `calc_advice` | 写 advice_history 数字列 | `zimu-domain::advice::score_advice`（03 §10 生产口径）出数 → 数字列直接留档（B9：`generated_at` + `data_version=adv-v1\|k=…\|q=…`）→ 返回 advice_id 与全部数字；日线拿不到**拒绝出建议** |
| `finalize_advice` | 写 advice_history 文案列 | 模型文案只回填 reason/risks（risks 至少一条，03 §10）——**数字不经模型转写**（ADR-0007 结构性保证） |
| `echo` | 只读 | P2-T2 链路验证保留（网关排查不依赖数据） |

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
cargo test -p zimu-agent   # 10 项集成（全离线）：黄金向量 av-bull-add 同款输入端到端
                           #（日线 24 根 + 当日 bar 实时合成 → score 73/conf 0.9/加仓 → 留档逐位一致）、
                           # 文案回填不动数字、risks 空被拒、B7 裸代码拒绝、限价与 domain 同源、
                           # sidecar 挂时 refresh 报错不写库 / market_status 不瘫、Skill 触发词唯一 + tools_required 已注册
```

注意：工具输出经 JSON 文本往返后与内存值可差 1 ulp（serde_json 浮点解析
fast path 的双舍入），测试比对与 03 附录同款按容差（1e-9），不做 Value 精确相等。

## 持仓录入 API（P4-T1，手工逐笔先行）

录入流程的后端半边（ADR-0006：手工逐笔先行，CSV 导入是 P4 后续增强单独立项）。
校验与派生**全部走 `zimu-domain` 纯函数**（整手、涨跌停、费用、摊薄、仓位、
现金充足性——即黄金向量锁死的那套口径），`zimu-store` 只负责事务组织：
`BEGIN IMMEDIATE` → 校验 → 试算 → 持仓/流水/现金三写 → `COMMIT`，任一步失败整体回滚。

| 端点 | 说明 |
|---|---|
| `GET /api/holdings` | 组合全景：存储行 + `recalc()` 的逐笔派生（`per`）与汇总（`agg`）、账户现金 |
| `POST /api/holdings/trade` | `{"code","side":"buy\|sell","qty","price","note","meta":{...}}` → 201 收据（trade 行 + 变动后持仓 + 试算预览 + warnings）；卖出与已有标的加仓可不带 `meta` |
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
| `POST /api/market/refresh` | 拉全部持仓实时行情 → 回写 `price/prev/name` + `limit_pct` 推断（仅未配置时，ETF 不推断）→ `recalc()` 全量重算并返回组合全景；无行情标的（停牌/no_data）保持库存旧值并进 `skipped` |

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
zimu-market / zimu-server 四 crate），`src/openapi.rs` 的 `ApiDoc`（utoipa 5）
汇编成 OpenAPI 3.1 → **`docs/api/openapi.json`**（36 schema、16 路径；文档
永不手改）。前端 `npm run gen:api`（openapi-typescript）生成
`src/api/schema.d.ts`，P5-T1 的 API client 直接消费。运行时
`GET /api/openapi.json` 提供同一文档。

契约测试（验收「样例响应过 schema 校验」，`tests/contract.rs` + jsonschema
draft 2020-12）三连：

1. 仓库 `docs/api/openapi.json` 与代码生成**逐字节一致**（改契约忘了再生成
   会直接红）；
2. seeded 测试服务（FakeApi 注入，`tests/common/` 共享装置）全端点**实时
   响应**过其声明的响应 schema，含 422/404/400/503 错误契约；
3. **`fixtures/api/` 15 份提交样例**过同一校验（样例永不腐烂，前端可当 mock）。

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
不可改写，ADR-0007）、`alerts`、`equity_snapshots`（15:05 调度写入是 P4-T7）、`push_log`。

迁移机制：`PRAGMA user_version` 逐版前进，每版一个事务；已发布版本的 SQL 永不修改，
新迁移在 `MIGRATIONS` 尾部追加。rusqlite 版本必须与 robit-agent 对齐（0.40）——
`libsqlite3-sys` 的 links 约束全依赖图只允许一份 libsqlite3。

```powershell
cargo test -p zimu-store   # 23 项：录入事务/校验拒绝/回滚/重启不丢数据/行情回写（P4-T4）/建议留档（P4-T6）
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

robit 全家 pin 到 tag `v0.1.26`（P2-T5 共同演进演练：`git = "file:///E:/Github/robit"`，
含首发的 `robit-web`）。**尾巴**：robit 分支 `feat/web-bridge` 合并推送、tag 推送、
crates.io 发布（robit 仓库 `publish.ps1`）之后，把 `backend/Cargo.toml` 三行 git
pin 换回 registry 写法（`robit-ai = "0.1.26"` 等）并重新 `cargo build` 验证。

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

P4-T6 起 CLI 与 server 是**同一份工具面与 Skill**（`zimu-agent::tools::build_registry`
+ `skills::load_zimu_skills`），差别只在 Frontend（CliFrontend vs WebFrontend）。
运行时会话库/记忆写在 `<cwd>/.robit/memory/`（已在仓库 `.gitignore` 忽略）。

## zimu-domain（P3-T3 领域纯函数库）

数字口径（`docs/03-domain-rules.md` 正文 + 附录）在 Rust 侧的唯一实现处：
手数/数量校验/快捷档位、费用、摊薄试算、recalc 汇总、纪律四条 + 降仓数量、
净值指标、波动合成、涨跌停（比例推断 / 整数单元限价 / 委托价校验，P1-T2
定稿）、**分析结论打分**（`advice`，P4-T6：03 §10 生产口径——信号票制、
中性维 60、70/55/45 三带、纪律拦截优先、target/stop 锚现价 half-up；ADR-0007
「数字归规则」的唯一实现，TS 共享层镜像 `frontend/src/domain/advice.ts`）——
全部纯函数无 I/O，输入输出 serde 结构体的字段名与
`fixtures/golden/*.json` 键一一对应。测试即验收：

```powershell
cargo test -p zimu-domain   # 16 个单测 + 黄金向量 runner（9 组 86 用例；pending 组自动跳过，现全部 active）
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
| `RUST_LOG` | 未设置时按 robit config `log_level`（缺省 info，含 tower_http=debug） | 设置了则优先于 robit 配置；有 robit 配置时日志由 `init_logging` 接管（控制台 + `<cwd>/.robit/logs`），无配置时回落服务自己的 tracing_subscriber（R3，见 `docs/07-backlog.md`） |

## 目录

```text
Cargo.toml                  workspace（robit crates pin 本地 git tag v0.1.26，其余依赖版本与 robit workspace 对齐）
crates/zimu-server/         主服务：health + 静态托管 + Agent SSE 会话 + 持仓录入 API + 行情 API + OpenAPI 契约
                            ├─ lib.rs            /api/* 业务路由装配（api_router + /api/openapi.json，集成测试同一入口）
                            ├─ main.rs           启动接线（store/market 先于 Agent 装配——工具与 HTTP 同句柄；robit 配置缺失时优雅降级；bin 侧契约键集单测）
                            ├─ agent_bridge.rs   Agent 装配（工具面/Skill 来自 zimu-agent；WebBridge 接线）
                            ├─ holdings_api.rs   /api/holdings* 录入流程（P4-T1，错误→状态码映射；utoipa::path 契约标注）
                            ├─ market_api.rs     /api/market/*（P4-T4：status/quote/kline/cb_list/calendar；refresh 处理器 P4-T6 起薄壳调 zimu-agent::service）
                            ├─ openapi.rs        OpenAPI 3.1 汇编（P4-T5：ApiDoc + ApiError/Health 等共享契约类型 + 再生成入口）
                            └─ tests/            market_api.rs 8 项集成（口径等价 + 降级）· contract.rs 3 项契约（文档新鲜度 + 实时响应/样例过 schema）· common/ 共享装置（FakeApi/seed）
crates/zimu-agent/          agent 应用层（P4-T6）：领域工具 + Skill 加载 + 应用服务（server/CLI 共用）
                            ├─ lib.rs            AgentDeps（store + market + 日历缓存）与 crate 边界说明
                            ├─ tools/            8 领域工具 + echo（mod.rs 注册器；market/portfolio/advice 三组）
                            ├─ service.rs        refresh_portfolio 刷新链（HTTP 与工具同链）+ SkippedRow/RefreshResponse（server 再导出，契约组件名不变）
                            ├─ skills.rs         Skill 目录解析（ZIMU_SKILLS_DIR → 仓库根 skills/）+ robit loader 接线
                            └─ tests/tools.rs    10 项集成（黄金向量端到端/留档/降级/Skill 完整性，全离线）
crates/zimu-agent-cli/      Agent 调试 REPL（P2-T2 起；P4-T6 与 server 同工具面/Skill）
crates/zimu-domain/         领域纯函数库（P3-T3）：lot/fees/dilution/recalc/discipline/perf/vol_synth/price_limit/advice（P4-T6 打分）
                            └─ tests/golden.rs     黄金向量 runner（读仓库根 fixtures/golden/，9 组 86 用例）
crates/zimu-store/          存储层（P4-T1/T2）：SQLite 六表 + user_version 迁移 + 录入事务 + 行情回写（apply_quotes）+ 建议留档（insert/finalize/list_advice，P4-T6）
                            └─ tests/store.rs      23 项集成测试（含重启不丢数据、COALESCE 回写、留档数字不可经文案回填改写）
crates/zimu-market/         行情适配器（P4-T4）：sidecar 唯一消费入口
                            ├─ client.rs         MarketApi trait（注入点）+ HttpMarketClient（错误三分类映射）
                            ├─ models.rs         类型化契约（quote/kline/cb_list/calendar/healthz/错误体）
                            ├─ clock.rs          市场时钟纯函数（03 §9 生产口径：恒 +8、日历优先、启发式降级）
                            ├─ calendar.rs       日历缓存（1h；sidecar 挂后退旧缓存再退启发式）
                            ├─ synth.rs          盘中当日 bar 合成（04 附录 B）
                            └─ symbols.rs        B7 前缀拼装（显式 mkt → sh/sz，拼不出即拒绝）
```
