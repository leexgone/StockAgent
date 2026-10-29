/**
 * 净值快照 store（P5-T2）：每交易日 15:05 由盘后链写入（03 §12，date 主键幂等），
 * 数字 = recalc agg 原值。rows 为 date 升序（净值曲线/风险指标输入形态）。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type EquitySnapshotRow = components["schemas"]["EquitySnapshotRow"];

interface EquityState {
  rows: EquitySnapshotRow[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useEquity = create<EquityState>((set) => ({
  rows: [],
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/equity_snapshots", {
      params: { query: { limit: 250 } },
    });
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取净值快照失败") });
    }
  },
}));
