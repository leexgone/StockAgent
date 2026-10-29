# datafetch · 行情 sidecar（P4-T3 正式化）

子木的行情取数层。ADR-0005 定案：Python sidecar（uv + FastAPI），**唯一消费者是本机
`zimu-server`**（P4-T4 起经 Rust 适配器接入），监听 `127.0.0.1`，不开公网入站。

P2-T1 spike 的通道结论全部沿用（`docs/04` 附录）；本阶段把「能跑通」变成「坏了不崩」：
兜底链 + 熔断 + 限频 + 缓存 + 错误契约，端点补齐 `/quote` `/kline` `/cb_list` `/calendar`。

## 运行

```bash
cd datafetch
uv sync                                  # 首次（dev 组含 pytest）
uv run uvicorn zimu_datafetch.server:app --host 127.0.0.1 --port 8300
uv run pytest                            # 离线单测：假源注入，不打网络
uv run spike_probe.py --quick            # 历史探测（P2-T1，三轮结论的出处）
```

## 端点契约

| 端点 | 参数 | 说明 |
|---|---|---|
| `GET /quote` | `codes=sh600519,sz159915,sh000001`（≤50，须带显式前缀） | 实时批量；`status` 四态：`ok / halted_or_delisted / empty_or_delisted / no_data`；行情时间随行内 `date/time`（主源）显式 |
| `GET /kline/{symbol}` | `kind=stock\|etf\|index\|cb`（可缺省，仅内部路由）、`start`/`end`（YYYYMMDD 或 YYYY-MM-DD，默认 400 天内） | 日线；股票 qfq、其余不复权；空结果 `200 count=0` 不是错 |
| `GET /cb_list` | — | 转债全表：新浪现券 × 集思录强赎 join（字段见下）；集思录挂 → 强赎字段置 null 并标 `redeem_source: null`，不 502 |
| `GET /calendar` | `start`/`end`（默认今天 ±180 天，**含未来交易日**） | `trade_dates[]`，`marketClock` 节假日/「下一交易日」的数据源 |
| `GET /healthz` | — | 版本、熔断器状态、缓存命中数、当前禁用源清单 |

**代码必须带显式市场前缀**（`sh/sz/bj` + 6 位数字）——B7 在 sidecar 侧的落实：
裸代码一律 `400 BAD_SYMBOL`，不做号段猜测；`mkt` 由契约调用方（Rust 适配器 / 前端）
拼好带来。号段推断只剩 kline 缺省 `kind` 时的内部路由，响应 `kind_routed` 标注。

**错误契约**（所有可预期错误同形，不透传堆栈）：

```json
{ "error": { "code": "UPSTREAM_ALL_FAILED", "message": "…", "detail": { "attempts": [ { "source": "sina_hq", "error": "ConnectionError: …" } ] } } }
```

| HTTP | code | 场景 |
|---|---|---|
| 400 | `BAD_SYMBOL` | 缺前缀 / 格式非法（B7） |
| 400 | `BAD_PARAMS` | kind 非法、日期格式错、codes 空/超限、日历区间越界 |
| 502 | `UPSTREAM_ALL_FAILED` | 该类目兜底链全挂；`detail.attempts` 逐源原因 |
| 500 | `INTERNAL` | 未预期异常（细节只进日志） |

## 降级、熔断、限频、缓存

每个数据类目是一条**有序源链**；请求先过缓存，未命中按链尝试。响应里
`degraded: true` + `attempts[]` 如实告知走过哪些失败路——主程序能区分「主源正常」
与「凑合活着」，`/healthz` 可观测。

| 类目 | 链（优先级从左到右） | 依据 |
|---|---|---|
| 实时 | 新浪 `hq.sinajs.cn` 直连 → 腾讯 `qt.gtimg.cn` 直连 → 东财 `push2 ulist` 直连（浏览器 UA + `fltt=2`） | spike：akshare 的东财实时封装被指纹拦，直连通道不依赖它；东财被拦时快速失败 ~0.4s，适合熔断切源而非重试等待。腾讯进二通道（B14，2026-10-29）：新浪不 carry 部分指数（中证转债 sh000832 回空名零价行）；**quote 编排按标的粒度补齐**——某源成功但未收录的标的自动落下一源（attempts 留痕），链尽仍缺合成 `no_data`，502 收窄为「无任何源成功」 |
| 股票日线 | 新浪 `stock_zh_a_daily`(qfq) → baostock(前复权) → akshare 东财 hist（成交量手→股 ×100 归一） | **baostock 复测通过**（2026-10-28 登录+日线+日历全 OK；spike 时的 10002007 是暂时故障）→ 保留，进兜底链 |
| 指数日线 | 新浪 `stock_zh_index_daily` → baostock 指数 | |
| ETF/转债日线 | 仅新浪系（baostock 不覆盖场内 ETF/转债） | 全链挂即 502，诚实降级 |
| 转债列表 | 新浪现券表（北交所定转按沪深覆盖范围过滤） × 集思录强赎表（低频，可缺） | 集思录字段全：转股价/触发价/正股价/强赎状态/最后交易日/剩余规模等 |
| 交易日历 | akshare 新浪全量(1990 起，含未来) → baostock 区间 | |

- **缓存 TTL**（进程内，重启清零——权威数据在上游，这里丢的只是缓存）：
  `/quote` 按标的 3s（一次上游批量请求扇出多个调用方）；`/kline` 按 symbol+区间 300s；
  `/cb_list` 整表 300s；`/calendar` 半天。全市场快照类重接口**不进分钟级轮询**，
  靠缓存 + 限频闸双保险。
- **限频闸**：同源两次外呼强制最小间隔（新浪 hq 0.2s、东财直连 0.5s、集思录 2s 等，
  逐项可用 `ZIMU_DF_GATE_<源名大写>` 覆盖）。
- **熔断**：按源连续失败 ≥3 → open 冷却 60s（期间直接跳下一源，不空等）；
  冷却后半开放行一次探测，成功即闭合。

## 验收「主源故障时降级不崩」怎么做

故障注入开关 `ZIMU_DF_DISABLE`（逗号分隔源名，见下环境变量表）：

```bash
ZIMU_DF_DISABLE=sina_hq uv run uvicorn zimu_datafetch.server:app --port 8300
curl "http://127.0.0.1:8300/quote?codes=sh600519,sh510300"   # → 200，source=tencent-qt，degraded=true
ZIMU_DF_DISABLE=sina_hq,tencent_qt,em_push2 uv run uvicorn zimu_datafetch.server:app --port 8300
curl "http://127.0.0.1:8300/quote?codes=sh600519"            # → 502 UPSTREAM_ALL_FAILED（JSON 契约，进程无恙）
curl "http://127.0.0.1:8300/kline/sh600519"                  # → 200：另一条链，实时全挂不影响日线
curl "http://127.0.0.1:8300/healthz"                         # → 200
```

单测（41 项，`uv run pytest`）覆盖兜底顺序、逐标的补齐（B14）、熔断跳源、
注入、缓存、`no_data` 归一、错误契约形状、cb_list 降级、日历兜底、新浪/腾讯
解析器纯函数（真实抓包样本）。退市回归样本：`sh110074`（定格价 →
`halted_or_delisted`）、`sz127022`（空行 → `empty_or_delisted`）；新浪未收录
样本：`sh000832`（空名零价 → 不出行，交下一源——不再伪装 halted）。

## 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `ZIMU_DF_DISABLE` | 空 | 故障注入：逗号分隔源名（如 `sina_hq,tencent_qt,em_push2,sina_calendar`），链上直接跳过并记 attempt |
| `ZIMU_DF_QUOTE_TTL` / `ZIMU_DF_KLINE_TTL` / `ZIMU_DF_CB_TTL` / `ZIMU_DF_CALENDAR_TTL` | 3 / 300 / 300 / 21600（秒） | 各类目缓存时长 |
| `ZIMU_DF_BREAKER_THRESHOLD` / `ZIMU_DF_BREAKER_COOLOFF` | 3 / 60（秒） | 熔断阈值与冷却 |
| `ZIMU_DF_GATE_<源名大写>` | 见 `config.DEFAULT_GATES` | 单源限频闸间隔 |
| `ZIMU_DF_MAX_QUOTE_SYMBOLS` | 50 | 单次 `/quote` 合并上限 |

## 与业务口径的边界

sidecar 只做**通道与行归一**，不定义任何业务口径：涨跌停、费用、摊薄、仓位全部在
`zimu-domain`（`03` 唯一来源）；ETF 的 `limit_pct` 属标的属性，由数据层（holdings
meta）配置携带，**不在 sidecar 推断**。成交量单位：股票/ETF 统一到股/份（东财通道
×100），指数成交量沿用源口径且契约不消费；转债日线成交量为源侧「张」口径，
P4-T4 适配器接 `03` 时如需量比再校准。

## 文件

| 文件 | 用途 |
|---|---|
| `zimu_datafetch/app.py` | FastAPI 工厂 + 端点 + 错误渲染（`create_app` 可注入假 service） |
| `zimu_datafetch/service.py` | 兜底链编排：缓存→禁用注入→熔断→限频→调用（降级如实上报） |
| `zimu_datafetch/sources.py` | 各源 I/O：新浪 hq/东财直连/akshare 新浪系/baostock/集思录 |
| `zimu_datafetch/governor.py` | TTLCache / Breaker / RateGate 三件套 |
| `zimu_datafetch/symbols.py` | 前缀校验（B7）、号段路由、日期归一 |
| `zimu_datafetch/config.py` | 源名（契约字符串）与全部默认参数（env 可覆盖） |
| `zimu_datafetch/errors.py` | `DfError` 与错误契约表 |
| `zimu_datafetch/server.py` | uvicorn 入口 |
| `tests/` | 36 项离线单测（假源注入，不打网络） |
| `spike_probe*.py` | P2-T1 三轮探测脚本（保留：通道结论的复现入口） |

`pyproject.toml` / `uv.lock` 由 uv 管理（lock 入库）。spike 版 `app.py` 已由本包取代删除。
