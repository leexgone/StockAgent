# 决策记录（ADR）

这里有且只放「为什么不那样做」。代码会自己解释怎么做，文档要解释为什么选这条路——半年后没人记得。

## 什么时候补一条

- 在两个可行方案里选了一个
- 明确决定**不做**某件事
- 推翻了之前的决定
- 引入了新的依赖、框架、服务、数据源

改一个界面不需要 ADR；给项目加一套构建工具需要。

## 格式

文件名 `NNNN-<kebab-slug>.md`，编号递增，不复用、不删除（被取代的改为 `已取代 by NNNN`）。

```markdown
# NNNN · 标题

- **状态**：提议 / 接受 / 已取代（by NNNN）
- **日期**：YYYY-MM-DD
- **决策者**：

## 背景
什么压力让我们必须选一个。

## 决定
选了什么，一句话说清。

## 选项与取舍
A …（选/不选，为什么）
B …

## 后果
正面、负面、以及「以后要改回来得动哪里」。
```

## 索引

| 编号 | 标题 | 状态 |
|---|---|---|
| [0001](./0001-docs-before-code-scaffold.md) | 先固化文档与口径，不建代码骨架 | 接受 |
| [0002](./0002-prototype-stays-buildless.md) | 原型保持零构建纯 HTML 形态 | 已取代（by 0005） |
| [0003](./0003-defer-stack-choice.md) | 暂缓后端与前端框架选型 | 已取代（by 0005） |
| [0004](./0004-demo-data-behind-contract.md) | 演示样本固定在 `ZM` 契约后面 | 接受 |
| [0005](./0005-tech-route-rust-robit-react-sidecar.md) | 技术路线：Rust + robit 基建、React 前端、Python 行情 sidecar | 接受 |
| [0006](./0006-first-position-entry-manual-first.md) | 首笔持仓录入：手工逐笔先行，CSV 导入作后续增强（C5） | 接受 |
| [0007](./0007-hybrid-scoring-rules-plus-model.md) | 分析结论生成：规则打分 + 模型写文案的混合方式（C8） | 接受 |
| [0008](./0008-frontend-conventions.md) | 前端工程约定：路由表单一来源 + 契约客户端 + zustand + 手写组件 | 接受 |
| [0009](./0009-direct-holding-maintenance.md) | 持仓直接维护：快照校正三入口，不动现金不写流水 | 接受 |
| [0010](./0010-chat-markdown-rendering.md) | 助手消息 Markdown 渲染：react-markdown + remark-gfm | 接受 |
| [0011](./0011-agent-channels-and-server-side-history.md) | 智能体接入模块：通道独立会话、服务端真值、QQ 验证通道与配置分界 | 接受 |
| [0012](./0012-agent-ledger-write-tools.md) | 智能体写账本工具面：自动落账 + 读回，流水只增不改 | 接受 |
| [0013](./0013-agent-holding-snapshot-tools.md) | 智能体持仓快照校正工具：初始化与定期校准 | 接受 |
| [0014](./0014-agent-standard-tools.md) | 智能体启用 robit 标准工具面：全套消费、确认关闭自动执行（推翻 R2） | 接受 |
| [0015](./0015-web-attachment-upload.md) | Web 对话框文件上传识别：multipart 随消息发送，robit 零改动 | 接受 |
