/**
 * 预警 store（P5-T2）：内容由调度引擎判定落库（03 §12），前端只读取与归档，
 * 不做任何判定、不改写 text/level/type。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type AlertRow = components["schemas"]["AlertRow"];

interface AlertsState {
  /** 全量（含已归档），新在前；待处理 = rows.filter(r => !r.done) */
  rows: AlertRow[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
  /** 归档一条（幂等；成功后就地更新行，不整表重拉） */
  markDone: (id: number) => Promise<void>;
}

export const useAlerts = create<AlertsState>((set) => ({
  rows: [],
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/alerts", { params: { query: { limit: 200 } } });
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取预警失败") });
    }
  },

  async markDone(id) {
    const { data, error } = await api.POST("/api/alerts/{id}/done", { params: { path: { id } } });
    if (data) {
      set((s) => ({ rows: s.rows.map((r) => (r.id === id ? data : r)) }));
    } else {
      set({ error: apiErrorMessage(error, "归档失败") });
    }
  },
}));

/** 待处理预警（选择器helper：组件里 `useAlerts(pending)` 用） */
export const pending = (s: AlertsState): AlertRow[] => s.rows.filter((r) => !r.done);
