# backend · 子木 Rust 服务

ADR-0005 定案：axum REST + SSE 托管 React SPA，内网部署，默认不开公网入站。
当前包含：`zimu-server`（主服务：`/api/health` + `frontend/dist` 静态托管 +
Agent SSE 会话，P2-T3）与 `zimu-agent-cli`（P2-T2 robit 集成 spike：最小 Agent
REPL + echo 工具，保留为调试入口）。领域纯函数库（P3-T3）将来作为独立 crate 加入。

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

Agent 随服务启动装配（与 zimu-agent-cli 同一链路，`CliFrontend` 换成
`SseFrontend`：broadcast 扇出 + 确认槽位，见 `agent_bridge.rs`）。找不到 robit
配置时服务照常起、静态首页可用，仅 `/api/chat*` 降级 503。spike 边界：进程级
单会话，多会话与持久化留给 P4+。

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

## 运行 zimu-agent-cli（P2-T2 spike）

robit 核心组件直接消费 crates.io 发布版（`robit-ai` / `robit-agent` 0.1.25，
与 robit 仓库 tag 一致，无需 path override）。

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

## 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `ZIMU_ADDR` | `127.0.0.1:8080` | 监听地址；LAN 部署可改 `0.0.0.0:8080`，公网入站默认禁止（ADR-0005） |
| `ZIMU_STATIC_DIR` | `frontend/dist`（按编译期 manifest 定位） | 静态目录，部署时可指向任意构建产物 |
| `RUST_LOG` | 未设置时按 robit config `log_level`（缺省 info，含 tower_http=debug） | 设置了则优先于 robit 配置；有 robit 配置时日志由 `init_logging` 接管（控制台 + `<cwd>/.robit/logs`），无配置时回落服务自己的 tracing_subscriber（R3，见 `docs/07-backlog.md`） |

## 目录

```text
Cargo.toml                  workspace（robit crates 消费 crates.io 发布版，其余依赖版本与 robit workspace 对齐）
crates/zimu-server/         主服务：health + 静态托管 + Agent SSE 会话（P2-T3）
                            ├─ main.rs           端点与启动装配（robit 配置缺失时优雅降级）
                            └─ agent_bridge.rs   SseFrontend / SseEvent / echo 工具 / Agent 装配
crates/zimu-agent-cli/      robit 集成 spike：Agent REPL + echo 工具（P2-T2，调试入口）
```
