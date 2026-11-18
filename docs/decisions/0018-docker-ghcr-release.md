# 0018 · 发布模式：Docker 双镜像 + GHCR + GitHub Actions tag 驱动流水线

- **状态**：接受
- **日期**：2026-11-17
- **决策者**：主人拍板（镜像仓库 / 架构 / 触发策略 / 首版号 / 版本线 / 部署目标 / 拓扑七项 + 智能体运行时依赖增补需求），Claude 协作探查与实施

## 背景

仓库已公开（github.com/leexgone/StockAgent，2026-11-17），产品形态 = 常驻 Web 服务 + QQ/微信 IM 入口（ADR-0005），天然适合容器化分发。现状零发布基建：无 `.github/`、无 Dockerfile/compose、零 git tag；版本线三处不齐（backend workspace 与 frontend 0.0.1、datafetch 0.1.0）。主人要求：**通过 Docker 发布程序，GitHub 自动流水线构建发布**。挂载点 = `06` P6-T3「部署与运维」。

探查已核实的容器化关键事实（实现直接采信）：

- **运行时几何**：working_dir = 启动 cwd（zimu-server main.rs:287-288）；人格/config/memory/logs/robit.db 在 `{cwd}/.robit/` 下，media/images/zimu.db 落 `{cwd}`；`ZIMU_STATIC_DIR` 与 `ZIMU_SKILLS_DIR` 缺省是**编译期 CARGO_MANIFEST_DIR 绝对路径**（main.rs:240、skills.rs:20-22）——二进制搬进容器必须 env 指回烘焙资产。
- **优雅关闭仅 tokio ctrl_c**（main.rs:260-281）：Linux 只收 SIGINT，`docker stop` 缺省 SIGTERM 不触发 graceful 路径。
- **凭据覆盖坑**：robit-ai 0.3.0 `load_env_from` 用 `std::env::set_var` **无条件覆盖**进程 env——卷内 `.robit/.env` 会静默压过 compose env_file 注入值。
- **构建依赖**：rusqlite bundled（cc）；TLS 全 rustls 无 OpenSSL；但 rustls 缺省 crypto aws-lc-rs → **aws-lc-sys 需 cmake + C 编译器**；robit 全家 crates.io 0.3.0 无 git pin。
- **arm64 硬阻塞**：`datafetch/uv.lock` 的 py-mini-racer 0.6.0 **只有 manylinux1_x86_64 wheel**（sdist 是 2021 V8 源码包），且是功能必需（sources.py:233 新浪 K 线 JS 解密、:335 集思录强赎）——arm64 上 `uv sync --frozen` 必失败。
- **版本 bump 连带**：openapi.rs `version = env!("CARGO_PKG_VERSION")`，契约测试逐字节守护 docs/api/openapi.json——bump 必须连带再生成。
- sidecar 是**独立外部 HTTP 进程**（端口 8300，纯内存缓存零落盘），server 缺它降级 503 不瘫（main.rs:339-347）。

## 决定

1. **D1 · 拓扑 = 双镜像 + docker compose**：`ghcr.io/leexgone/zimu`（server，含前端静态产物与 robit-wechat 登录 CLI）+ `ghcr.io/leexgone/zimu-datafetch`（行情 sidecar）；仓库根 `compose.yaml` 一键起（`docker compose up -d`）。与现有开发形态同构：进程隔离、独立重启/升级、sidecar 故障不拖死 server（降级设计原样保留）。
2. **D2 · 镜像仓库 = GHCR**：`ghcr.io/leexgone/zimu{,-datafetch}`，流水线用内置 `GITHUB_TOKEN`（`packages: write`）推送，**零额外 secrets**。公开仓库 → 镜像设公开后 `docker pull` 无需登录。首推 package 默认 private，需手动设公开（docs/08 首发布清单钉死）。
3. **D3 · 双架构 = linux/amd64 + linux/arm64，原生 runner 矩阵**：公开仓库免费 arm64 runner（`ubuntu-24.04-arm`），每架构原生构建（**不用 QEMU**，Rust 交叉模拟编译时长不可接受）；`docker/build-push-action` 按 digest 推送（`push-by-digest=true` + `name-canonical=true` + **`provenance: false`**——缺省 attestation 会生成 unknown/unknown 附加 manifest 干扰合并），manifest job 用 `buildx imagetools create` 合并打 `vX.Y.Z` + `latest`。gha cache 按 image×arch 分 scope。
4. **D4 · 触发 = tag 驱动 + 版本一致性守卫**：push `v*` → release workflow；main/PR 只跑 ci.yml 测试门禁不出镜像。首版 **v0.1.0**（pre-1.0：P6-T4 连续 5 交易日实跑与微信真机冒烟未完）。三处版本字段（backend/Cargo.toml workspace.package、frontend/package.json、datafetch/pyproject.toml）**发布前手工同步**，release 首 job 比对 tag 与三处字段，不等即 fail——零机器人回写提交，线性历史干净。
5. **D5 · 容器几何 = /app 烘焙资产 + /data 唯一持久卷**：server 镜像 `WORKDIR=/data`（命名卷挂载点），烘焙资产在 `/app`（static=前端 dist、skills=仓库根 skills/、defaults/{agent.md,config.example.toml}）；ENV 钉死 `ZIMU_STATIC_DIR=/app/static`、`ZIMU_SKILLS_DIR=/app/skills`、`ZIMU_ADDR=0.0.0.0:8080`；**ZIMU_DB_PATH 不设**（缺省 cwd 相对天然落 /data/zimu.db）。entrypoint 首跑**幂等播种**（永不覆盖已有）：卷内 `.robit/prompts/agent.md`（用户可改卷内人格，升级不自动覆盖）与 `.robit/config.toml`（缺则拷 example + 醒目 warn），mkdir media/images，**`exec "$@"`**（compose run 登录命令不被吞成起服务；播种 config 恰好兜住 robit-wechat CLI 的 `load_config().expect()`）。非 root uid 1000，`/data` 属主 1000（命名卷继承）。zimu.db/media/images/.robit 全套天然落卷；sidecar **无卷**。**v0.1.1 修订（NAS bind 实部署暴露）**：Docker 自动创建的 bind 宿主目录归 root，非 root 进程写不进——Dockerfile 不再设 `USER`，entrypoint 以 root 启动先 `chown -R 1000 /data` 再 `gosu` 降权（postgres 模式），主进程仍非 root；命名卷/已 chown 目录场景 chown 近乎无开销，安全受限环境以非 root 启动时自动走降权后分支。
6. **D6 · 优雅关闭 = STOPSIGNAL SIGINT 零代码适配**：Dockerfile `STOPSIGNAL SIGINT` + compose `stop_signal: SIGINT` + `stop_grace_period: 30s`——`docker stop` 发 SIGINT 命中 tokio ctrl_c，既有 graceful 序列（调度器 abort → 逐通道 join → SSE 断 → axum 排空）原样生效，**backend 零改动**。不加 unix SIGTERM 处理（robit/子木代码面不动；将来若加属增强另案）。
7. **D7 · robit-wechat 登录 CLI 随 server 镜像**：builder 阶段 `cargo install --locked robit-wechat --version 0.3.0 --root /opt/cli`（crate 自带 Cargo.lock；CARGO_TARGET_DIR 指 workspace target 下吃缓存），二进制进运行时 `/usr/local/bin`。Docker 登录流程（停服约定照旧 ADR-0017 D3）：`docker compose stop zimu` → `docker compose run --rm -it --no-deps zimu robit-wechat login`（WORKDIR=/data，`-w` 可省）→ `docker compose start zimu`。token 失效态 adapter 静默，不停服重扫安全 + 60s 探测自动恢复（ADR-0017 D4）。成本：每架构 +3~6min 编译，tag 低频可接受。
8. **D8 · 凭据单通道 = compose 根 .env（env_file）**：容器部署**只走 env_file 注入容器 env**，config.toml 整串 `${VAR}` 占位展开消费；**不在 /data/.robit/ 放 .env**（robit set_var 无条件覆盖会静默压过容器 env——上游事实，不写绕过补丁），entrypoint 不播种 .env。`.env.example` 入库（DASHSCOPE_API_KEY / BAILIAN_API_KEY / ZIMU_QQ_APP_ID / ZIMU_QQ_APP_SECRET 空占位；微信无需 env——扫码 state 文件接管）。`deploy/config.example.toml` 以现役 config 为结构蓝本净化：`[zimu.channels] qq=false wechat=false`（fail-closed 缺省）、`log_level="info"`、省略 wechat_bot 节（走扫码 state 流）、保留 `auto_approve=true`（ADR-0014）+ 警示注释与 `global_storage=false`（通道库隔离前提）。裸机开发的双层 .env 约定不受影响。
9. **D9 · 卷缺省 = 命名卷 `zimu-data:/data`**：Windows Docker Desktop bind 挂载（gRPC-FUSE/9p）有 SQLite WAL 锁语义风险；Linux bind 新建目录归 root、uid 1000 写不进。命名卷两平台都顺（镜像内 /data 属主 1000 → 新卷继承）。bind `./data:/data` 作 compose 注释备选并写明两坑（Linux 先 chown 1000）。
10. **D10 · arm64 阻塞解除 = py-mini-racer 换名续作 mini-racer**：pyproject 加直接依赖 `mini-racer`（≥0.12.4 提供 aarch64 manylinux wheel，import 命名空间仍 `py_mini_racer`）+ `[tool.uv] override-dependencies = ["py-mini-racer; python_version < '3'"]`（requires-python>=3.13 使 marker 永假 → 全平台剔除旧包），`uv lock` 重生成。**验证门**（实测修正）：所用 akshare 函数路径的模块全部是顶层 `import py_mini_racer` + `MiniRacer()`（stock_zh_a_sina/fund_etf_sina/index_stock_zh/bond_zh_cov/trade_date_hist），mini-racer 0.14.1 顶层导出兼容；旧式子模块导入 `from py_mini_racer import py_mini_racer` 仅存于**未使用模块**（stock_hk_sina 港股/air_zhenqi 空气质量/multi_decrypt 无调用者死代码），不在子木行情面路径上。门 = 顶层导入 + V8 eval + 所用 10 函数全量导入 + 48 项离线 pytest 双架构绿 + arm64 新浪通道 live_smoke。**兜底**：换名失败则 datafetch 只发 amd64（server 双架构不变），arm64 宿主经模拟跑 sidecar，本条回写降级记录。
11. **D11 · 版本 bump 连带契约再生成**：bump 三处版本后必须 `cargo test -p zimu-server --lib -- --ignored regenerate_openapi_json` 更新 docs/api/openapi.json（info.version），再 `npm run gen:api` 核对 schema.d.ts（漂移守卫 CI 钉死）。health.version 随 CARGO_PKG_VERSION 自动走。
12. **D12 · 智能体运行时依赖三层支持**（主人增补需求：智能体处理上传 Excel/CSV 等需要程序库）：
    - **烘焙常用层**：server 运行时镜像装 python3 + python3-venv（bookworm 3.11）+ 构建期建只读 venv `/opt/agent-env`（**pandas / openpyxl / xlrd / charset-normalizer**——.xlsx/.xls/CSV 含 GBK 编码探测），`PATH=/opt/agent-env/bin:$PATH` 使智能体 bash 的 `python3` 直接命中；root 属主、agent 只读消费；arm64 全有 aarch64 wheel；镜像增重约 150–250MB（取舍接受）。
    - **运行时自助扩展（持久）**：额外包由智能体/用户在卷上自建 venv（`python3 -m venv /data/.venv-agent && /data/.venv-agent/bin/pip install <pkg>`）——落持久卷容器重建不丢；非 root 可行、venv 不受 debian PEP 668 externally-managed 限制。与 ADR-0014 信任模型一致（无沙箱 bash 已在边界内，纪律是唯一软约束，不新增信任面）。
    - **系统级依赖（apt）**：非 root 运行时不能 apt——派生镜像 `FROM ghcr.io/leexgone/zimu` 叠加（docs/08 配方），走重建不走运行时。
    - 不选「pip install --user 到系统 Python」：PEP 668 阻断 + 重建即丢。sidecar 镜像的 pandas 在另一容器，agent bash 不可达，不复用。

## 选项与取舍

- **拓扑**：双镜像 + compose（**选**——sidecar 无状态独立进程、server 降级不瘫的既有设计原样保留，进程隔离/独立升级，与开发形态同构）；单镜像 all-in-one + s6/supervisord（否——镜像臃肿、进程监管复杂、日志交织、任一进程挂掉牵连整容器，与降级设计相悖；`docker run` 单命令的便利由 compose 一键等价覆盖）。
- **镜像仓库**：GHCR（**选**——与仓库同平台、GITHUB_TOKEN 零 secrets、公开仓库镜像公开后拉取免登录）；Docker Hub（否——需两个额外 secrets + 免费档限流）；双发（否——维护面翻倍）。
- **多架构**：原生 arm64 runner 矩阵（**选**——公开仓库免费、Rust 原生编译时长可接受）；QEMU 模拟（否——Rust 全量模拟编译 5–10 倍时长）；仅 amd64（否——主人拍板要 arm64，NAS/ARM 云覆盖面）。
- **触发**：tag 驱动（**选**——版本线清晰、镜像 tag 与 git tag 一一对应、构建频率低）；main edge 镜像（否——Rust 全量编译高频消耗 Actions 分钟）；仅手动（否——易忘、多一步人工）。
- **版本线**：手工同步 + CI 守卫（**选**——零机器人回写、历史干净、守卫钉死一致性）；CI 自动回写（否——机器人提交污染线性历史 + 并发/跳过 CI 循环处理）；镜像 tag 即唯一版本（否——health/--version 报不出真实版本，排查易混）。
- **卷**：命名卷缺省（**选**——两平台属主/锁语义都顺）；bind 缺省（否——Windows WAL 锁风险 + Linux root 属主坑；作注释备选）。
- **登录 CLI**：随 server 镜像 cargo install（**选**——容器内扫码闭环、运维零额外安装）；第三镜像（否——运维面翻倍）；宿主机 cargo install（否——部署者多一步、版本漂移面）。
- **凭据**：env_file 单通道（**选**——容器原生姿势、避开 robit set_var 覆盖坑）；卷内 .robit/.env（否——静默覆盖容器 env，双通道真相不可预测）。
- **arm64 阻塞**：mini-racer 换名（**选**——续作同命名空间、aarch64 wheel 齐备、双架构决策保全）；datafetch amd64-only（兜底——换名失败时降级，server 不受影响）；锁 py-mini-racer 旧版 + QEMU（否——阻塞是 wheel 缺位不是版本问题）。
- **智能体依赖**：烘焙 + 卷上 venv + 派生镜像三层（**选**——开箱即用与可扩展兼得、持久性正确）；运行时 pip --user（否——PEP 668 + 重建即丢）；全部运行时安装（否——首跑慢、网络依赖、不可复现）；精简镜像不带 Python（否——Excel/CSV 解析是智能体既有能力面，容器化不得缩水）。
- **provenance**：false（**选**——v0.1 取舍，digest 合并干净；供应链 attestation 待 buildx 生态成熟另案）。

## 后果

**正面**：`docker compose up -d` 一键部署（Docker Desktop 与 Linux/NAS 同口径）；tag → 双架构镜像 + GitHub Release 全自动；CI 三门禁（前端含 schema 漂移守卫、后端 cargo test --workspace 含契约三连、datafetch 双架构 pytest）对齐 CLAUDE.md 验证方式；优雅关闭/降级/停服登录等既有设计零代码适配；智能体 Excel/CSV 能力容器内开箱即用且可持久扩展；凭据永不进镜像与构建上下文（.dockerignore 排除真实 db/media/.robit 运行态，人格豁免内容级写法）。

**负面 / 接受的成本**：
- server 镜像增重（python3+pandas 约 150–250MB；robit-wechat CLI 编译 +3~6min/架构）。
- Rust 全量编译首跑 10–20min/架构（gha cache 缓解；tag 低频可接受）。
- GHCR package 首推默认 private，手动设公开是一次性人工步骤。
- 卷内人格升级不自动覆盖（设计取舍：用户可改；docs/08 升级段写明手工 diff）。
- mini-racer 换名有 akshare 旧式导入兼容风险（验证门钉死；兜底 amd64-only）。
- provenance:false 无供应链 attestation（v0.1 取舍）。
- arm64 编译门由 release 矩阵承担（CI backend 只跑 amd64；tag 低频失败即无发布不污染 latest）。
- /api/health 恒 200 只做存活探针（降级判据读响应体，docs 明示）。

**以后要改回来得动哪里**：换镜像仓库 → release.yml login/metadata 两处 + docs/08；加架构 → 矩阵 arch 列表 + runner 映射；单镜像化 → 新 ADR 取代 D1 + supervisord 层 + compose 收一服务；edge 镜像 → release.yml 加 main push 触发 + tag 策略；bind 卷缺省 → compose volumes 行 + docs 两坑注；智能体依赖清单调整 → server.Dockerfile venv 行 + docs/08 烘焙清单；provenance 开启 → build-push-action 参数 + manifest 合并适配；SIGTERM 原生支持 → backend shutdown_signal 加 unix 臂（届时 STOPSIGNAL 可回缺省）。