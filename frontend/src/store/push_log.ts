/**
 * 推送留痕 store（P5-T2 补读端点，B13）：盘后链/渠道推送写入，前端只读。
 * ok=1 只表示请求已被受理（03 §12 留档语义），界面文案不得写成「已完成推送」。
 * index 时间线消费；wechat 屏推送日志（gate C4）落地时复用。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type PushLogRow = components["schemas"]["PushLogRow"];

interface PushLogState {
  /** 新在前（ts DESC） */
  rows: PushLogRow[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const usePushLog = create<PushLogState>((set) => ({
  rows: [],
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/push_logs", {
      params: { query: { limit: 50 } },
    });
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取推送日志失败") });
    }
  },
}));
