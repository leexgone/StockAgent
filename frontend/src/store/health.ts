/**
 * 服务健康 store（P5-T2）：GET /api/health——agent_online 驱动 index 问答卡
 * 统计行与 chat 屏的 Agent 在线状态（已接线，ChatView 局部 fetch 已删）。
 */

import { create } from "zustand";
import { api } from "../api/client";
import type { components } from "../api/schema";

export type Health = components["schemas"]["Health"];

interface HealthState {
  health: Health | null;
  error: string | null;
  load: () => Promise<void>;
}

export const useHealth = create<HealthState>((set) => ({
  health: null,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/health");
    if (data) {
      set({ health: data, error: null });
    } else {
      set({ error: error ? String(error) : "后端不可达" });
    }
  },
}));
