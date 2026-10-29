/**
 * 市场 store（P5-T2）：市场时钟/sidecar 状态（GET /api/market/status，永不失败，
 * 全挂时时钟退启发式）+ 顶栏行情条的指数实时行（GET /api/market/quote，
 * sidecar 支持指数代码，P4-T3 号段路由）。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type MarketStatus = components["schemas"]["MarketStatusResponse"];
export type QuoteRow = components["schemas"]["QuoteRow"];

/** 行情条指数（顺序与原型 strip 一致；symbol = sidecar 前缀格式） */
export const INDEX_SYMBOLS = [
  { symbol: "sh000001", name: "上证指数" },
  { symbol: "sz399001", name: "深证成指" },
  { symbol: "sz399006", name: "创业板指" },
  { symbol: "sh000300", name: "沪深300" },
  { symbol: "sh000832", name: "中证转债" },
] as const;

interface MarketState {
  /** null = 尚未加载成功（前端退本地启发式时钟） */
  status: MarketStatus | null;
  statusError: string | null;
  /** 指数实时行，key = symbol（sh000001）；缺行 = 该指数暂不可用，strip 显示 "—" */
  quotes: Record<string, QuoteRow>;
  quotesError: string | null;
  loadStatus: () => Promise<void>;
  loadIndexQuotes: () => Promise<void>;
}

export const useMarket = create<MarketState>((set) => ({
  status: null,
  statusError: null,
  quotes: {},
  quotesError: null,

  async loadStatus() {
    const { data, error } = await api.GET("/api/market/status");
    if (data) {
      set({ status: data, statusError: null });
    } else {
      // 后端整体不可达才会走到这（status 端点自身永不失败）；保留旧值
      set({ statusError: apiErrorMessage(error, "市场状态不可用") });
    }
  },

  async loadIndexQuotes() {
    const codes = INDEX_SYMBOLS.map((i) => i.symbol).join(",");
    const { data, error } = await api.GET("/api/market/quote", {
      params: { query: { codes } },
    });
    if (data) {
      const quotes: Record<string, QuoteRow> = {};
      for (const q of data.quotes) quotes[q.symbol] = q;
      set({ quotes, quotesError: data.degraded ? "行情源降级（兜底链顶上）" : null });
    } else {
      // sidecar 挂了 → 503：strip 保留旧值/显示 "—"，不打扰主界面
      set({ quotesError: apiErrorMessage(error, "指数行情不可用") });
    }
  },
}));
