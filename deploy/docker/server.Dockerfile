# syntax=docker/dockerfile:1
# 子木 server 镜像（ADR-0018 D1/D5/D7/D12）：前端产物 + zimu-server + robit-wechat
# 登录 CLI + 智能体 Python 数据环境。三阶段构建；运行时非 root uid 1000，
# WORKDIR=/data（唯一持久卷：zimu.db/.robit/media/images 天然落卷）。

# ── 阶段 1：前端构建（schema.d.ts 已入库，无需 gen:api）──
FROM node:22-bookworm-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ── 阶段 2：Rust 构建（aws-lc-sys 需 cmake；rusqlite bundled 需 cc，镜像自带）──
# 编译缓存策略（2026-11-17 优化）：cargo-chef 依赖层分离——先 cook 全部依赖成
# 独立层（仅 Cargo.toml/lock 变才 invalidate），再 COPY 源码只重编 workspace
# crate；registry 缓存挂载跨构建持久（索引不重拉，根治本地重建卡索引）。
# target 留层内（不用 mount）——CI gha 层缓存可导出，COPY --from 不受影响。
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked
# 登录 CLI 随镜像（D7）：不依赖 workspace 源码，置于源码 COPY 之前——层近乎
# 永久缓存（crate 自带 Cargo.lock，--locked 可复现）；独立 target 目录。
# 版本须与 backend 消费的 robit-wechat lib 同步（0.8.0：计划任务 M2/M3 + LLM
# 超时/重试修复，CLI login 路径不受影响；0.7.0：计划任务 M1；0.6.0：typing 抽象
# 框架侧落地（微信实现待上游）+ PlatformCaps 加固；0.5.1：直调发送透明内容投影
# R17 + 长度守卫 R16；0.4.0 起 login 可 Ctrl+C 取消、state 含 outbound 节流
# 快照段）——升 robit 时连带改这里。
ENV CARGO_TARGET_DIR=/src/cli-target
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install --locked --root /opt/cli --version 0.8.0 robit-wechat
# 依赖层：workspace + 六成员 manifest（新增成员须补 COPY 行）→ chef 预编依赖
ENV CARGO_TARGET_DIR=/src/backend/target
WORKDIR /src/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
# 版本戳（ADR-0021 补记 2026-11-28）：取 [workspace.package] version 烘焙进镜像
# /app/VERSION——entrypoint 比对卷内戳不一致时强制刷新人格/系统技能播种。
# 与 ADR-0018 版本守卫同源（同字段），发版 bump Cargo.toml 即自动联动。
RUN grep -m1 '^version' Cargo.toml | cut -d'"' -f2 > /src/VERSION
COPY backend/crates/zimu-server/Cargo.toml crates/zimu-server/Cargo.toml
COPY backend/crates/zimu-agent/Cargo.toml crates/zimu-agent/Cargo.toml
COPY backend/crates/zimu-agent-cli/Cargo.toml crates/zimu-agent-cli/Cargo.toml
COPY backend/crates/zimu-domain/Cargo.toml crates/zimu-domain/Cargo.toml
COPY backend/crates/zimu-store/Cargo.toml crates/zimu-store/Cargo.toml
COPY backend/crates/zimu-market/Cargo.toml crates/zimu-market/Cargo.toml
# chef prepare 需可解析的 manifest：cargo 要求目标存在——六成员置 dummy src
# （cook 用 recipe 自建 dummy 工程，真实源码随后 COPY 覆盖；CI image-build 门禁实证）
RUN for c in zimu-server zimu-agent zimu-agent-cli zimu-domain zimu-store zimu-market; do \
      mkdir -p crates/$c/src && echo "" > crates/$c/src/lib.rs; \
    done
RUN cargo chef prepare --recipe-path recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo chef cook --release --recipe-path recipe.json
# 源码层：真实代码覆盖 chef _dummy，依赖命中 cook 层，只重编 workspace crate
COPY backend/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release -p zimu-server --bin zimu-server

# ── 阶段 3：运行时 ──
FROM debian:bookworm-slim AS runtime
# D12 烘焙层：智能体 bash 的 python3 直接命中 /opt/agent-env（PATH 前置），
# pandas/openpyxl/xlrd/charset-normalizer = Excel(.xlsx/.xls)/CSV(含 GBK 探测) 开箱即用
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates tzdata curl python3 python3-venv \
    && rm -rf /var/lib/apt/lists/* \
    && python3 -m venv /opt/agent-env \
    && /opt/agent-env/bin/pip install --no-cache-dir pandas openpyxl xlrd charset-normalizer \
    && mkdir -p /data /app/defaults
COPY --from=web /web/dist /app/static
COPY skills/ /app/skills
COPY backend/.robit/prompts/agent.md /app/defaults/agent.md
COPY deploy/config.example.toml /app/defaults/config.example.toml
COPY --from=build /src/backend/target/release/zimu-server /usr/local/bin/zimu-server
COPY --from=build /opt/cli/bin/robit-wechat /usr/local/bin/robit-wechat
COPY --from=build /src/VERSION /app/VERSION
COPY deploy/docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
# 容器几何（D5）：static/skills 缺省是编译期路径，必须 env 指回烘焙资产；
# ZIMU_SKILLS_DIR 语义 = 系统技能播种源（ADR-0021）：应用初始化播种进卷内
# /data/.robit/skills（缺则播种永不覆盖，用户可自增技能），加载单源是该目录；
# ZIMU_DB_PATH 不设——缺省 cwd 相对天然落 /data/zimu.db
ENV ZIMU_STATIC_DIR=/app/static \
    ZIMU_SKILLS_DIR=/app/skills \
    ZIMU_ADDR=0.0.0.0:8080 \
    PATH=/opt/agent-env/bin:$PATH
# v0.1.2 起 root 运行（D5 修订）：UGOS NAS virtiofs/9p bind 翻译层使 chown
# 静默无效（v0.1.1 非 root/gosu 在标准 Linux CI 通过、NAS 仍 Permission denied）；
# 个人 NAS 部署 root 惯例。缓解：仅内网端口、无 privileged/无 docker.sock、
# 应用层绑定白名单不变。sidecar 无卷仍保留非 root。
WORKDIR /data
EXPOSE 8080
# 优雅关闭（D6）：tokio ctrl_c 在 Linux 只收 SIGINT——docker stop 缺省 SIGTERM
# 不触发 graceful 序列，STOPSIGNAL 改 SIGINT 零代码适配（compose 侧同步 stop_signal）
STOPSIGNAL SIGINT
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1
ENTRYPOINT ["/entrypoint.sh"]
CMD ["zimu-server"]