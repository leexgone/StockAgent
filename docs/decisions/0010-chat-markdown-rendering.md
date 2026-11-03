# 0010 · 助手消息 Markdown 渲染：react-markdown + remark-gfm

- **状态**：接受
- **日期**：2026-11-03
- **决策者**：leexgone（产品所有者），Claude Code 辅助整理

## 背景

chat 屏会话面板（frontend ChatPanel）此前把助手消息按纯文本渲染（`white-space: pre-wrap` 原样输出），而 Skill 模板要求助手输出结构化 Markdown——`pre-market-watch` 要求「关键价位表」（表格）、三段小节（标题/列表），`generate-advice` 要求三段式 + 风险自陈列表，模型实际产出普遍含 `**加粗**`、`-` 列表、GFM 表格。原样糊出时 `#`、`|`、`**` 全是噪点。

约束：ADR-0008 第 4 条「不引 UI 组件库、图表库、Tailwind——要引先补 ADR」。Markdown 渲染器是文本展示工具而非上述三者，属灰区，按精神补本 ADR 定边界。安全面：助手文本来自真实 LLM 流（SSE `text_delta`），内容不受程序控制，可能混入 HTML 标签或恶意链接（prompt 注入面）。

## 决定

1. **依赖 = `react-markdown` + `remark-gfm`**（仅此两个，不引 `rehype-raw`、不引 `marked`/`markdown-it` + `DOMPurify`）。
   - react-markdown 渲染产物是 React 元素：**原始 HTML 默认不渲染**（无 `dangerouslySetInnerHTML`），LLM 输出里的 `<script>`/`on*=` 只是被丢弃，XSS 免疫是结构性的，不靠净化器兜底。
   - 不安全协议（`javascript:` 等）由 react-markdown 内置 urlTransform 默认剥除。
   - remark-gfm 补表格/删除线/任务列表——价位表是硬需求。
   - 排除「marked → HTML 字符串 + DOMPurify + innerHTML」路线：引两个包、要净化器才安全、且绕过 React 转义模型。
2. **渲染范围 = 仅助手气泡**。用户消息、notice、tool 条目保持纯文本——用户输入无需 Markdown，同时堵住格式注入；tool 参数/结果是 JSON 原文，渲染成 Markdown 反而破坏可读性。
3. **渲染守卫（组件映射 `components`）**：
   - `a` → 新标签页打开 + `rel="noopener noreferrer"`；
   - `img` → 不渲染（助手文本回答不该出现图片，出现即按注入面处理，丢弃）；
   - `table` → 外套横向滚动容器（窄屏/宽表不撑破气泡）。
4. **样式 = 设计令牌，作用域 `.chat-md`**：正文 13px/1.7、标题收敛到 ≤15px（气泡不是文档页）、表格 `font-variant-numeric: tabular-nums` + 数字走 `--font-mono`、边框 `--border`、表头底 `--surface-2`、代码沿用 chat.css 现有全局 `code` 样式。**链接不用 `--accent`**（每屏 ≤2 处的预算保护），用继承色 + 下划线。三套涨跌语义色规则照常适用。
5. **流式兼容 = 零协议改动**。SSE wire、robit、zimu-server 全部不碰；ChatPanel 本就累积整段文本再渲染，每个 `text_delta` 重渲整段（气泡文本尺寸下开销可忽略）。空白帧守卫与 `turn_complete` 收口（10c1e26 落地）语义不变。同日拍板：流式回复**去掉闪烁光标**（`chat-streaming::after` 删除）——进行状态由输入框「等待本轮回复…」占位与「取消」按钮承担，正文到达本身即进度指示。
6. **历史兼容 = 无迁移**。localStorage 会话存的是原始 Markdown 文本，展示层变化不影响已存档数据；旧会话重载即得新渲染。

## 后果

- `frontend/package.json` 运行时依赖 +2：`react-markdown`、`remark-gfm`（合计 gzip ~50–90KB，对本机常驻 Web 服务可忽略）。
- ADR-0008 第 4 条的「组件策略」获得一个已记录例外：**文本渲染工具不属于 UI 库/图表库/CSS 框架**，但任何后续想引的同类库（语法高亮 KaTeX 之类）仍需先补 ADR。
- `docs/02` chat 节补展示口径一段（助手 GFM、其余纯文本、守卫三条）。
- 渲染正确性靠人工走查（chat.css/ChatPanel 无自动化渲染测试）：验证方式 = 让助手跑一次 `/盘前` 或普通问答，确认表格/列表/加粗成形态、原始 HTML 不出现。
