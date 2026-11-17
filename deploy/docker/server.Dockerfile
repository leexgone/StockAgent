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
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src/backend
COPY backend/ ./
RUN cargo build --release -p zimu-server --bin zimu-server
# 登录 CLI 随镜像（D7）：crate 自带 Cargo.lock，--locked 可复现；
# target 指到 workspace 下共享 registry 缓存面（每架构 +3~6min，tag 低频可接受）
ENV CARGO_TARGET_DIR=/src/backend/target/cli-install
RUN cargo install --locked --root /opt/cli --version 0.3.0 robit-wechat

# ── 阶段 3：运行时 ──
FROM debian:bookworm-slim AS runtime
# D12 烘焙层：智能体 bash 的 python3 直接命中 /opt/agent-env（PATH 前置），
# pandas/openpyxl/xlrd/charset-normalizer = Excel(.xlsx/.xls)/CSV(含 GBK 探测) 开箱即用
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates tzdata curl python3 python3-venv \
    && rm -rf /var/lib/apt/lists/* \
    && python3 -m venv /opt/agent-env \
    && /opt/agent-env/bin/pip install --no-cache-dir pandas openpyxl xlrd charset-normalizer \
    && useradd -u 1000 -m zimu \
    && mkdir -p /data /app/defaults \
    && chown 1000:1000 /data
COPY --from=web /web/dist /app/static
COPY skills/ /app/skills
COPY backend/.robit/prompts/agent.md /app/defaults/agent.md
COPY deploy/config.example.toml /app/defaults/config.example.toml
COPY --from=build /src/backend/target/release/zimu-server /usr/local/bin/zimu-server
COPY --from=build /opt/cli/bin/robit-wechat /usr/local/bin/robit-wechat
COPY deploy/docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
# 容器几何（D5）：static/skills 缺省是编译期路径，必须 env 指回烘焙资产；
# ZIMU_DB_PATH 不设——缺省 cwd 相对天然落 /data/zimu.db
ENV ZIMU_STATIC_DIR=/app/static \
    ZIMU_SKILLS_DIR=/app/skills \
    ZIMU_ADDR=0.0.0.0:8080 \
    PATH=/opt/agent-env/bin:$PATH
USER zimu
WORKDIR /data
EXPOSE 8080
# 优雅关闭（D6）：tokio ctrl_c 在 Linux 只收 SIGINT——docker stop 缺省 SIGTERM
# 不触发 graceful 序列，STOPSIGNAL 改 SIGINT 零代码适配（compose 侧同步 stop_signal）
STOPSIGNAL SIGINT
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1
ENTRYPOINT ["/entrypoint.sh"]
CMD ["zimu-server"]