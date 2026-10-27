/**
 * 子木 · 领域纯函数库 TS 共享层（P3-T4）。
 *
 * 口径唯一来源：`docs/03-domain-rules.md`（正文 + 附录「黄金测试向量」）；
 * 镜像 Rust 侧 `backend/crates/zimu-domain/`——模块划分、字段名、运算顺序一致。
 * 黄金向量（仓库根 `fixtures/golden/*.json`）由两边各自的 runner 跑同一份文件
 * （Rust：`cargo test -p zimu-domain`；TS：本目录 `golden.test.ts`，`npm test`），
 * 两边实现永不互相「对拍」，只对 fixture。
 *
 * 约定：
 * - **纯函数**：无 I/O、无全局状态、无时钟；供抽屉试算等即时交互使用（P5 消费）。
 * - **字段名 snake_case**：与 fixture JSON 键（及 Rust serde 结构体）一一对应；
 *   函数名按 TS 惯例 camelCase，fixture 的 `fn` 串到函数的映射在 runner 里。
 * - **number（IEEE754 double）算术**：与 Rust f64 同数系，运算顺序照抄实现，
 *   两边逐位等价；黄金向量按容差比对（03 附录比对规则）。
 * - **除零防护**：分母为 0 时各比率取 0（`ratio`）；阈值判定带 `THRESHOLD_EPS`
 *   守卫，压线按含边界语义（03 附录约定 4）。
 * - 改口径的顺序：**先改 `docs/03` 与 fixtures，再改本层与 Rust 侧**。
 * - `price_limit`（涨跌停）已随 P1-T2 定稿（2026-10-28），fixture 组全部 active。
 */

export * from "./common";
export * from "./lot";
export * from "./fees";
export * from "./dilution";
export * from "./recalc";
export * from "./discipline";
export * from "./perf";
export * from "./vol_synth";
export * from "./price_limit";
