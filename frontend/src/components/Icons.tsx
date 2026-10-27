/**
 * 图标集 —— 移植自 design/assets/data.js 的 ICONS（路径逐一对应）。
 * 尺寸/线宽由 ui.css 的容器类控制（.ic svg / .nav-item svg / .icon-btn svg…），
 * 组件本身不设 width/height；特例（界面卡右箭头）在调用处给内联样式。
 */

export function IconSpark() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M3 17l5-6 4 3 5-7 4 4" />
    </svg>
  );
}

export function IconGrid() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </svg>
  );
}

export function IconWallet() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H18a2 2 0 0 1 2 2v1" />
      <path d="M3 7.5V18a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-3" />
      <path d="M21 12.5h-4a1.5 1.5 0 0 0 0 3h4z" />
    </svg>
  );
}

export function IconChat() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M21 12a8 8 0 0 1-8 8H8l-4 3v-6.5A8 8 0 0 1 12 4h1a8 8 0 0 1 8 8z" />
      <path d="M9 11h6M9 14h3" />
    </svg>
  );
}

export function IconShield() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z" />
      <path d="M9 12l2 2 4-4" />
    </svg>
  );
}

export function IconWechat() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M4 11a7 5.5 0 1 1 11.2 4.4L17 18l-3-1.3" />
      <path d="M9.5 9.5h.01M6.5 9.5h.01" />
      <path d="M14 13.5a5.5 4.5 0 0 1 6 4.4L19 20l2 1-.5-1.8" />
    </svg>
  );
}

export function IconBell() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M18 9a6 6 0 1 0-12 0c0 5-2 6-2 6h16s-2-1-2-6" />
      <path d="M10.5 20a2 2 0 0 0 3 0" />
    </svg>
  );
}

export function IconSearch() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <circle cx="11" cy="11" r="7" />
      <path d="M16.5 16.5L21 21" />
    </svg>
  );
}

export function IconFilter() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M4 6h16M7 12h10M10 18h4" />
    </svg>
  );
}

export function IconRefresh() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M21 12a9 9 0 1 1-3-6.7" />
      <path d="M21 4v5h-5" />
    </svg>
  );
}

/** 界面入口卡右侧箭头（原型为内联 svg，带显式线宽与尺寸） */
export function IconChevronRight() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" style={{ width: 15, height: 15 }}>
      <path d="M9 6l6 6-6 6" />
    </svg>
  );
}

/** 名称 → 组件（NAV/SCREENS 用字符串引用，与原型 ICONS 键一致） */
export const ICONS: Record<string, () => React.JSX.Element> = {
  grid: IconGrid,
  wallet: IconWallet,
  spark: IconSpark,
  chat: IconChat,
  shield: IconShield,
  wechat: IconWechat,
};
