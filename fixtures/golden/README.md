# fixtures/golden · 黄金测试向量

`docs/03-domain-rules.md` 数字口径的唯一权威样例集。Rust 领域库（P3-T3，`backend/`）与 TS 共享层（P3-T4，`frontend/`）跑**同一份文件**；任何公式改动先改这里与 `03` 正文，再改实现。

- 文件格式、比对规则、公式组 → 函数映射、新增约定：见 `docs/03-domain-rules.md` 附录「黄金测试向量」。
- `status: "pending"` 的组（如 `price_limit`）口径未定稿，runner 必须跳过。
- 每个 case 的 `desc` 是手算过程（含精确分数）；期望值必须能在纸上复核。
