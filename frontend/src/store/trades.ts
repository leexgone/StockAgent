/**
 * 成交流水 store（P5-T2 holdings 屏）：GET /api/trades（最新在前，费用三分项留档）。
 * 录入成功后整表重拉——流水以后端为唯一真值（ADR-0008），前端不做乐观拼接。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type TradeRow = components["schemas"]["TradeRow"];

interface TradesState {
  rows: TradeRow[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useTrades = create<TradesState>((set) => ({
  rows: [],
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/trades", { params: { query: { limit: 200 } } });
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取成交流水失败") });
    }
  },
}));
