# 0023 · 仓位纪律在建议生成链降级为纯参考（参考线，不拦截）

- **状态**：接受
- **日期**：2026-11-26
- **决策者**：主人拍板（全部转纯参考、目标锚保留、建议内嵌参考字段），Claude 协作探查与设计

## 背景

03 §10 生产口径「纪律拦截优先于得分」让四条仓位纪律（单票≤25% / 行业≤35% / 转债≤12% / 现金≥10%，§6）在建议决策表里**硬改写 action**：单票超 25% 的高分票只能拿「持有（建议降至 25%）」、现金不足时高分票强制「买入→观望」。主人判断：这使部分标的无法按行情客观评价——信号结论被仓位事实否决，而仓位纪律本质是风险管理偏好，不是标的事实。要求把纪律在建议链的影响降为**仅参考建议**。

代码面拦截只存在于偏多带四处分支（`advice.rs:323-339`，TS 镜像同构）；其余得分带本就不沾纪律。表达面强锚 = 03 §10 决策表、两侧 domain 实现、`skills/generate-advice/SKILL.md:31`「讲清规则依据」句、`tools/advice.rs position_text` 文案产地。

## 决定

1. **D1 · 四条纪律（含现金）在 score_advice 一律不拦截、不改写 action**：偏多带 action 只由得分 × 持仓形态决定——未持仓高分 = 买入、超上限高分 = 加仓、现金不足照买。15% 目标带判定保留（`w<15−ε → 加仓至 15`；`15−ε≤w≤25+ε → 持有已达目标`——这是**仓位目标锚**，属 sizing 不属纪律，不随本次降级）。held=false 时 weight=0 语义保留。
2. **D2 · 目标锚保留为 suggested 数字来源；超上限偏多票 = 加仓 + 上限锚**：`w>25+ε` 时 `suggested=25%（至上限）`、`mode=Target`、position 文案读作「当前 x% → 建议 ≤25%」（上限锚语义，非降仓指令）；**该情形不给 `sell_qty_hint`**——减仓股数归 `calc_discipline.reduce_hint` 本家链路（§6/红绿灯/再平衡），建议链不再产出反向数量。horizon/target/stop 按加仓带键控派生天然继承（拦截票自此能拿到目标区间，顺带修复旧口径别扭）。
3. **D3 · 建议内嵌 `discipline_refs` 参考字段**：`AdviceOutput` 增 `discipline_refs: Vec<DisciplineRef{rule,label,value,threshold,text}>`——仅未过项入列、**只在偏多带生成**（其余带恒空，口径最小变更）、固定序 = §6 表序（single_stock→sector→cb_total→cash_reserve）、压线=过（`THRESHOLD_EPS` 复用）。输入侧 `cash_reserve_pass: bool` 退役，新增可选 `discipline_ctx{cash_pct?,sector_pct?,cb_total_pct?}`（工具层从 `discipline::check` 现算注入，sector_pct 为**该标的自身行业**占市值比，与 §6 同分子分母；cb_total 仅转债用）。数字仍全部规则侧产出（ADR-0007 不变）：`rule/value/threshold` 进 fixture 比对，`label/text` 展示层不校验（`dims[].t` 同等待遇）。留档经 **zimu-store v5 迁移**（本表首个 `ALTER TABLE ADD COLUMN` 先例）`advice_history.discipline_refs TEXT` 可空列；**旧行 NULL 读回 `[]`**（存储层兜底，前端恒拿到数组）。
4. **D4 · 纪律本体的其余消费链一律不动**：§6 四条公式与 `sell_to`、`LIMIT_*` 常量、holdings 四条红绿灯卡、diagnosis 再平衡清单、§12 预警链（止损/止盈是**交易级防线**，非四条纪律）、TradeDrawer 收据「纪律提示不拦提交」（本就是提示语义）——全部逻辑零改动，仅措辞向「参考线」收口。C6（阈值随规模浮动）与 C15 待拍板议题原样不动。

## 选项与取舍

- **现金留硬闸（否）**：主人拍板全部转纯参考——现金不足买不进是物理事实（资金校验在录入链拦截），不需要建议链替用户保守。
- **超上限高分票 = 持有 + refs 注记（Maintain）（否）**：违反拍板决策 2「目标锚保留为 suggested 数字来源」，且 action 仍被纪律改写（只是换了个标签），降级不彻底。
- **新增 `PositionMode::Cap`（否）**：要同动 Rust/TS 枚举、utoipa、TS union、fixture mode 字符串四处契约，收益仅省一个格式化分支——契约扩散不值，用「action 参入 position_text」解决文案读法。
- **refs 全带生成（否）**：纪律历史上只介入偏多带；中性/偏空带新增附注属口径扩大，最小变更原则只在偏多带出 refs。

## 后果

**正面**：信号结论不再被仓位事实否决——高分重仓票可如实拿到加仓评价；上限/现金属性以可回溯的参考项呈现而非隐藏改判；拦截票拿不到目标区间的旧别扭消失；决策表更简单（纪律分支归零）。
**负面 / 接受的成本**：
- 建议不再替用户保守——超上限继续加仓的风险由主人在 refs 提示下自担（免责与「不构成投资建议」表述纪律不变）；
- `cash_reserve_pass` 删除是 `zimu-domain` 公共 API 破坏（调用点四处已核）；黄金向量 advice.json 九 case 键换形 + 三 case 期望重写 + 新增多 refs case；
- 「加仓 + 建议 ≤25%」方向反差是**刻意语义**（上限锚）——03 §10、SKILL.md、工具 description 三处必须同措辞，否则模型文案会写成自相矛盾（tools.rs e2e 的 text 断言为网）；
- 部署实例的 `generate-advice` 技能模板因 ADR-0021「播种永不覆盖」**滞留旧文案**（要求「讲清拦截规则依据」句与新口径冲突）——须删除卷内该技能目录重播种才生效；技能模板版本化/迁移策略另案；
- advice_history 首开 ALTER 先例，后续加列沿用本模式。

**与 ADR-0007 的关系**：0007 约束的是「数字归规则、文字归模型」——本次改的是规则的**强度**（拦截→参考），数字归属不变（refs 的 value/threshold 全由规则侧产出进留档），0007 状态保持接受，尾注交叉锚。

**以后要改回来得动哪里**：`advice.rs`/`advice.ts` 偏多带决策表恢复拦截分支 + 删 refs 构建、`fixtures/golden/advice.json` 期望回写（旧期望在 git 历史与 `av-*` id 可考）、`cash_reserve_pass` 输入键还原、store v5 不回滚（列保留恒 NULL 亦可）、03 §10/§6 与 SKILL.md 措辞还原、置本 ADR 已取代。
