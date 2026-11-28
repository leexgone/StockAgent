/**
 * TradeDrawer 候选池构造（持仓 + 自选未持有）——原 holdings 屏内联 memo 上收共享
 * （2026-11-28：analysis 屏「写入持仓试算」改为本屏直开抽屉，两屏共用一份组装口径）。
 * 字段口径不变：持仓带 per 原值（mv/weight），自选只带可用 quote（status=ok 且 price>0，
 * B14 空行守卫）、held=false 买入即新建；B7 市场码显式不猜。
 */
import type { Candidate } from "../components/TradeDrawer";
import { symbolOf } from "./kind";
import type { QuoteRow } from "../store/market";
import type { PortfolioView } from "../store/portfolio";
import type { WatchlistRow } from "../store/watchlist";

export function buildTradeCandidates(
  view: PortfolioView | null,
  watchRows: WatchlistRow[],
  watchQuotes: Record<string, QuoteRow>,
): Candidate[] {
  if (!view) return [];
  const held: Candidate[] = view.holdings.map((h, i) => {
    const p = view.per[i];
    return {
      code: h.code, name: h.name, kind: h.kind, mkt: h.mkt, sector: h.sector,
      shares: h.shares, cost: h.cost, mv: p?.mv ?? 0, weight: p?.weight ?? 0,
      limit_pct: h.limit_pct ?? null, price: h.price ?? null, prev: h.prev ?? null, held: true,
    };
  });
  const heldCodes = new Set(view.holdings.map((h) => h.code));
  const watch: Candidate[] = watchRows
    .filter((w) => !heldCodes.has(w.code))
    .map((w) => {
      const sym = symbolOf(w.mkt, w.code);
      const q = sym ? watchQuotes[sym] : undefined;
      const usable = q && q.status === "ok" && (q.price ?? 0) > 0;
      return {
        code: w.code, name: w.name, kind: w.kind, mkt: w.mkt, sector: w.sector,
        shares: 0, cost: 0, mv: 0, weight: 0,
        limit_pct: w.limit_pct ?? null,
        price: usable ? (q.price ?? null) : null,
        prev: usable ? (q.prev ?? null) : null,
        held: false,
      };
    });
  return [...held, ...watch];
}
