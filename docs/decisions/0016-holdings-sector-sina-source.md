# 0016 · 持仓行业数据源：新浪行业板，挂刷新链顺带补 sector

- **状态**：接受
- **日期**：2026-11-15
- **决策者**：主人拍板（C14 四项决策），Claude 协作探查与实施

## 背景

券商持仓 CSV 导入诊断（2026-11-15）：导出 19 列**无行业列**，`upsert_holding.sector` 可选缺省空串，刷新链只回写 `price/prev/name/limit_pct` → `holdings.sector` 恒空（类型 `kind` 已正确，智能体按名称推断 stock/etf 全对）。

连带一处**现存失真**：`zimu-domain::recalc`/`discipline` 按 sector 字符串精确分组、无空串特判（`recalc.rs:119`），行业恒空时全组合聚成一个 `""` 桶 → `hhi=1`、`top3_sector=100`、§6 单一行业纪律**恒 fail**，且每天落进 `equity_snapshots`。diagnosis 行业集中度、dashboard 行业重叠标注、vol_synth 同行业 ρ 判定全部失真。

主人在三条补行业路径（① 对话手工指定 ② 智能体常识标注+确认 ③ 接行业数据源）中拍板**方案 3**，并完成 C14 四项设计决策。本轮补行业数据源闭环。

## 决定

1. **数据源 = 新浪行业板**（单源）：`ak.stock_sector_spot(indicator='新浪行业')`（49 板）+ `ak.stock_sector_detail(sector=label)`（成分股）→ 建「6 位代码 → 行业板名」映射，**板名直用**（金融行业/电力行业/有色金属…，不做映射表）。sidecar 新增 `GET /sector_map`（无参返回全量，日级 TTL 缓存 + singleflight + 部分板失败降级）。
2. **ETF/转债单列类别**：ETF→`基金`、转债→`转债`，**Rust 侧按 `kind` 判定**（不查映射、sidecar 不推断——标的归类是业务口径，归消费侧）；股票查映射。板名「基金」「转债」不在新浪 49 板内（实测核对，无碰撞）。
3. **挂行情刷新链顺带补**：在 `service::refresh_portfolio_detailed` 内、`apply_quotes` 之后、`recalc` 之前插入补全 pass，不建独立调度链。pass 遍历**全部持仓**（含停牌/退市），不被 quote `status==ok` 门控（sector 是标的属性、与行情状态无关）。三个既有刷新触发点（HTTP refresh / `refresh_quotes` 工具 / 调度器盘中与盘后）自动顺带补。
4. **只补空串，不加锁**：自动补全仅写 `sector==''` 的行（store `fill_sectors` 用 `WHERE sector=''`）；手工非空值永不被自动覆盖。`upsert_holding` 整行覆盖语义保留，**无 schema 迁移**（版本恒 4）。用户把 sector 改回空串 = 重新请求自动分类。
5. **空串桶口径定稿**：未分类（空串）标的聚一桶**参与**集中度/HHI/top3/vol_synth ρ（保守 fail-safe，**不改分组实现**）；03 §6 明文 + 四个黄金向量用例钉死（`rc-empty-sector`/`dc-unclassified-all`/`dc-unclassified-mixed`/`vs-empty-sector`）。
6. **三零改动**：**robit 零改动**（纯子木 datafetch+market+store+agent）、**前端零改动**（行业列/集中度卡/重叠标注有值即显示）、**wire 契约零改动**（`SectorMapPayload` 照 `SidecarHealth` 先例不 derive ToSchema、不进 ApiDoc；`RefreshResponse` 不加字段，补全效果由 `portfolio.holdings[].sector` + `agg.sectors` 透出）。

## 选项与取舍

- **数据源**：东财行业板/个股信息（`stock_board_industry_name_em`/`stock_individual_info_em`）——**本环境实测 ConnectionError 不可达**（与 docs/04 附录 A 既有「东财被拦」结论一致），否；baostock `query_stock_industry`——可达但返回**证监会粗分类**（「J66货币金融服务」代码需映射、退市股大量空值），与新浪板名是两套体系不宜混用，否；申万一级——无免费稳定通道，否；**新浪行业板**——可达、49 板覆盖全 A 股、与 sidecar 主源同族、docs/04 附录 A 早已记为验证通道，**选**。
- **ETF/转债归属**：单列类别（**选**，简单诚实、§6 已有转债合计≤12% 独立规则、集中度按真实股票行业算）；穿透归类（ETF 按跟踪指数/转债按正股，需额外数据源且部分不可得，否）；留空（未分类桶仍参与集中度，否）。
- **补全时机**：挂刷新链顺带补（**选**，复用现链与三个触发点、需求驱动稳态零开销）；独立低频调度链（更解耦但新增链路与装配面，主人选挂刷新链）；仅导入时一次性（新标的/重分类不跟进，否）。**缺口已补**：补全 pass 遍历全量持仓含停牌退市，不被 quote 状态门控；仅「quote 整链失败（sidecar 全挂）」时按原契约早退不补（此时映射也不可得）。
- **手工值保护**：只补空串不加锁（**选**，免 schema 迁移、两写路径 `BEGIN IMMEDIATE` 串行化手工恒赢）；`sector_locked` 列（保护更强但需迁移 + store/工具/表单三处同步 + 界面开关，否）。
- **空串桶**：聚一桶参与集中度（**选**，保守诚实——未分类是真实的集中度未知，跳过会低估风险；现状即此语义零实现改动）；跳过空串（需四处实现加特判 + 重定分母，低估风险且违背「降级不伪造」，否）。
- **端点入参**：无参全量（**选**，建映射成本恒 1+49，codes 过滤省不了上游成本反增 B7 校验面与缓存键复杂度；全量 ~5000 条回环小 payload）；codes 过滤（否）。
- **缓存**：sidecar 日级 TTL + Rust 24h `SectorMapCache`（stale 回落）双层（**选**，永久未映射代码会令每轮刷新都打 sidecar，Rust 缓存压到每日一次；sidecar 重启缓存清零时 stale 回落避免干等 30–90s 重建）；仅 sidecar 缓存（否）。

## 后果

**正面**：行业列/行业集中度卡/行业重叠标注/vol_synth 再平衡测算自动受益（前端零改动）；修掉 sector 恒空导致的 hhi=1/§6 恒 fail 失真；手工值优先、自动补全幂等。

**负面 / 接受的成本**：
- **冷建延迟**（最大风险）：首轮建映射 1+49 次新浪调用 30–90s，阻塞该轮刷新与调度 tick。缓解内置：`sector_map` 单请求 **180s 超时**（默认 10s 必超时）、singleflight、需求驱动（补完零开销）、Rust stale 回落。残余：sidecar 重启后首轮重建仍慢，文档明示。
- **已补值不随板成员漂移更新**：决策 4 的直接后果——重分类走手工 `upsert_holding`（写进 03 §13）。
- **永久未映射代码**（新股未入板/退市摘除）：留空串不伪造，Rust 24h 缓存把重试压到每日一次。
- **部分板失败**：降级缓存部分映射，缺码留空至次日 TTL（`ZIMU_DF_SECTOR_TTL` 可调短）。
- **akshare 列名/板成员漂移**：source KeyError → 502 → 本轮跳过（诚实），`live_smoke.py` 是哨兵。

**以后要改回来得动哪里**：换数据源 → 改 `datafetch/sources.py:sina_sector_map` + 新 ADR 取代本条；改保护语义（加锁）→ store `fill_sectors` 的 WHERE + `upsert_holding` + schema 迁移 + 表单；改空串桶口径 → `recalc.rs`/`discipline.rs` 与 TS 镜像四处 + 对应 fixture（口径先行）；改补全时机（独立链）→ `service.rs` pass 抽出 + 调度器装配。