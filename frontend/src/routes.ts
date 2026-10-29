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
  { path: "/dashboard", crumb: "总览仪表盘", ported: false },
  { path: "/holdings", crumb: "持仓管理", ported: false },
  { path: "/analysis", crumb: "AI 分析建议", ported: false },
  { path: "/chat", crumb: "智能问答", ported: true },
  { path: "/diagnosis", crumb: "组合诊断", ported: false },
  { path: "/wechat", crumb: "微信助理接入", ported: false },
];

export function routeFor(path: string): ScreenRoute | undefined {
  return SCREEN_ROUTES.find((r) => r.path === path);
}
