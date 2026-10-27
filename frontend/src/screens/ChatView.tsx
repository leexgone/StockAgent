import { useEffect, useState } from "react";
import ChatPanel from "../components/ChatPanel";
import type { View } from "../data/demo";

/**
 * 智能问答视图 —— P2-T3 的 SSE 链路验证入口（原最简框架首页迁到这里）。
 * spike 级形态：运行状态卡 + ChatPanel；正式 chat 屏 P5 按 design/chat.html 重做，
 * 届时示例问答（demo.ts qa）与真实 Agent 会话并存。
 */

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

export default function ChatView({ onNavigate }: { onNavigate: (v: View) => void }) {
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
    <main className="page" id="main" data-od-id="chat-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">P2-T3 链路验证 · SPIKE 级</div>
          <h1 className="page-title">智能问答</h1>
          <p className="page-desc">
            经后端 SSE 桥与子木 Agent 实时对话（流式输出、echo 工具调用、工具确认）。
            正式问答屏在 P5 按原型 chat.html 重做。
          </p>
        </div>
        <div className="page-actions">
          <button className="btn" onClick={() => onNavigate("home")}>返回工作台</button>
        </div>
      </div>

      <section className="card" data-od-id="chat-status" style={{ marginBottom: 16 }}>
        <div className="card-head">
          <div>
            <div className="card-title">运行状态</div>
            <div className="card-sub">zimu-server · /api/health</div>
          </div>
        </div>
        <div className="card-body">
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
        </div>
      </section>

      {state.kind === "online" && <ChatPanel agentOnline={state.health.agent_online} />}

      <div className="footnote" id="chat-disclaimer" data-od-id="chat-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          本程序只给建议、不代下单；所有分析与建议仅供参考，<b>不构成收益承诺</b>。投资有风险，决策需独立判断。
        </span>
      </div>
    </main>
  );
}
