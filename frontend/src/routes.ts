/**
 * 路由表单一来源（P5-T1，ADR-0008）：路径与 design/ 原型文件名一致，
 * 深链可刷新可收藏（zimu-server SPA 回落 → index.html）；未知路径回落 "/"。
 * 未移植屏 = 占位屏（PlaceholderScreen）；P5-T2 每移植一屏，把对应 ported
 * 翻真并在 App 路由表挂真实屏组件。
 */
export interface ScreenRoute {
  /** URL 路径；"/" 为工作台根 */
  path: string;
  /** 顶栏面包屑文案 */
  crumb: string;
  /** false = 占位屏，T2 移植后翻真 */
  ported: boolean;
}

export const SCREEN_ROUTES: ScreenRoute[] = [
  { path: "/", crumb: "工作台", ported: true },
  { path: "/dashboard", crumb: "总览仪表盘", ported: true },
  { path: "/holdings", crumb: "持仓管理", ported: true },
  { path: "/analysis", crumb: "AI 分析建议", ported: true },
  { path: "/chat", crumb: "智能问答", ported: true },
  { path: "/diagnosis", crumb: "组合诊断", ported: true },
  { path: "/wechat", crumb: "微信助理接入", ported: false },
];

export function routeFor(path: string): ScreenRoute | undefined {
  return SCREEN_ROUTES.find((r) => r.path === path);
}

/* ---------- 导航项（侧栏 + 移动底栏；path 必须在上方路由表注册） ---------- */

export interface NavItem {
  path: string; // 路由路径（上方 SCREEN_ROUTES 单一来源）
  icon: string;
  label: string;
  short: string;
  kbd?: string;
}

export const NAV: NavItem[] = [
  { path: "/dashboard", icon: "grid", label: "总览仪表盘", short: "总览", kbd: "1" },
  { path: "/holdings", icon: "wallet", label: "持仓管理", short: "持仓", kbd: "2" },
  { path: "/analysis", icon: "spark", label: "AI 分析建议", short: "建议", kbd: "3" },
  { path: "/chat", icon: "chat", label: "智能问答", short: "问答", kbd: "4" },
  { path: "/diagnosis", icon: "shield", label: "组合诊断", short: "诊断", kbd: "5" },
];

export const WECHAT_NAV: NavItem = { path: "/wechat", icon: "wechat", label: "微信助理接入", short: "微信" };
