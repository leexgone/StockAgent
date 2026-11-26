/**
 * 基准对齐（03 §7「基准对齐」段，P7-T10）——镜像 `backend/crates/zimu-domain/src/bench.rs`。
 * 把基准指数日线按快照日 carry-forward 对齐并归一到区间净值起点：perf 的 bench
 * 输入形态（「已归一到同一起点」）的唯一口径实现，黄金向量 `fixtures/golden/bench.json`。
 * 原前端单侧实现 lib/bench.ts（P5-T2）已改为薄委托（KlineBar → BenchBar 适配）。
 *
 * 纯函数纪律：不排序、不校验入参有序性（前置条件 = dates/bars 均升序，调用方保证）；
 * 日期比较是 YYYY-MM-DD 字符串字典序（= 时间序）。null 语义（03 附录固定口径 6）：
 * 整段不可对齐 ≠ 数值 0——调用方以空 bench 喂 perf 走 bench_* 全 0 降级形态。
 */

/** 基准日线 bar（只消费 date/close；close 无效值在对齐前过滤） */
export type BenchBar = {
  /** 交易日 YYYY-MM-DD */
  date: string;
  /** 收盘价；null / ≤0 / NaN = 无效 bar（对齐前过滤，不占指针位） */
  close: number | null;
};

export type AlignBenchInput = {
  /** 快照日序列（升序，与 nav 同序） */
  dates: string[];
  /** 基准日线（升序） */
  bars: BenchBar[];
  /** 归一锚 = 区间首个净值点 */
  nav0: number;
};

/**
 * 基准对齐主函数（03 §7 基准对齐段，判定序 = 文档条目序）：
 * ① bars 空或 dates < 2 → null；② 无效收盘过滤后为空 → null；
 * ③ carry-forward（只向后不向前；首快照日无值 → null；同日多根取输入序靠后）；
 * ④ 归一 raw[i]/raw[0]×nav0（raw[0] ≤ 0 或 nav0 ≤ 0/NaN → null）。
 */
export function alignBench(inp: AlignBenchInput): number[] | null {
  // ① 退化分支
  if (inp.bars.length === 0 || inp.dates.length < 2) return null;
  // ② 无效收盘过滤（null / ≤0 / NaN——NaN > 0 为 false，自动出局）
  const closes: [string, number][] = [];
  for (const b of inp.bars) {
    if (b.close !== null && b.close !== undefined && b.close > 0) closes.push([b.date, b.close]);
  }
  if (closes.length === 0) return null;
  // ③ 双指针 carry-forward：每个快照日取「当日或此前最近」有效收盘。
  //    首快照日就无值（区间起点早于日线窗口）→ null，整段不画不伪装。
  const raw: number[] = [];
  let last: number | null = null;
  let j = 0;
  for (const d of inp.dates) {
    while (j < closes.length && closes[j][0] <= d) {
      last = closes[j][1]; // 同日多根：指针扫过全部 ≤d 的 bar，取输入序靠后者
      j++;
    }
    if (last === null) return null;
    raw.push(last);
  }
  // ④ 归一到区间净值起点（base/nav0 无效 → null；NaN 的 !(x>0) 判定同 Rust）
  const base = raw[0];
  if (!(base > 0) || !(inp.nav0 > 0)) return null;
  return raw.map((v) => (v / base) * inp.nav0);
}
