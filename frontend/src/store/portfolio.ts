/**
 * 组合 store（P5-T2，ADR-0008）：后端是派生数字唯一真值——
 * view 直接缓存 `GET /api/holdings` / `POST /api/market/refresh` 返回的
 * PortfolioView（recalc 全景），前端不重算任何汇总。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type PortfolioView = components["schemas"]["PortfolioView"];
export type TradeRequest = components["schemas"]["TradeRequest"];
export type RecordReceipt = components["schemas"]["RecordReceipt"];
/** 录入拒绝体：message + 特定拒绝的附加数值（涨跌停 limit_up/limit_down/decimals、现金不足 cash_after） */
export type TradeRejection = components["schemas"]["ApiError"];

/** recordTrade 结果：成功带后端收据（数字权威），失败带 ApiError（含就地高亮字段） */
export type TradeResult =
  | { ok: true; receipt: RecordReceipt }
  | { ok: false; error: TradeRejection };

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
  /** 记一笔加减仓：POST /api/holdings/trade；成功后整表重拉（后端为唯一真值，不做乐观拼接） */
  recordTrade: (body: TradeRequest) => Promise<TradeResult>;
  /** 设置可用现金：POST /api/account（onboarding 注资入口，负数后端拒 400）；成功后重拉 */
  setCash: (cash: number) => Promise<boolean>;
}

export const usePortfolio = create<PortfolioState>((set, get) => ({
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

  async recordTrade(body) {
    const { data, error } = await api.POST("/api/holdings/trade", { body });
    if (data) {
      // 提交后以后端返回覆盖（ADR-0008）：整表重拉拿权威 recalc 全景，收据留给调用方 toast
      await get().load();
      return { ok: true, receipt: data };
    }
    // openapi-fetch 对 4xx/5xx 把解析后的 ApiError 放进 error（含 limit_up/cash_after 等附加字段）
    return { ok: false, error: (error ?? { message: "录入失败（网络或后端不可用）" }) as TradeRejection };
  },

  async setCash(cash) {
    const { data, error } = await api.POST("/api/account", { body: { cash } });
    if (data) {
      await get().load();
      return true;
    }
    set({ error: apiErrorMessage(error, "更新现金失败") });
    return false;
  },
}));
