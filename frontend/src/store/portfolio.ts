/**
 * 组合 store（P5-T2，ADR-0008）：后端是派生数字唯一真值——
 * view 直接缓存 `GET /api/holdings` / `POST /api/market/refresh` 返回的
 * PortfolioView（recalc 全景），前端不重算任何汇总。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type PortfolioView = components["schemas"]["PortfolioView"];

interface PortfolioState {
  /** null = 尚未成功加载过（首屏骨架/错误态判据） */
  view: PortfolioView | null;
  loading: boolean;
  /** 最近一次加载/刷新失败原因；null = 正常。刷新失败保留旧 view（宁旧不假） */
  error: string | null;
  /** 刷新链的非致命提示（sidecar 降级 / 部分标的无行情），成功刷新后重算 */
  note: string | null;
  load: () => Promise<void>;
  /** 顶栏刷新：POST /api/market/refresh（拉行情 → 回写 → recalc 重算）；false = 失败（旧值保留） */
  refresh: () => Promise<boolean>;
}

export const usePortfolio = create<PortfolioState>((set) => ({
  view: null,
  loading: true,
  error: null,
  note: null,

  async load() {
    set({ loading: true });
    const { data, error } = await api.GET("/api/holdings");
    if (data) {
      set({ view: data, loading: false, error: null });
    } else {
      set({ loading: false, error: apiErrorMessage(error, "读取持仓失败") });
    }
  },

  async refresh() {
    const { data, error } = await api.POST("/api/market/refresh");
    if (!data) {
      set({ error: apiErrorMessage(error, "刷新失败（行情源不可用，旧数字保留）") });
      return false;
    }
    const notes: string[] = [];
    if (data.degraded) notes.push("行情源降级（兜底链顶上）");
    if (data.skipped.length > 0) {
      notes.push(`${data.skipped.length} 只标的无行情回写（${data.skipped[0].code}：${data.skipped[0].reason}），保持旧值`);
    }
    set({ view: data.portfolio, error: null, note: notes.length ? notes.join("；") : null });
    return true;
  },
}));
