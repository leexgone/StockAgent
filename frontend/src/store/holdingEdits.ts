/**
 * 持仓维护留痕 store（ADR-0009「维护记录」卡）：GET /api/holding_edits（最新在前）。
 * 直接维护（新增/编辑/删除）成功后整表重拉——后端为唯一真值（ADR-0008），
 * 前端不做乐观拼接；交易录入不产生本表记录（走 trades 流水）。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type HoldingEditRow = components["schemas"]["HoldingEditRow"];
export type HoldingSnapshot = components["schemas"]["HoldingSnapshot"];

interface HoldingEditsState {
  rows: HoldingEditRow[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useHoldingEdits = create<HoldingEditsState>((set) => ({
  rows: [],
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/holding_edits", {
      params: { query: { limit: 50 } },
    });
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取维护留痕失败") });
    }
  },
}));
