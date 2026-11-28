#!/bin/sh
# 子木容器入口（ADR-0018 D5，v0.1.2 起 root 运行）：播种 + 版本戳强刷 + exec "$@"
# 为何 root：UGOS 等 NAS 的 Docker 经 virtiofs/9p 翻译 bind 挂载，chown 静默
# 无效（v0.1.1 gosu 降权方案在标准 Linux CI 通过、NAS bind 仍 Permission
# denied）；个人 NAS 部署 root 运行与社区镜像惯例一致（jimeng 等）。
# 缓解面：仅内网端口、无 privileged/无 docker.sock、应用层绑定白名单不变。
# - 同版本重启：缺则播种、永不覆盖卷内已有文件（用户可改卷内人格/config，不被动）
# - 版本变化（ADR-0021 补记 2026-11-28）：镜像 /app/VERSION 与卷内戳
#   /data/.robit/zimu_seed_version 不一致 → **强制刷新**人格与系统技能——
#   根治「发版后须登 NAS 点删旧文案」的运维债；戳缺失（旧卷首跑新镜像）同样
#   走一次强刷，历史上滞留的旧版人格/技能就此自愈。用户自增技能不在烘焙源
#   清单（/app/skills/*）内，任何情况下不碰；config.toml / wechat-state.json /
#   robit.db 永不属于刷新范围。
# - 不播种 .env：凭据单通道 = compose 根 .env（env_file）——robit 的 .env 加载
#   会无条件覆盖容器 env（D8），卷内放 .env 会造成双通道真相不可预测
# - exec "$@" 使 `docker compose run ... zimu robit-wechat login` 透传执行，
#   不被吞成起服务；播种 config 恰好兜住 robit-wechat CLI 的 load_config().expect()
# - ZIMU_APP_DIR/ZIMU_DATA_DIR 仅为测试注入口，容器内用缺省 /app /data
set -eu

APP_DIR="${ZIMU_APP_DIR:-/app}"
DATA_DIR="${ZIMU_DATA_DIR:-/data}"

mkdir -p "$DATA_DIR/.robit/prompts" "$DATA_DIR/media" "$DATA_DIR/images"

# ── 版本戳检测：不一致（含缺戳）→ 强制刷新系统播种资产 ──
NEW_VER=$(cat "$APP_DIR/VERSION" 2>/dev/null || echo unknown)
OLD_VER=$(cat "$DATA_DIR/.robit/zimu_seed_version" 2>/dev/null || echo "")
if [ "$NEW_VER" != "$OLD_VER" ]; then
  echo "[entrypoint] 播种版本 ${OLD_VER:-无}<${NEW_VER}：强制刷新人格与系统技能（用户自增技能保留）" >&2
  cp "$APP_DIR/defaults/agent.md" "$DATA_DIR/.robit/prompts/agent.md"
  for d in "$APP_DIR/skills"/*/; do
    name=$(basename "$d")
    if [ -d "$DATA_DIR/.robit/skills/$name" ]; then
      rm -rf "$DATA_DIR/.robit/skills/$name"
    fi
  done
  echo "$NEW_VER" > "$DATA_DIR/.robit/zimu_seed_version"
fi

# ── 常规缺则播种（版本刷新之后兜底首跑场景；永不覆盖已有） ──
if [ ! -f "$DATA_DIR/.robit/prompts/agent.md" ]; then
  cp "$APP_DIR/defaults/agent.md" "$DATA_DIR/.robit/prompts/agent.md"
fi

if [ ! -f "$DATA_DIR/.robit/config.toml" ]; then
  cp "$APP_DIR/defaults/config.example.toml" "$DATA_DIR/.robit/config.toml"
  echo "[entrypoint] WARN: 已播种示例 config.toml（$DATA_DIR/.robit/config.toml）。" >&2
  echo "[entrypoint] WARN: 请在 compose 根 .env 填入凭据、按需翻 [zimu.channels] 开关后重启；" >&2
  echo "[entrypoint] WARN: 微信需扫码登录（docker compose run --rm -it --no-deps zimu robit-wechat login，docs/08 §3）。" >&2
fi

exec "$@"
