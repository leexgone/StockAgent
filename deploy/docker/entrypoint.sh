#!/bin/sh
# 子木容器入口（ADR-0018 D5，v0.1.2 起 root 运行）：幂等播种 + exec "$@"
# 为何 root：UGOS 等 NAS 的 Docker 经 virtiofs/9p 翻译 bind 挂载，chown 静默
# 无效（v0.1.1 gosu 降权方案在标准 Linux CI 通过、NAS bind 仍 Permission
# denied）；个人 NAS 部署 root 运行与社区镜像惯例一致（jimeng 等）。
# 缓解面：仅内网端口、无 privileged/无 docker.sock、应用层绑定白名单不变。
# - 缺则播种、永不覆盖卷内已有文件（用户可改卷内人格/config，升级不自动覆盖）
# - 不播种 .env：凭据单通道 = compose 根 .env（env_file）——robit 的 .env 加载
#   会无条件覆盖容器 env（D8），卷内放 .env 会造成双通道真相不可预测
# - exec "$@" 使 `docker compose run ... zimu robit-wechat login` 透传执行，
#   不被吞成起服务；播种 config 恰好兜住 robit-wechat CLI 的 load_config().expect()
set -eu

mkdir -p /data/.robit/prompts /data/media /data/images

if [ ! -f /data/.robit/prompts/agent.md ]; then
  cp /app/defaults/agent.md /data/.robit/prompts/agent.md
fi

if [ ! -f /data/.robit/config.toml ]; then
  cp /app/defaults/config.example.toml /data/.robit/config.toml
  echo "[entrypoint] WARN: 已播种示例 config.toml（/data/.robit/config.toml）。" >&2
  echo "[entrypoint] WARN: 请在 compose 根 .env 填入凭据、按需翻 [zimu.channels] 开关后重启；" >&2
  echo "[entrypoint] WARN: 微信需扫码登录（docker compose run --rm -it --no-deps zimu robit-wechat login，docs/08 §3）。" >&2
fi

exec "$@"