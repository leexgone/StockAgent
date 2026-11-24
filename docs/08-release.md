# 08 · 发布与部署（Docker · ADR-0018）

子木的发布形态 = **Docker 双镜像 + docker compose 一键起**（ADR-0018）：

| 镜像 | 内容 | 端口 |
|---|---|---|
| `ghcr.io/leexgone/zimu` | zimu-server（axum + 前端静态产物 + Agent）+ robit-wechat 登录 CLI + 智能体 Python 数据环境 | 8080 |
| `ghcr.io/leexgone/zimu-datafetch` | 行情 sidecar（FastAPI + akshare/baostock，无状态零落盘） | 8300（仅容器网络，不映射宿主） |

发布由 GitHub Actions tag 驱动：push `v*` → 版本守卫 → 测试门禁 → amd64/arm64 原生矩阵构建 → GHCR manifest 合并 → 容器冒烟 → GitHub Release。维护者流程见 §8。

## 1. 快速开始

前置：Docker（Docker Desktop 或 Linux docker engine + compose v2）。

```bash
# 取 compose 与 env 模板（或克隆仓库）
curl -LO https://raw.githubusercontent.com/leexgone/StockAgent/main/compose.yaml
curl -L -o .env.example https://raw.githubusercontent.com/leexgone/StockAgent/main/.env.example
cp .env.example .env        # 填入凭据（见 §2；空 key 也能起，Agent 离线但服务不瘫）
docker compose up -d
# → http://127.0.0.1:8080
```

- 端口缺省 **127.0.0.1:8080**（内网监听面，ADR-0005）；局域网/反代自行改 compose `ports`（公网暴露前请自加鉴权——服务本身无鉴权，IM 信任边界 = 绑定白名单）。
- 数据落**命名卷 `zimu-data`**（/data）：zimu.db、.robit/（config/人格/技能/记忆/日志/微信 state）、media/、images/ 全在卷内。
- 首跑 entrypoint 自动播种 `.robit/config.toml`（示例模板）与 `.robit/prompts/agent.md`（子木人格），**永不覆盖已有文件**；系统技能（三模板）由**应用初始化**（AgentCore 装配，非 entrypoint）播种进 `.robit/skills/`——同样缺则播种永不覆盖，用户可在该目录自增技能（ADR-0021）。
- 健康：`curl http://127.0.0.1:8080/api/health`——恒 200 只做存活探针；`agent_online`/`static_ready`/`qq_online`/`wechat_online` 读响应体判断降级面。

**Docker Desktop（Windows/macOS）**：命名卷缺省即可，勿改 bind 挂载（gRPC-FUSE/9p 上 SQLite WAL 锁语义有风险）。
**Linux 服务器/NAS**：命名卷缺省即可；若偏好 bind 挂载 `./data:/data`（compose 内有注释备选），v0.1.2 起容器以 root 运行、`mkdir data` 即可无需 chown（UGOS 等 NAS 的 bind 翻译层 chown 静默无效，ADR-0018 D5），并知悉 bind WAL 锁风险。安全约束：仅内网端口、不挂 docker.sock、不给 privileged。

## 2. 配置：.env 与 config.toml

**凭据单通道纪律（ADR-0018 D8）**：容器部署凭据**只放 compose 根 `.env`**（env_file 注入容器 env）。**不要在卷内 `/data/.robit/` 放 `.env`**——robit 的 .env 加载会无条件覆盖容器环境变量，双通道并存时真相不可预测。

`.env`（模板 `.env.example`）：

| 变量 | 用途 |
|---|---|
| `DASHSCOPE_API_KEY` | 对话模型（通义千问，OpenAI 兼容面） |
| `BAILIAN_API_KEY` | 生图模型（通义万相） |
| `ZIMU_QQ_APP_ID` / `ZIMU_QQ_APP_SECRET` | QQ 机器人凭据（启用 QQ 通道才需要） |
| `# ZIMU_SCHEDULER_DISABLE=1` | 可选：停用调度器（设**任意值**即停用） |

微信**无需 env**——bot_token 由扫码登录落 state 文件（§3）。

`config.toml`（卷内 `/data/.robit/config.toml`，首跑自 `deploy/config.example.toml` 播种）：模型/provider/生图参数；凭据一律整串 `${VAR}` 占位（展开消费容器 env），**永不写密钥字面量**。通道开关 `[zimu.channels] qq/wechat` 缺省 **false（fail-closed）**——填好凭据后翻 true 并 `docker compose restart zimu`。`auto_approve=true` 是 ADR-0014 拍板缺省（工具自动执行，纪律是唯一软约束）；重开确认须先补 docs/07 C13。

## 3. 微信登录（Docker 形态，停服约定照旧 ADR-0017 D3）

```bash
docker compose stop zimu                                    # 停服（state 非原子写，硬约定）
docker compose run --rm -it --no-deps zimu robit-wechat login   # 终端二维码扫码（WORKDIR=/data，-w 可省）
docker compose start zimu
```

- 登录成功 → bot_token 落卷内 `.robit/wechat-state.json`；`[zimu.channels] wechat=true` 时服务启动即连。
- **token 失效后**（errcode -14）：adapter 静默、服务进入 60s 只读探测态——此时**不停服重扫也安全**，扫完 ≤60s 自动重建连接（ADR-0017 D4）。
- 绑定会话：Web「助理接入」屏待绑定列表一键绑定（ADR-0017 D3 补记）；未绑定来消息静默丢弃（fail-closed）。
- QQ 通道无交互登录：.env 凭据 + config 开关即可。

## 4. 智能体运行时依赖（ADR-0018 D12）

智能体持 bash/write/edit 标准工具面（自动执行，ADR-0014），解析上传的 Excel/CSV 依赖 Python 数据环境——三层支持：

1. **开箱即用（烘焙层）**：镜像内置 `/opt/agent-env` venv（PATH 已含）——`pandas` / `openpyxl`（.xlsx）/ `xlrd`（.xls）/ `charset-normalizer`（CSV GBK 编码探测）。智能体 bash 里 `python3` 直接命中。
2. **自助扩展（持久层）**：需要额外包时，在卷上自建 venv（容器重建不丢）：
   ```bash
   python3 -m venv /data/.venv-agent
   /data/.venv-agent/bin/pip install <package>
   # 之后用 /data/.venv-agent/bin/python 执行
   ```
   非 root 可行、不受 debian PEP 668 限制；只装可信包（供应链纪律与 ADR-0014 信任模型一致）。
3. **系统级依赖（重建层）**：apt 包运行时不能装（非 root）——派生镜像叠加：
   ```dockerfile
   FROM ghcr.io/leexgone/zimu:latest
   USER root
   RUN apt-get update && apt-get install -y --no-install-recommends <pkg> && rm -rf /var/lib/apt/lists/*
   USER zimu
   ```
   compose `image:` 改指派生镜像即可。

## 5. 数据卷与备份

卷内即全部真相：`zimu.db`（账本/会话档案/绑定）、`.robit/`（config/人格/技能/记忆/robit.db IM 会话/日志/微信 state）、`media/`（上传与附件）、`images/`（生图产物）。

```bash
docker compose stop zimu        # 静默后备份（SQLite WAL 一致性）
docker run --rm -v zimu-data:/data -v "$PWD":/backup alpine tar czf /backup/zimu-data-$(date +%F).tar.gz -C /data .
docker compose start zimu
```

- 微信 state 含凭据——备份文件按密钥纪律保管，**永不入库/上传**。
- 日志：robit 日志落卷内 `.robit/logs/`（robit 自带 retention 清理）；容器 stdout 日志由 Docker json-file 驱动管（可配 max-size）。
- 恢复：停服 → 解包回卷 → 起服。

## 6. 升级

```bash
docker compose pull && docker compose up -d
```

- 数据卷原样保留；entrypoint 播种幂等——**卷内人格/config 不会被新版本覆盖**。想采用新版默认人格：对比 `/app/defaults/agent.md` 与卷内 `.robit/prompts/agent.md` 手工合并（或删除卷内文件让 entrypoint 重播种，自定义会丢）。
- 技能同款语义（ADR-0021）：**卷内 `.robit/skills/` 已有技能目录（含用户自增与改过的系统技能）不会被新版本覆盖**；系统技能新版模板在镜像 `/app/skills/`，想采用新版须删除卷内对应技能目录让应用初始化重播种（自定义会丢）。用户自增技能目录不受升级影响。
- 版本真值：`/api/health` 的 `version` 字段 = 镜像版本（与 git tag 一致）。
- 回滚：compose `image:` 钉旧版 tag（如 `ghcr.io/leexgone/zimu:v0.1.0`）重新 up。

## 7. 故障排查速查

| 症状 | 查 |
|---|---|
| 网页打不开 | `docker compose ps`；ports 是否 127.0.0.1（远程访问需反代或改绑定） |
| Agent 离线 | health `agent_online=false` → .env 模型 key / config providers；`docker compose logs zimu` |
| 行情 503 | datafetch 容器状态 / `docker compose logs datafetch`；server 缺 sidecar 降级不瘫是设计行为 |
| 微信不应答 | 绑定白名单（助理接入屏待绑定列表）；token 失效看日志探测态（§3） |
| QQ 不应答 | .env 凭据 + `[zimu.channels] qq=true` + 绑定；凭据闸门拒连看日志 |
| 容器反复重启 | `docker compose logs`；卷权限（v0.1.2+ root 运行无需处理；v0.1.0/0.1.1 bind 挂载在 UGOS 上不支持，请升 v0.1.2+） |
| 优雅关闭验证 | `docker compose stop zimu` 日志应见调度器/通道/SSE 关闭序列（STOPSIGNAL SIGINT，30s 宽限） |

## 8. 维护者发布流程

1. **同步三处版本**（手工，ADR-0018 D4）：`backend/Cargo.toml` workspace.package、`frontend/package.json`、`datafetch/pyproject.toml` → 目标版本（如 0.1.0）。
2. **连带再生成**（D11）：`cargo test -p zimu-server --lib -- --ignored regenerate_openapi_json` → `cd frontend && npm run gen:api` → 核对 diff；全量门禁复跑（cargo test --workspace / vitest / uv run pytest）。
3. 提交 `chore(release): vX.Y.Z`，push main，**打 tag `vX.Y.Z` 并 push tag**（触发 release.yml）。
4. 流水线自动：版本守卫（tag ≠ 三处版本即 fail）→ ci 门禁 → 2 镜像 × 2 架构原生构建 → digest 合并 manifest（vX.Y.Z + latest）→ 容器冒烟（health version==tag、static_ready）→ GitHub Release（安装指引）。
5. **首发布一次性**：GHCR 两个 package（zimu / zimu-datafetch）默认 private——GitHub → Packages → 各 package → Package settings → Change visibility → Public，并关联仓库。
6. 干净机器验证：`docker compose pull && up -d` → health/七屏/行情；arm64 设备（或 Docker Desktop Rosetta）拉 arm64 manifest + datafetch `live_smoke.py` 新浪通道。

CI 门禁（ci.yml，PR/main）：frontend（npm ci → gen:api 漂移守卫 → vitest → build）、backend（cargo test --workspace --no-fail-fast，含契约三连与 persona 守护）、datafetch（**双架构** uv sync --frozen → pytest；arm64 腿专证 lock 可安装）。全离线，联网冒烟不进 CI。

## 9. 绿联 NAS 正式部署（`deploy/compose.nas.yaml`）

本地 Docker Desktop 适合测试（命名卷 + 手工启动覆盖层）；**正式服务推荐 NAS 常驻**，用专用模板 `deploy/compose.nas.yaml`——与通用 compose 的差异：bind 挂载到 NAS 可见目录（文件管理器可查、NAS 备份工具直接覆盖；Linux 本地盘 SQLite WAL 安全，无 Desktop gRPC-FUSE 坑）、`restart: unless-stopped`（NAS 重启自动拉起）、镜像版本经 `.env` 的 `ZIMU_VERSION` 钉死、json-file 日志轮转（10m×3）。

**步骤（SSH 推荐，UGOS 控制面板先启用 SSH）**：

```bash
mkdir -p /volume1/docker/zimu && cd /volume1/docker/zimu   # 路径按实际共享文件夹调整
# 放入 compose.yaml（= deploy/compose.nas.yaml 内容）与 .env（= .env.example 填凭据 + ZIMU_VERSION）
# 首跑可先 touch .env（空文件合法；env_file 缺文件直接报错，勿注释该行绕过——日后凭据不注入）
mkdir -p data                                                # v0.1.2 起 root 运行自动建子目录，无需 chown
docker compose pull && docker compose up -d
# → http://<NAS局域网IP>:8018
```

- **架构自适应**：镜像双架构 manifest，Intel DXP（amd64）与 Realtek ARM 机型（arm64）自动拉对。
- **网络**：专用桥接 `zimu-net`（固定名，UGOS 网络管理可见；子网 Docker 自动分配避免与局域网段冲突）——用户定义 bridge 自带容器名 DNS（`ZIMU_MARKET_URL=http://datafetch:8300`），sidecar 仅本网内可达、不映射宿主。
- **端口面**：`8018:8080`（宿主 8018 → 容器 8080，容器内端口与 healthcheck 不动）局域网访问；服务无鉴权，**勿直接暴露公网**——公网需求自加反代 + 鉴权（ADR-0005 监听面纪律）。
- **微信登录（NAS SSH 终端扫码，停服约定照旧）**：`docker compose stop zimu` → `docker compose run --rm -it --no-deps zimu robit-wechat login` → `docker compose start zimu`；token 失效态不停服重扫安全（60s 探测恢复）。
- **通道启用**：改 `data/.robit/config.toml` 的 `[zimu.channels]` qq/wechat=true + `.env` 凭据 → `docker compose up -d`（env_file 变更需重建容器，restart 不重读）。
- **升级**：`.env` 改 `ZIMU_VERSION=vX.Y.Z` → `docker compose pull && docker compose up -d`；回滚改回旧 tag 同命令。数据卷原样，人格不自动覆盖（§6）。
- **备份**：NAS 快照/备份工具直接覆盖 `data/` 目录即可（含账本/人格/记忆/微信 state）；一致性要求高时先 `docker compose stop zimu` 再备（§5 同理）。微信 state 含凭据，备份按密钥纪律保管。
- UGOS Docker 界面「项目/Compose」粘贴模板亦可，但 `.env` 须位于项目目录（env_file 相对路径）；SSH 方式目录自主、更直观。
- **国内网络加速（ghcr.io 直拉慢）**：Docker `registry-mirrors`/UGOS 镜像加速**只代理 Docker Hub，不覆盖 ghcr.io**——配加速器无效。正路 = 镜像源前缀拉取 + retag 回原名（2026-11-17 实测南大/DaoCloud 源存活）：
  ```bash
  docker pull ghcr.nju.edu.cn/leexgone/zimu:v0.1.0   # 备选前缀 docker.m.daocloud.io/ghcr.io/
  docker pull ghcr.nju.edu.cn/leexgone/zimu-datafetch:v0.1.0
  docker tag ghcr.nju.edu.cn/leexgone/zimu:v0.1.0 ghcr.io/leexgone/zimu:v0.1.0
  docker tag ghcr.nju.edu.cn/leexgone/zimu-datafetch:v0.1.0 ghcr.io/leexgone/zimu-datafetch:v0.1.0
  docker compose up -d   # 本地已有镜像不再直拉 ghcr（pull_policy 缺省 missing）；勿跑 compose pull
  ```
  升级同法（新 tag 镜像源拉 + retag + up -d）。有代理者给 dockerd 配 HTTPS_PROXY 更彻底。信任注：镜像源转发 manifest/layers，layer digest 由 docker 校验，tag→manifest 映射信任镜像源——个人使用可接受。