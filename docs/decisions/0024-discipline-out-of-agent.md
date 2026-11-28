# 0024 · 仓位纪律彻底退出智能体：仅 Web 端测算展示，模型行为调整走技能

- **状态**：接受
- **日期**：2026-11-28
- **决策者**：主人拍板（智能体零纪律、纪律只留 Web 测算、未来影响经技能引入），Claude 协作探查与设计

## 背景

ADR-0023 把纪律从「拦截」降为「参考线」，但纪律仍然占据智能体的全部可见面：`get_holdings` 返回 `discipline` 体检块、专用工具 `calc_discipline`（四条判定 + reduce_hint）、`calc_advice` 返回的 `discipline_refs` 与四个 `*_pass` 布尔、帽锚位仓位文案、人格提示词「参考线」专段、四个技能模板各自的纪律体检/转述段落。参考线仍是判断框架——模型说话依旧被纪律牵着，与「充分发挥大模型作用」的产品思路冲突。主人拍板新阶段分工：**纪律的作用仅限于 Web 端给出相关测算展示；对智能体完全去掉纪律影响，让模型基于中性事实自主发挥；未来需要对模型施加的影响，通过引入技能（SKILL.md）调整**。

摸底结论（决定改动半径）：Web 端三处纪律展示（持仓屏红绿灯卡、诊断屏再平衡清单、TradeDrawer 超限警告）**全部前端现算**——`src/domain` 的 `LIMIT_*`/`sell_to`/dilution 常量 + `/api/holdings` 的 agg/per 原值，服务端 HTTP 面本就不下发纪律判定（`PortfolioView` 无 discipline 块）；唯一的服务端纪律数据通道是 `/api/advice → AdviceRow.discipline_refs` 存值透传。故「纪律退回 Web 测算层」在架构上是顺势收缩，契约与存储可以整体不动。

## 决定

1. **D1 · 智能体可见面零纪律**：`calc_discipline` 从 agent 工具面删除（领域工具 15→14，注册 25/26–32）；`get_holdings` 返回去 `discipline` 键；`calc_advice` 返回 JSON 去 `discipline_refs` 与 `discipline` 四布尔块、去帽锚位文案，留档 `discipline_refs` 恒空 `[]`；人格提示词删「参考线」专段（守护子串 13→12）；四个技能模板删纪律步骤/段落/转述规则与 `tools_required` 声明。
2. **D2 · 打分链回归纯得分表**：`AdviceInput.discipline_ctx` 退役（继 `cash_reserve_pass` 之后第二个退场的纪律输入键），`score_advice` 不再调用 `discipline::check`——偏多带决策表三分支：未持有 → 买入至 15%；已持有 `w<15−ε` → 加仓至 15%；其余（**含超 25% 的重仓票**）→ 持有维持。`AdviceOutput.discipline_refs` 字段与 `build_discipline_refs` 整条删除，两侧（Rust/TS）同构，黄金向量 advice 组 11 例键换形（计数不变）。15% 目标带本就是得分锚非纪律，不受影响。
3. **D3 · Web 测算展示保留且只在 Web**：红绿灯卡、再平衡清单、抽屉警告零逻辑改动（本来就前端现算）；建议卡纪律注记降级为**历史留档查看器**——旧行按存值照常渲染，新行恒空自然隐藏；持仓屏纪律卡文案定性「界面端测算，智能体建议不含纪律判定」。`zimu-domain::discipline` 模块与 §6 黄金向量**完整保留**——它是 Web TS 镜像的拍板基准；`sell_to` 泛式仍被 `calc_advice` 的 `sell_qty_hint` 复用（目标来自得分锚 15%，公式借用不构成纪律判断，03 §6 定位注同述）。
4. **D4 · 契约与存储刻意不动**：zimu-store v5 列、`AdviceRow`/`NewAdvice` 字段、`map_advice_row` 兜底全部保留（历史行照常读，迁移只增不减的房规不破）；openapi/schema.d.ts 形状不变零漂移；`/api/*` 无任何端点增删。
5. **D5 · 未来影响通道 = 技能**：对模型行为的调整一律经 SKILL.md 模板引入（产品文案资产、入库评审、ADR-0021 播种面），不再往人格提示词或工具返回结构里塞判断框架。人格只保留安全底线（永不代下单、数字来源、账本纪律、计划任务自律）。将来若要用技能重新引入纪律数字，届时重新注册数据工具即可——工具面收缩不封死回退路径。

## 选项与取舍

- **保留 calc_discipline 工具、只拆提示词约束（否）**：模型仍可主动调工具拿阈值判定框架，「完全去掉影响」不成立；且工具 description 本身就是纪律话术的载体。
- **domain 继续算 discipline_refs、仅为 Web 落档（否）**：能把「这条建议产生时越了哪些线」留档展示，但纪律逻辑回流 advice 模块、打分链对仓位事实仍有余温，违背「打分链无纪律」的语义一致性（D2）。历史注记需求由旧留档满足。
- **删除 store v5 列 / 回滚迁移（否）**：破坏「迁移只增不减」纪律，且旧行数据作废；列保留恒空、读路径兜底不变，成本为零。
- **技能里保留「纪律四条」作为中性数据段（否）**：pass/threshold 比对就是纪律判断；HHI、行业分布、最大单票占比等中性统计本就来自 recalc 原值，保留为「组合结构/集中度画像」，禁阈值对比。

## 后果

**正面**：模型拿到的是无判断框架的中性事实（价格、权重、行业、盈亏），结论完全自主；技能成为行为调节的唯一活性层，改动半径从「代码+契约+存储」缩到「一份 markdown 播种文件」；HTTP 契约与迁移零变化，部署侧无破坏性升级动作。
**负面 / 接受的成本**：
- 建议不再附带任何仓位风险提示框架——超上限重仓票拿到「持有维持」也不会被点名，风险完全由主人在 Web 测算展示与自主阅读中自担（免责表述纪律不变）；
- ADR-0023 的建议内嵌参考字段（其 D3）整体作废，本件**取代 0023**；0023 的历史事实记录（store v5 ALTER 先例、选项否决理由）保留原文不回改；
- 部署实例的人格与四份系统技能因 ADR-0021「播种永不覆盖」滞留旧文案——须点删卷内 `prompts/agent.md` 与四个技能子目录重播种才生效（红线照旧：勿碰 wechat-state.json / robit.db / config.toml）；
- `discipline_refs` 列成为只读化石列（新行恒空）；将来纪律回潮若经技能引入，需重开 ADR 说明重新注册工具的理由。

**以后要改回来得动哪里**：`advice.rs`/`advice.ts` 决策表加回纪律输入 + refs 构建（git 历史 `3a8b8a5`~`5d7f719` 区间可考全部实现）、`fixtures/golden/advice.json` 期望回写、`tools/portfolio.rs` 恢复 CalcDisciplineTool 并回 `DOMAIN_TOOL_NAMES`、get_holdings/calc_advice 返回补纪律键、agent.md 参考线段 + persona 守护第 13 子串还原、四技能 frontmatter `tools_required` 回补、置本 ADR 已取代。
