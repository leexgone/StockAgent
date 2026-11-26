/**
 * 沪深 300 基准对齐（dashboard / diagnosis 共用，P5-T2；P7-T10 起口径上收）。
 * equity_snapshots 无 bench 列，基准序列取自 sh000300 指数日线：
 * 按快照日 carry-forward 对齐（当日或此前最近一根收盘），再归一到区间净值起点
 * （03 §7 perf 的 bench 输入形态：已归一到同一起点）。
 *
 * **口径唯一实现已上收 `src/domain/bench.ts`**（镜像 zimu-domain::bench，黄金向量
 * `fixtures/golden/bench.json`，03 §7 基准对齐段）——本文件只做 KlineBar → BenchBar
 * 适配的薄委托，不再持有独立逻辑。
 * 注意：sh000300 为价格指数（未含分红），非 §7 所述「全收益」——B11 基准口径
 * 待定稿，图下/免责如实标注，不伪装成全收益。
 */

import { alignBench as domainAlignBench } from "../domain";
import type { KlineBar } from "../store/kline";

/**
 * @param dates 快照日序列（升序，与 nav 同序）
 * @param bars  基准日线（升序）；undefined/空 → null
 * @param nav0  区间净值起点（归一锚）
 * @returns 归一后的基准序列（与 dates 等长）；起点早于日线窗口 / 数据不足 → null（整条不画，不伪装）
 */
export function alignBench(dates: string[], bars: KlineBar[] | undefined, nav0: number): number[] | null {
  return domainAlignBench({
    dates,
    bars: (bars ?? []).map((b) => ({ date: b.date, close: b.close ?? null })),
    nav0,
  });
}
