/**
 * 自选 store（P5-T2 dashboard 屏，B13 前端半边）：CRUD 走 /api/watchlist
 * （upsert 幂等，added_at 保首次值——后端语义），展示行情走 /api/market/quote
 * 单独合并拉取，**永不回写库**（03 §12：holdings 表只存持仓）。
 * 判定侧（自选按无持仓形态出预警）全在后端调度器，本 store 只读取与展示。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";
import { symbolOf } from "../lib/kind";
import type { QuoteRow } from "./market";

export type WatchlistRow = components["schemas"]["WatchlistRow"];
export type NewWatchlist = components["schemas"]["NewWatchlist"];

interface WatchlistState {
  /** added_at, code 升序（后端 ORDER BY 原样） */
  rows: WatchlistRow[];
  loaded: boolean;
  error: string | null;
  /** 展示行情，key = symbol；缺行/status 非 ok = 显示 "—"（B14 防御同款） */
  quotes: Record<string, QuoteRow>;
  quotesError: string | null;
  load: () => Promise<void>;
  /** 批量拉自选实时行（一次合并 quote 调用；rows 为空不打 sidecar） */
  loadQuotes: () => Promise<void>;
  /** 成功 true（rows 重拉，顺序以后端为准）；失败 false + error 文案 */
  add: (w: NewWatchlist) => Promise<boolean>;
  remove: (code: string) => Promise<boolean>;
}

export const useWatchlist = create<WatchlistState>((set, get) => ({
  rows: [],
  loaded: false,
  error: null,
  quotes: {},
  quotesError: null,

  async load() {
    const { data, error } = await api.GET("/api/watchlist");
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取自选失败") });
    }
  },

  async loadQuotes() {
    const syms = get()
      .rows.map((w) => symbolOf(w.mkt, w.code))
      .filter((s): s is string => s !== null);
    if (syms.length === 0) {
      set({ quotes: {}, quotesError: null });
      return;
    }
    const { data, error } = await api.GET("/api/market/quote", {
      params: { query: { codes: syms.join(",") } },
    });
    if (data) {
      const quotes: Record<string, QuoteRow> = {};
      for (const q of data.quotes) quotes[q.symbol] = q;
      set({ quotes, quotesError: data.degraded ? "行情源降级（兜底链顶上）" : null });
    } else {
      // sidecar 挂 → 保留旧行情（宁旧不假），错误只作提示
      set({ quotesError: apiErrorMessage(error, "自选行情不可用") });
    }
  },

  async add(w) {
    const { data, error } = await api.POST("/api/watchlist", { body: w });
    if (!data) {
      set({ error: apiErrorMessage(error, "添加自选失败") });
      return false;
    }
    await get().load();
    await get().loadQuotes();
    return true;
  },

  async remove(code) {
    const { data, error } = await api.DELETE("/api/watchlist/{code}", { params: { path: { code } } });
    if (!data) {
      set({ error: apiErrorMessage(error, "移除自选失败") });
      return false;
    }
    set((s) => ({ rows: s.rows.filter((r) => r.code !== code) }));
    return true;
  },
}));
