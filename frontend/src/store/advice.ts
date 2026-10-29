/**
 * 建议留档 store（P5-T2）：agent 工具链写入（ADR-0007——数字归规则、文字归模型），
 * 前端只读；reason 空串 = 数字已留档、模型文案未回填，界面如实展示占位。
 * analysis 屏移植时按 code 过滤消费同一 store。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type AdviceRow = components["schemas"]["AdviceRow"];

interface AdviceState {
  /** 全量留档，新在前（generated_at DESC） */
  rows: AdviceRow[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useAdvice = create<AdviceState>((set) => ({
  rows: [],
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/advice", {
      params: { query: { limit: 200 } },
    });
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取建议留档失败") });
    }
  },
}));

/** 已覆盖标的数（去重 code）——index 建议卡统计行 */
export const coveredCount = (rows: AdviceRow[]): number =>
  new Set(rows.map((r) => r.code)).size;
