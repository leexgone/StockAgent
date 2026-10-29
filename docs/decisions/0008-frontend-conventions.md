# 0008 · 前端工程约定：路由、API client、状态方案与组件策略

- **状态**：接受
- **日期**：2026-10-29
- **决策者**：leexgone（产品所有者），Claude Code 辅助整理

## 背景

P5-T1：7 屏从 `design/` 原型移植进 `frontend/`（React 19 + Vite 6 + TS 5.6，ADR-0005 已定）之前，需要定死四件事——路由方案、API client、状态方案、组件策略——否则 T2 逐屏移植会各屏各写一套。现状：SPA 只有视图状态切换（`App.tsx` 注释「路由方案 P5-T1 再定」）；契约类型已由 `openapi-typescript` 生成（`src/api/schema.d.ts`，P4-T5）但无消费入口；服务端 SPA 回落已就位（`zimu-server` main.rs `ServeDir.fallback(index.html)`，深链刷新不 404）；robit-gui 的状态基线是 zustand ^5。

## 决定

1. **路由 = react-router v7（library mode，BrowserRouter）**。路由表单一来源 `src/routes.ts`，路径与原型文件名一致：`/`（工作台）、`/dashboard`、`/holdings`、`/analysis`、`/chat`、`/diagnosis`、`/wechat`；未知路径回落 `/`。未移植屏渲染占位屏（PlaceholderScreen），T2 每移植一屏把 `ported` 翻真。URL 即导航状态：预警→建议等深链（如 `/analysis?code=600519`）可刷新、可收藏。
2. **API client = openapi-fetch**，唯一 REST 入口 `src/api/client.ts`（`createClient<paths>`，类型来自 `npm run gen:api` 生成的 `schema.d.ts`）。全仓禁止手拼 URL 字符串与裸 fetch；**唯一例外**：SSE 会话 `/api/chat*`（流式读取不在 openapi-fetch 覆盖面内，ChatPanel 保持原生 fetch + ReadableStream）。契约变更流程：后端再生成 `openapi.json` → `npm run gen:api` → 类型漂移由 `tsc` 暴露。
3. **状态 = zustand ^5**（与 robit-gui 基线一致）。铁律：**派生数字以后端为唯一真值**——recalc 汇总、打分、预警判定全在服务端（口径唯一实现在 `zimu-domain`），store 只缓存 API payload 与 UI 状态，前端不重算汇总。唯一例外：加减仓抽屉的**实时试算**用 `src/domain/` TS 共享库（P3-T4 黄金向量同款），提交后以后端返回覆盖本地试算值。store 文件按域拆 `src/store/<domain>.ts`（portfolio / alerts / market…，随 T2 逐屏落地，不预建空壳）。
4. **组件策略 = 手写组件 + 设计令牌**：样式沿用 `src/styles/tokens.css` + `ui.css`（自 `design/assets/` 移植），图表手写 SVG 组件镜像 `design/assets/charts.js`（ZMC）。**不引 UI 组件库、不引图表库、不引 Tailwind**——要引先补 ADR（CLAUDE.md 原有约定，此处重申并覆盖 Tailwind）。原型约定全部保留：每屏 `data-od-id`、底部 `*-disclaimer`、格式化统一 `money/fmt/pct/wan`（过渡期在 `data/demo.ts`，demo.ts 删除前抽到 `src/lib/`）、三套涨跌语义不混类、`--accent` 每屏 ≤2 处。

## 选项与取舍

- **路由：手写视图状态（不选）**——现状即如此；7 屏 + 深链（预警→建议带 code、空态 `?empty=1` 类演示参数）会让视图状态退化成手写 router。
- **路由：HashRouter（不选）**——免服务端配合，但 zimu-server 的 SPA 回落已实现且注释明言为客户端路由准备，hash URL 丑且与「常驻 Web 服务」的产品形态不符。
- **client：axios/ky + 手写类型（不选）**——HTTP 库与契约类型两张皮，openapi.json 变了类型不会跟着变；openapi-fetch 直接消费 openapi-typescript 产物，零额外代码生成、~6KB。
- **状态：TanStack Query（不选）**——查询缓存/重试/失效对单人本机应用过重，且 robit 基线无此依赖；刷新链已由后端调度器承担（P4-T7），前端只需拉取。
- **状态：React context 手搓（不选）**——跨屏共享（顶栏 strip、dashboard、holdings 读同一份组合数据）会催生样板代码与全树重渲染；zustand 选择器天然避免。
- **组件：Tailwind / UI 库（不选）**——robit-gui 用 Tailwind + Radix，但子木已有完整的 tokens + ui.css 资产（P1-T4 三轴语义刚定稿），换体系等于重做 7 屏样式且与 `docs/05` 令牌表冲突。

## 后果

- `frontend/package.json` 运行时依赖 +3：`react-router-dom` ^7.18.5、`openapi-fetch` ^0.17.0、`zustand` ^5.0.16（zustand 在 T2 首个 store 落地前暂未被 import，属有意预置基线）。
- 新增屏幕必须走 `routes.ts` 注册 + `Chrome` 导航项，禁止组件内私自切视图。
- `demo.ts` 的 mock 数据随 T2 逐屏被真实 API 替换，最后一屏迁完即删除（`design/` 冻结见 P5-T3）；格式化与时钟工具先抽 `src/lib/` 再删。
- 未来若引 UI 库、图表库或 CSS 框架，属推翻本决策第 4 条，须重开 ADR。
