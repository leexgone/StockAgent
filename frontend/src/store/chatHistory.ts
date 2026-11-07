/**
 * Web 会话档案 store（T4/ADR-0011）：服务端 chat_messages 是唯一真值。
 *
 * 语义（docs/02 chat 展示口径）：
 * - `rows=null` = **尚未取到**（首帧/失败）——ChatPanel 回落 localStorage 缓存
 *   （缓存只在服务端不可达时作只读展示，不再当会话真值）；
 * - `rows=[]` = 服务端确认档案为空（新会话/已重置）——照常水合为空线程，
 *   缓存被真值覆盖；
 * - `reset()`：POST /api/chat/reset——**服务端清档案 + 通知 agent 清内存历史**；
 *   失败返回 false，调用方不得先清本地缓存（本地不该比真值先消失）。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { ChatMessageRow } from "../lib/chatHydrate";

interface ChatHistoryState {
  /** null = 未就绪/不可达（走缓存降级）；[] = 服务端确认空档案 */
  rows: ChatMessageRow[] | null;
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
  reset: () => Promise<boolean>;
}

export const useChatHistory = create<ChatHistoryState>((set) => ({
  rows: null,
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/chat/history", {
      params: { query: { limit: 100 } },
    });
    if (data) {
      set({ rows: data.messages, loaded: true, error: null });
    } else {
      // 取不到 = 维持 null：ChatPanel 用本地缓存只读展示，不假装档案是空的
      set({ loaded: true, error: apiErrorMessage(error, "读取会话档案失败（回退本机缓存展示）") });
    }
  },

  async reset() {
    const { data, error } = await api.POST("/api/chat/reset", {});
    if (data) {
      set({ rows: [], error: null });
      return true;
    }
    set({ error: apiErrorMessage(error, "清空服务端档案失败") });
    return false;
  },
}));
