/**
 * 界面态 store（P5-T2）：toast（原型 chrome() 的铃铛/刷新提示同位）。
 * 只放跨组件的瞬时 UI 态；表单/抽屉局部态留在组件内。
 */

import { create } from "zustand";

interface UiState {
  /** 当前 toast 文案；null = 不显示 */
  toast: string | null;
  pushToast: (msg: string) => void;
  clearToast: () => void;
}

let timer: ReturnType<typeof setTimeout> | undefined;

export const useUi = create<UiState>((set) => ({
  toast: null,

  pushToast(msg) {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => set({ toast: null }), 3200);
    set({ toast: msg });
  },

  clearToast: () => {
    if (timer) clearTimeout(timer);
    set({ toast: null });
  },
}));
