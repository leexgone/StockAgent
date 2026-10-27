# 子木 · 股票智能助理（StockAgent）

面向个人投资者的股票助理程序：盯持仓、读行情、给可执行的买卖建议、诊断组合风险。发布形态是**常驻 Web 服务**——浏览器访问全部界面，也可以通过微信 / QQ 与助理对话、接收预警与盘后复盘（与 robit 的桌面端定位不同）。

**当前阶段：技术基座 Spike（路线图 P2）。** 技术路线已定案（[ADR-0005](docs/decisions/0005-tech-route-rust-robit-react-sidecar.md)）：Rust + [robit](https://github.com/leexgone/robit) agent 基建 / React + Vite 前端 / Python 行情 sidecar / SQLite。**最简运行框架已立起**：`backend/`（axum）+ `frontend/`（React），启动服务即可在浏览器进首页；`design/` 原型（零构建、演示样本）保留为设计参照与验收基准。

```powershell
# 运行最简框架（生产形态：后端托管前端构建产物，单端口）
cd frontend; npm install; npm run build
cd ..\backend; cargo run                  # → http://127.0.0.1:8080

# 开发形态：前端热更新（5173，/api 反代 8080），细节见 backend/README.md
```

```bash
python -m http.server 8000 --directory design   # 原型：或直接双击 design/index.html
# → http://localhost:8000/
```

## 目录

| 路径 | 内容 |
|---|---|
| `backend/` | Rust 服务（Cargo workspace，axum）：health + SPA 静态托管，robit agent 集成在 P2-T2 |
| `frontend/` | React + TS + Vite 前端：首页骨架 + 设计令牌迁移（`design/assets/ui.css` 为令牌来源） |
| `datafetch/` | Python 行情 sidecar（uv + FastAPI + akshare）：P2-T1 spike 产物，生产化在 P4-T3 |
| `design/` | 界面原型（7 个页面 + 共享样式/数据/图表层），原型自带的说明见 `design/README.md` |
| `docs/` | 产品规格与工程规范：范围、界面、业务口径、数据契约、设计系统、路线图、待办 |
| `docs/decisions/` | 决策记录（ADR）—— 每个「为什么这样做」都留档 |
| `CLAUDE.md` | 给 Claude Code 的工程约定，动手前先读这个 |

## 原型包含的界面

| 界面 | 职责 |
|---|---|
| `index.html` | 工作台入口：随时段变化的问候、资产快照、界面导航、待决事项 |
| `dashboard.html` | 总览：净值 vs 沪深300、当日盈亏归因、待处理预警、持仓走势、收盘摘要 |
| `holdings.html` | 持仓：明细/排序/筛选，加减仓抽屉（整手与涨跌停校验、费用、摊薄成本、仓位重算） |
| `analysis.html` | AI 建议：结论 + 得分 + 置信度、四维拆解、30 日 K 线、分笔执行计划、风险自陈 |
| `chat.html` | 问答：结论 / 为什么 / 怎么做三段式回答，附依据来源与追问 |
| `diagnosis.html` | 组合诊断：风险指标、回撤水下曲线、行业集中度与 HHI、再平衡清单 |
| `wechat.html` | 微信接入：通道与降级、推送订阅、静音标的、绑定、卡片模板、推送日志 |

## 文档地图

改原型之前，按需读：

- `docs/01-vision-and-scope.md` —— 做什么、不做什么、合规边界
- `docs/02-screens.md` —— 每屏模块与数据依赖
- `docs/03-domain-rules.md` —— **口径手册**：整手、费用、摊薄成本、涨跌停、仓位纪律、波动测算
- `docs/04-data-contract.md` —— `window.ZM` 数据契约，接真实数据时唯一的替换边界
- `docs/05-design-system.md` —— 视觉令牌、红涨绿跌语义、断点
- `docs/06-roadmap.md` / `docs/07-backlog.md` —— 阶段计划与已知问题
- `docs/decisions/` —— ADR：文档先行、零构建原型、演示数据边界、技术路线（Rust + robit + React + sidecar）

## 三条硬约定

1. **页面上不写死数字。** 总资产、盈亏、占比、集中度等一律从持仓与净值序列派生，避免文案与图表互相打架。
2. **共享派生逻辑只能放在 `design/assets/data.js`。** 页面不得各自往全局对象上补函数（已有跨页引用踩坑，见 `docs/07-backlog.md`）。
3. **`design/` 原型不引入构建、框架与第三方依赖**；正式前端按 ADR-0005 在 `frontend/`（React + Vite）另起，原型保留为参照直至迁移完成。

## 免责声明

本项目用于界面与信息架构验证。所有「买入 / 持有 / 卖出」结论均由演示数据生成，**不构成投资建议**；即便接入真实行情，每一笔委托仍需你本人确认。程序不连接券商交易接口，不代提交委托。

## 许可

Apache-2.0，见 `LICENSE`。
