import { useEffect, useState } from "react";
import ChatPanel from "./components/ChatPanel";

/** 后端 /api/health 返回形状（backend/crates/zimu-server/src/main.rs） */
interface Health {
  ok: boolean;
  service: string;
  version: string;
  static_dir: string;
  static_ready: boolean;
  agent_online: boolean;
}

type BackendState =
  | { kind: "loading" }
  | { kind: "online"; health: Health }
  | { kind: "offline"; reason: string };

export default function App() {
  const [state, setState] = useState<BackendState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;
    fetch("/api/health")
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json() as Promise<Health>;
      })
      .then((h) => {
        if (!cancelled) setState({ kind: "online", health: h });
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setState({ kind: "offline", reason: e instanceof Error ? e.message : String(e) });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <main className="home" data-od-id="home">
      <section className="hero" data-od-id="home-hero">
        <div className="brand-mark" aria-hidden="true">子</div>
        <h1>子木 · 股票智能助理</h1>
        <p className="muted">
          最简运行框架已就绪——本页由 React 渲染；业务功能按路线图（docs/06）逐步接入。
        </p>
      </section>

      <section className="card" data-od-id="home-status">
        <h2>运行状态</h2>
        {state.kind === "loading" && (
          <p className="row">
            <span className="dot dot-wait" aria-hidden="true" />
            正在连接后端…
          </p>
        )}
        {state.kind === "offline" && (
          <p className="row">
            <span className="dot dot-bad" aria-hidden="true" />
            后端未连接（{state.reason}）——开发模式下请先启动：
            <code>cd backend &amp;&amp; cargo run</code>
          </p>
        )}
        {state.kind === "online" && (
          <>
            <p className="row">
              <span className="dot dot-ok" aria-hidden="true" />
              {state.health.service} v{state.health.version} 在线
            </p>
            <p className="row muted">
              静态目录 <code>{state.health.static_dir}</code>
              {state.health.static_ready
                ? "（index.html 就绪）"
                : "（未构建——在 frontend/ 执行 npm run build 后由后端托管）"}
            </p>
            <p className="row">
              <span className={`dot ${state.health.agent_online ? "dot-ok" : "dot-bad"}`} aria-hidden="true" />
              Agent {state.health.agent_online ? "在线（qwen 会话可用）" : "离线（缺 robit 配置，详见服务日志）"}
            </p>
          </>
        )}
      </section>

      {state.kind === "online" && <ChatPanel agentOnline={state.health.agent_online} />}

      <footer id="home-disclaimer" data-od-id="home-disclaimer" className="disclaimer">
        本程序只给建议、不代下单；所有分析与建议仅供参考，不构成收益承诺。投资有风险，决策需独立判断。
      </footer>
    </main>
  );
}
