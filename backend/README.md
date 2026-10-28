# backend · 子木 Rust 服务

ADR-0005 定案：axum REST + SSE 托管 React SPA，内网部署，默认不开公网入站。
当前包含：`zimu-server`（主服务：`/api/health` + `frontend/dist` 静态托管 +
Agent SSE 会话 + 持仓录入 API，P2-T3/P4-T1；P2-T5 起桥体消费 robit `robit-web`
crate）、`zimu-agent-cli`（P2-T2 robit 集成 spike：最小 Agent REPL + echo 工具，
保留为调试入口）、`zimu-domain`（P3-T3 领域纯函数库：`docs/03` 全部数字口径的
Rust 实现，`cargo test -p zimu-domain` 跑 `fixtures/golden/` 黄金向量）与
`zimu-store`（P4-T1/T2 存储层：SQLite schema + 迁移 + 录入事务，校验复用
zimu-domain，`cargo test -p zimu-store`）。

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
提供）。宿主侧只剩 axum 接线（SSE 编码、`connected` 首帧、POST → `WebBridge`
路由，见 `main.rs`）与 Agent 装配 + echo 工具（`agent_bridge.rs`）。找不到
robit 配置时服务照常起、静态首页可用，仅 `/api/chat*` 降级 503。spike 边界：
进程级单会话，多会话与持久化留给 P4+（robit-web 约定单会话一桥）。

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

## zimu-store（P4-T2 存储层）

SQLite 单文件（C9 已决：rusqlite bundled，备份 = 拷文件；`.db` 在 `.gitignore`，
**真实持仓永不入 git**）。v1 schema 六表：`account`（单例行，现金源头）、
`holdings`（`shares` 为普通整数——奇零股存得下，B12 存储前提）、`trades`（费用三分项
留档可审计；`holding_code` **不是外键**——流水必须在清仓后仍留档）、
`advice_history`（`generated_at + data_version`，B9 正式解，P4-T6 起写入）、
`alerts`、`equity_snapshots`（15:05 调度写入是 P4-T7）、`push_log`。

迁移机制：`PRAGMA user_version` 逐版前进，每版一个事务；已发布版本的 SQL 永不修改，
新迁移在 `MIGRATIONS` 尾部追加。rusqlite 版本必须与 robit-agent 对齐（0.40）——
`libsqlite3-sys` 的 links 约束全依赖图只允许一份 libsqlite3。

```powershell
cargo test -p zimu-store   # 16 项：录入事务/校验拒绝/回滚/重启不丢数据
```

`Connection` 非 `Sync`：zimu-server 用 `Arc<Mutex<Store>>` 串行化，录入是同步短事务，
处理跨 `.await` 持锁被禁止（当前持仓处理器无 await）。

## 运行 zimu-agent-cli（P2-T2 spike）

robit 全家 pin 到 tag `v0.1.26`（P2-T5 共同演进演练：`git = "file:///E:/Github/robit"`，
含首发的 `robit-web`）。**尾巴**：robit 分支 `feat/web-bridge` 合并推送、tag 推送、
crates.io 发布（robit 仓库 `publish.ps1`）之后，把 `backend/Cargo.toml` 三行 git
pin 换回 registry 写法（`robit-ai = "0.1.26"` 等）并重新 `cargo build` 验证。

前置：`~/.robit/config.toml` 已配置 OpenAI 兼容 provider（本机为阿里云网关
DashScope compatible-mode 的 qwen），api_key 可写 `${ENV_VAR}` 并从
`~/.robit/.env` 解析——密钥永不入库。

```powershell
cargo run -p zimu-agent-cli
# > 请调用 echo 工具回显：子木集成验证   ← 模型应发起 echo function call
```

验证点：模型流式对话可用；`echo` 工具被 function-calling 正确调用并回显。
运行时会话库/记忆写在 `<cwd>/.robit/memory/`（已在仓库 `.gitignore` 忽略）。
P2-T3 把会话搬进 zimu-server 时，替换的只是 CliFrontend（换成 SSE 桥），
装配方式（LlmClient / ToolRegistry / Agent）原样复用。

## zimu-domain（P3-T3 领域纯函数库）

数字口径（`docs/03-domain-rules.md` 正文 + 附录）在 Rust 侧的唯一实现处：
手数/数量校验/快捷档位、费用、摊薄试算、recalc 汇总、纪律四条 + 降仓数量、
净值指标、波动合成、涨跌停（比例推断 / 整数单元限价 / 委托价校验，P1-T2
定稿）——全部纯函数无 I/O，输入输出 serde 结构体的字段名与
`fixtures/golden/*.json` 键一一对应。测试即验收：

```powershell
cargo test -p zimu-domain   # 11 个单测 + 黄金向量 runner（76 用例；pending 组自动跳过，现全部 active）
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
| `RUST_LOG` | 未设置时按 robit config `log_level`（缺省 info，含 tower_http=debug） | 设置了则优先于 robit 配置；有 robit 配置时日志由 `init_logging` 接管（控制台 + `<cwd>/.robit/logs`），无配置时回落服务自己的 tracing_subscriber（R3，见 `docs/07-backlog.md`） |

## 目录

```text
Cargo.toml                  workspace（robit crates pin 本地 git tag v0.1.26，其余依赖版本与 robit workspace 对齐）
crates/zimu-server/         主服务：health + 静态托管 + Agent SSE 会话 + 持仓录入 API
                            ├─ main.rs           端点与启动装配（robit 配置缺失时优雅降级）
                            ├─ agent_bridge.rs   Agent 装配 + echo 工具（WebBridge 接线）
                            └─ holdings_api.rs   /api/holdings* 录入流程（P4-T1，错误→状态码映射）
crates/zimu-agent-cli/      robit 集成 spike：Agent REPL + echo 工具（P2-T2，调试入口）
crates/zimu-domain/         领域纯函数库（P3-T3）：lot/fees/dilution/recalc/discipline/perf/vol_synth/price_limit
                            └─ tests/golden.rs     黄金向量 runner（读仓库根 fixtures/golden/）
crates/zimu-store/          存储层（P4-T1/T2）：SQLite 六表 + user_version 迁移 + 录入事务（校验复用 zimu-domain）
                            └─ tests/store.rs      16 项集成测试（含重启不丢数据）
```
