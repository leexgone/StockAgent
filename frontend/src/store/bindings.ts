/**
 * 通道绑定 store（ADR-0017 F3，wechat 屏绑定管理卡）：通道无关白名单管理——
 * QQ 与微信共用 /api/channel_bindings（拍板⑤：绑定关系界面管理、落 zimu.db；
 * 连接参数与启用开关只在服务器配置文件，本 store 不碰）。
 * 入站唯一闸门 = 绑定白名单（fail-closed）：未绑定会话来消息被 BoundOnly
 * 静默丢弃（服务端日志留 chat_id 供绑定）；解绑置 active=0 留痕不删行。
 * mutation 成功后重拉（顺序以后端 bound_at 升序为准），范式照 watchlist store。
 */

import { create } from "zustand";
import { api, apiErrorMessage } from "../api/client";
import type { components } from "../api/schema";

export type ChannelBindingRow = components["schemas"]["ChannelBindingRow"];
export type BindingRequest = components["schemas"]["BindingRequest"];

interface BindingsState {
  /** 活跃绑定（active=1），bound_at 升序（后端 ORDER BY 原样），全通道 */
  rows: ChannelBindingRow[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
  /** 幂等 upsert；成功 true（rows 重拉）；失败 false + error 文案 */
  bind: (req: BindingRequest) => Promise<boolean>;
  /** 解绑（置 active=0 留痕）；404 = 无该活跃绑定，经 apiErrorMessage 如实提示 */
  unbind: (channel: string, chatId: string) => Promise<boolean>;
}

export const useBindings = create<BindingsState>((set, get) => ({
  rows: [],
  loaded: false,
  error: null,

  async load() {
    const { data, error } = await api.GET("/api/channel_bindings");
    if (data) {
      set({ rows: data, loaded: true, error: null });
    } else {
      set({ loaded: true, error: apiErrorMessage(error, "读取绑定失败") });
    }
  },

  async bind(req) {
    const { data, error } = await api.POST("/api/channel_bindings", { body: req });
    if (!data) {
      set({ error: apiErrorMessage(error, "绑定失败") });
      return false;
    }
    await get().load();
    return true;
  },

  async unbind(channel, chatId) {
    const { data, error } = await api.DELETE("/api/channel_bindings/{channel}/{chat_id}", {
      params: { path: { channel, chat_id: chatId } },
    });
    if (!data) {
      set({ error: apiErrorMessage(error, "解绑失败") });
      return false;
    }
    await get().load();
    return true;
  },
}));