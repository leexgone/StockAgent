/**
 * 日线 store（P5-T2 dashboard 屏起）：缓存 GET /api/market/kline/{symbol} 的
 * bar 序列，key = symbol + 窗口 + with_today（同参数命中缓存不重复打 sidecar；
 * sidecar 侧另有 300s TTL 缓存双保险）。消费方：dashboard 迷你走势线（spark）
 * 与沪深 300 基准序列，analysis 屏 K 线卡后续复用（ADR-0008 按域建 store）。
 *
 * 只缓存 payload，不做任何加工——归一/对齐/指标全部在消费侧或 domain 纯函数。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type KlineBar = components["schemas"]["KlineBar"];

export interface KlineQuery {
  /** 起始 YYYY-MM-DD（缺省 = sidecar 默认窗口 400 天） */
  start?: string;
  end?: string;
  /** true = 盘中用实时行情合成当日 bar（04 附录 B，非定稿） */
  withToday?: boolean;
}

/** 缓存键：同 symbol 不同窗口/合成语义分开存 */
export function klineKey(symbol: string, q: KlineQuery = {}): string {
  return `${symbol}|${q.start ?? ""}|${q.end ?? ""}${q.withToday ? "|t" : ""}`;
}

interface KlineState {
  /** key = klineKey(...)；缺键 = 未取到（加载中或失败，见 errors） */
  series: Record<string, KlineBar[]>;
  errors: Record<string, string>;
  /** 取一个 symbol（缓存命中即返回）；StrictMode 双挂载靠 in-flight 表去重 */
  load: (symbol: string, q?: KlineQuery) => Promise<void>;
  /** 顶栏手动刷新：跳过缓存重取 */
  reload: (symbol: string, q?: KlineQuery) => Promise<void>;
}

const inflight = new Map<string, Promise<void>>();

export const useKline = create<KlineState>((set) => {
  async function fetchInto(key: string, symbol: string, q: KlineQuery) {
    const { data, error } = await api.GET("/api/market/kline/{symbol}", {
      params: {
        path: { symbol },
        query: { start: q.start, end: q.end, with_today: q.withToday },
      },
    });
    inflight.delete(key);
    if (data) {
      set((s) => ({ series: { ...s.series, [key]: data.kline }, errors: { ...s.errors, [key]: "" } }));
    } else {
      // 失败如实记录；旧缓存保留（宁旧不假），消费侧对空序列已有防御
      set((s) => ({ errors: { ...s.errors, [key]: apiErrorMessage(error, "日线读取失败") } }));
    }
  }

  function start(key: string, symbol: string, q: KlineQuery): Promise<void> {
    const p = fetchInto(key, symbol, q);
    inflight.set(key, p);
    return p;
  }

  return {
    series: {},
    errors: {},

    async load(symbol, q = {}) {
      const key = klineKey(symbol, q);
      if (useKline.getState().series[key] || inflight.has(key)) return;
      await start(key, symbol, q);
    },

    async reload(symbol, q = {}) {
      const key = klineKey(symbol, q);
      if (inflight.has(key)) return inflight.get(key);
      await start(key, symbol, q);
    },
  };
});
