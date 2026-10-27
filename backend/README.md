# backend · 子木 Rust 服务（最简骨架）

ADR-0005 定案：axum REST + SSE 托管 React SPA，内网部署，默认不开公网入站。
当前是**最简运行框架**（启动服务 → 浏览器进首页）：只有 `/api/health` 与
`frontend/dist` 静态托管。robit agent 集成在 P2-T2，SSE 事件流在 P2-T3，
领域纯函数库（P3-T3）将来作为本 workspace 的独立 crate 加入。

## 运行

生产形态（单服务单端口，前端构建产物由后端托管）：

```powershell
cd ..\frontend
npm install          # 首次
npm run build        # 产出 frontend/dist
cd ..\backend
cargo run            # → http://127.0.0.1:8080
```

开发形态（前端热更新，两个进程）：

```powershell
cargo run                              # 后端 :8080
cd ..\frontend; npm run dev            # 前端 :5173，/api 已反代到 :8080
```

浏览器访问 `http://127.0.0.1:5173`（开发）或 `http://127.0.0.1:8080`（生产形态）。

## 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `ZIMU_ADDR` | `127.0.0.1:8080` | 监听地址；LAN 部署可改 `0.0.0.0:8080`，公网入站默认禁止（ADR-0005） |
| `ZIMU_STATIC_DIR` | `frontend/dist`（按编译期 manifest 定位） | 静态目录，部署时可指向任意构建产物 |
| `RUST_LOG` | `info,tower_http=debug` | 日志级别 |

## 目录

```text
Cargo.toml                  workspace（依赖版本与 robit 对齐，避免 P2-T2 集成时依赖树分裂）
crates/zimu-server/         主服务：health + 静态托管（骨架）
```
