import { useEffect, useState } from "react";
import Chrome from "./components/Chrome";
import HomeScreen from "./screens/HomeScreen";
import ChatView from "./screens/ChatView";
import type { View } from "./data/demo";

/**
 * 子木 SPA 根组件（P2-T4）：外壳 Chrome + 两个视图——
 * home = index 工作台静态复刻（对照 design/index.html），
 * chat = P2-T3 的 Agent SSE 会话（健康状态 + ChatPanel）。
 * 路由方案 P5-T1 再定，当前用视图状态切换。
 */
export default function App() {
  const [view, setView] = useState<View>("home");
  // 市场时钟与问候语随真实时间每秒重绘（对应原型 bindChrome 的 paintClock 定时器）
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(t);
  }, []);

  return (
    <Chrome
      active={view}
      crumb={view === "chat" ? <b>智能问答</b> : <b>工作台</b>}
      now={now}
      onNavigate={setView}
      onRefresh={() => setNow(new Date())}
    >
      {view === "home" ? (
        <HomeScreen now={now} onNavigate={setView} />
      ) : (
        <ChatView onNavigate={setView} />
      )}
    </Chrome>
  );
}
