import { useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import Chrome from "./components/Chrome";
import ToastHost from "./components/Toast";
import { SCREEN_ROUTES } from "./routes";
import AnalysisScreen from "./screens/AnalysisScreen";
import ChatView from "./screens/ChatView";
import DashboardScreen from "./screens/DashboardScreen";
import DiagnosisScreen from "./screens/DiagnosisScreen";
import HoldingsScreen from "./screens/HoldingsScreen";
import HomeScreen from "./screens/HomeScreen";
import PlaceholderScreen from "./screens/PlaceholderScreen";
import { useAdvice } from "./store/advice";
import { useAlerts } from "./store/alerts";
import { useEquity } from "./store/equity";
import { useHealth } from "./store/health";
import { useMarket } from "./store/market";
import { usePortfolio } from "./store/portfolio";
import { usePushLog } from "./store/push_log";
import { useUi } from "./store/ui";

/**
 * 子木 SPA 根组件：外壳 Chrome + react-router 路由表（P5-T1，ADR-0008）。
 * 已移植屏：/ 工作台、/dashboard 总览仪表盘、/holdings 持仓管理（含加减仓抽屉接
 * API）、/analysis AI 分析建议、/diagnosis 组合诊断、/chat Agent SSE 会话（P5-T2
 * 真实数据）；余 /wechat 渲染占位屏（gate C4 待拍板），逐屏移植后在 routes.ts 翻
 * ported 并在此挂真实屏。
 *
 * 数据接线（P5-T2）：启动时全量拉一次；此后 60s 轮询——市场时钟恒拉，
 * 盘中（clock.live，03 §9 生产口径）才拉组合/指数/预警（与后端调度器同节奏，
 * 非交易时段不打 sidecar）。顶栏刷新 = POST /api/market/refresh（拉行情 →
 * 回写 → recalc 重算）+ 指数/时钟/预警重拉，失败保留旧数字并 toast 说明。
 */
export default function App() {
  // 市场时钟倒计时与问候语随真实时间每秒走针
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    const boot = async () => {
      await Promise.all([
        useHealth.getState().load(),
        useMarket.getState().loadStatus(),
        usePortfolio.getState().load(),
        useAlerts.getState().load(),
        useEquity.getState().load(),
        useAdvice.getState().load(),
        usePushLog.getState().load(),
        useMarket.getState().loadIndexQuotes(),
      ]);
    };
    void boot();
    const t = window.setInterval(() => {
      const live = useMarket.getState().status?.clock.live ?? false;
      void useMarket.getState().loadStatus();
      if (live) {
        void usePortfolio.getState().load();
        void useMarket.getState().loadIndexQuotes();
        void useAlerts.getState().load();
      }
    }, 60_000);
    return () => window.clearInterval(t);
  }, []);

  const onRefresh = async () => {
    const ok = await usePortfolio.getState().refresh();
    await Promise.all([
      useMarket.getState().loadIndexQuotes(),
      useMarket.getState().loadStatus(),
      useAlerts.getState().load(),
    ]);
    // 屏幕级数据（dashboard 的 kline/自选行情）以 refreshTick 为依赖强制重取
    useUi.getState().bumpRefresh();
    const note = usePortfolio.getState().note;
    useUi.getState().pushToast(
      ok
        ? note
          ? `行情已刷新，组合已重算 · ${note}`
          : "行情已刷新，组合已重算"
        : "刷新失败：行情源不可用（旧数字保留，未动库）",
    );
  };

  return (
    <>
      <Chrome now={now} onRefresh={() => void onRefresh()}>
        <Routes>
          <Route path="/" element={<HomeScreen now={now} />} />
          <Route path="/dashboard" element={<DashboardScreen now={now} />} />
          <Route path="/holdings" element={<HoldingsScreen />} />
          <Route path="/analysis" element={<AnalysisScreen />} />
          <Route path="/diagnosis" element={<DiagnosisScreen />} />
          <Route path="/chat" element={<ChatView now={now} />} />
          {SCREEN_ROUTES.filter((r) => !r.ported).map((r) => (
            <Route
              key={r.path}
              path={r.path}
              element={<PlaceholderScreen path={r.path} crumb={r.crumb} />}
            />
          ))}
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Chrome>
      <ToastHost />
    </>
  );
}
