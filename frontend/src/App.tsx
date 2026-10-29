import { useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import Chrome from "./components/Chrome";
import { SCREEN_ROUTES } from "./routes";
import ChatView from "./screens/ChatView";
import HomeScreen from "./screens/HomeScreen";
import PlaceholderScreen from "./screens/PlaceholderScreen";

/**
 * 子木 SPA 根组件：外壳 Chrome + react-router 路由表（P5-T1，ADR-0008）。
 * 已移植屏：/ 工作台（P2-T4 index 静态复刻）、/chat Agent SSE 会话（P2-T3）；
 * 其余路由渲染占位屏，P5-T2 逐屏移植后在 routes.ts 翻 ported 并在此挂真实屏。
 */
export default function App() {
  // 市场时钟与问候语随真实时间每秒重绘（对应原型 bindChrome 的 paintClock 定时器）
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(t);
  }, []);

  return (
    <Chrome now={now} onRefresh={() => setNow(new Date())}>
      <Routes>
        <Route path="/" element={<HomeScreen now={now} />} />
        <Route path="/chat" element={<ChatView />} />
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
  );
}
