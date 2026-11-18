#!/bin/sh
# 子木容器入口（ADR-0018 D5）：root 修属主 → 降权 → 幂等播种 → exec "$@"
# - postgres 模式：bind 挂载场景 Docker 自动建的宿主目录归 root，非 root 进程
#   写不进——root 启动先 chown /data 再 gosu 降权到 uid 1000，主进程仍非 root。
#   命名卷/已 chown 的 bind 目录场景 chown 近乎无开销；非 root 启动（安全受限
#   环境）直接走降权后分支，不报错。
# - 缺则播种、永不覆盖卷内已有文件（用户可改卷内人格/config，升级不自动覆盖）
# - 不播种 .env：凭据单通道 = compose 根 .env（env_file）——robit 的 .env 加载
#   会无条件覆盖容器 env（D8），卷内放 .env 会造成双通道真相不可预测
# - exec "$@" 使 `docker compose run ... zimu robit-wechat login` 透传执行，
#   不被吞成起服务；播种 config 恰好兜住 robit-wechat CLI 的 load_config().expect()
set -eu

if [ "$(id -u)" = "0" ]; then
  mkdir -p /data
  chown -R 1000:1000 /data
  exec gosu 1000:1000 /entrypoint.sh "$@"
fi

# 以下以 zimu(uid 1000) 身份运行
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