# syntax=docker/dockerfile:1
# 子木行情 sidecar 镜像（ADR-0018 D1）：无状态、零落盘（纯内存缓存）、无卷。
# 双架构通用（mini-racer 换名解除 arm64 wheel 阻塞，D10）；uv 钉版本与仓库开发环境一致。
FROM python:3.13-slim-bookworm
COPY --from=ghcr.io/astral-sh/uv:0.12.22 /uv /usr/local/bin/uv
ENV UV_LINK_MODE=copy UV_COMPILE_BYTECODE=1
WORKDIR /app
COPY datafetch/pyproject.toml datafetch/uv.lock ./
RUN uv sync --frozen --no-dev
COPY datafetch/zimu_datafetch/ ./zimu_datafetch/
RUN useradd -u 1000 zimu && chown -R 1000:1000 /app
USER zimu
EXPOSE 8300
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD python -c "import urllib.request as u; u.urlopen('http://127.0.0.1:8300/healthz', timeout=3)" || exit 1
CMD ["/app/.venv/bin/uvicorn", "zimu_datafetch.server:app", "--host", "0.0.0.0", "--port", "8300"]