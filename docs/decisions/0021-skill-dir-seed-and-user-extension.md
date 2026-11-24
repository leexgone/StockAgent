# 0021 · 技能目录改回 .robit/skills：系统技能初始化播种 + 用户自增

- **状态**：接受
- **日期**：2026-11-24
- **决策者**：主人拍板（技能目录就用 `.robit/skills`，应用初始化把系统技能播种进去，允许用户自行添加技能；播种语义缺则播种永不覆盖；比对粒度是技能的具体子目录不是 skills 根目录），Claude 协作实施

## 背景

P4-T6 起的 skill 单源政策：模板只从仓库根 `skills/` 加载（`load_skills_from_dirs` 单目录，R4 官方入口），刻意不读 `<working_dir>/.robit/skills`——理由是 `.robit/` 整目录被 gitignore（robit 运行时数据），而 Skill 模板是产品文案资产，必须入库随代码评审（内嵌 03 口径与免责边界）。ADR-0014 据此拒绝 `bootstrap_with`（强引全局/项目 skill 目录）。

政策挡住了用户扩展：主人在 NAS 卷 `/data/.robit/skills` 下自增了新技能，服务加载不到——容器里 `ZIMU_SKILLS_DIR=/app/skills` 指向烘焙层，卷上的技能目录根本不在加载路径里。产品资产入库评审的诉求与用户运行时扩展的诉求需要两全。

## 决定

1. **D1 · 技能目录 = `<working_dir>/.robit/skills`**：与 robit 默认项目级技能目录逐字同款（robit bootstrap.rs `working_dir.join(".robit/skills")`）。容器里 working_dir=/data，技能落命名卷；开发期 = `backend/.robit/skills`（gitignore 已覆盖，用户技能不入库）。加载单源 = 该目录（系统已播种 + 用户自增）。
2. **D2 · 系统技能在应用初始化播种（Rust 侧）**：`AgentCore::assemble` 装配 skills 时，把播种源（`skills_dir()`：`ZIMU_SKILLS_DIR` 覆盖，否则编译期 manifest 定位仓库根 `skills/`；容器烘焙 `/app/skills`）里的系统技能复制进技能目录。server/CLI/dev 三宿主统一走装配线，不放 entrypoint.sh。**ZIMU_SKILLS_DIR 语义从「加载目录」变为「系统技能播种源」**，env 值与镜像 COPY/ENV 不变。
3. **D3 · 缺则播种永不覆盖，粒度 = 单个技能子目录**：逐个枚举播种源子目录，目标同名目录**存在即跳过**（用户修改保留），缺失才整目录复制；比对的是技能的具体目录（如 `post-market-review/`），**不是 skills 根目录**——根目录已存在且含用户技能时（NAS 现状），仍逐个补缺系统技能。用户自增目录（播种源无同名）永不碰，任何文件不自动删除。复制走临时同级目录 + rename（防半截播种锁死；并发双实例时后到者见目标已存在视为成功）。
4. **D4 · 仓库 skills/ 仍是产品资产评审源**：系统技能的唯一真值仍在仓库（入库、评审、随版本演进），`.robit/skills` 是运行时合并空间（系统播种副本 + 用户扩展）。原单源政策的理由经「播种源」保留，不是放弃评审。
5. **D5 · 全局目录仍不叠、bootstrap_with 仍不用**：不读 `~/.robit/skills`（个人全局模板不进服务），装配仍走 `create_tools_from_config_with` 内核——skill 加载主权仍在子木（ADR-0014 核心决策不变，只是目录约定演进）。

**停用/退役语义**：停用某技能用 frontmatter `enabled: false`（D3 语义下持久保留）；删除系统技能目录下次启动会重播种（不是停用手段）；上游退役的系统技能在运行目录残留直至手删（已知局限，不自动删用户可见文件）。

## 选项与取舍

- **维持仓库单源、用户扩展另开目录**（否——用户明确要用 robit 约定的 `.robit/skills`，另开目录违背框架惯例且 NAS 已按此目录放置技能）。
- **每次启动刷新系统技能（覆盖同名目录）**（否——主人拍板缺则播种永不覆盖，与人格/config 同款；系统技能升级不自动刷新，想采用新版手动删该目录重播种，08-release 升级节同款指引）。
- **版本比对刷新**（否——多一套比对逻辑，行为依赖用户是否 bump frontmatter version，复杂度不值）。
- **entrypoint.sh 播种**（否——只覆盖容器，dev/CLI 不受益；主人原话「应用初始化时」，Rust 装配线三宿主统一）。

## 后果

**正面**：用户可在 `.robit/skills` 自增技能并被加载（NAS 事故直接修复）；系统技能开箱即用（首跑自动播种）；用户对系统技能的本地修改持久保留；产品资产评审链不变；容器几何零改动（COPY/ENV 值不动，卷上多一个技能目录）。

**负面 / 接受的成本**：

- 升级不刷新系统技能——NAS 上系统技能停在首播种版本，口径演进须手动删目录重播种（与人格/config 升级语义一致，08-release 已写明）。
- 开发期改仓库 `skills/` 模板后，本地 `backend/.robit/skills` 旧副本不会自动更新——删对应目录重播种（测试用 temp 目录不受影响）。
- 退役系统技能残留（已知局限）；停用须走 `enabled: false` 而非删目录。

**取代关系**：本 ADR 取代 skills.rs 模块 doc / backend/README「不放 `<workdir>/.robit/skills`」的单源目录条款；ADR-0014「不用 bootstrap_with」决策继续有效（其 skill 单源政策措辞补记指向本 ADR），ADR-0018 D5 烘焙几何补记（env 语义演进）。

**以后要改回来得动哪里**：skills.rs 恢复单源 `load_skills_from_dirs(&[skills_dir()])`、删播种函数与测试、assembly.rs 调用还原、CLAUDE.md/README/08-release 目录口径还原，并置本 ADR 已取代。