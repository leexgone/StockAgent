import { useEffect, useRef, useState } from "react";

/**
 * 子木会话面板 —— P2-T3 SSE 链路验证（docs/06-roadmap.md）。
 *
 * 这是 spike 级 UI：验证「POST /api/chat → Agent → SSE 流式回推」在浏览器里
 * 端到端成立，不是 P5 的 chat 屏（正式形态届时按 design/chat.html 重做）。
 * 事件形状与后端 SseEvent 对齐（backend/crates/zimu-server/src/agent_bridge.rs）。
 */

type SseEvent =
  | { type: "connected"; profile: string; model: string; tools: string[] }
  | { type: "text_delta"; text: string }
  | { type: "tool_call"; tool_call_id: string; name: string; arguments: string }
  | { type: "tool_result"; tool_call_id: string; is_error: boolean; content: string }
  | { type: "skill_triggered"; name: string; description: string }
  | { type: "async_tool_completed"; task_id: string; tool_call_id: string; is_error: boolean }
  | { type: "confirm_request"; tool_call_id: string; name: string; arguments: string }
  | { type: "turn_complete" }
  | { type: "error"; message: string };

type Entry =
  | { kind: "user"; text: string }
  | { kind: "assistant"; text: string; done: boolean }
  | { kind: "tool"; id: string; name: string; args: string; result?: { ok: boolean; content: string } }
  | { kind: "notice"; level: "info" | "error"; text: string };

type Pending = { tool_call_id: string; name: string; arguments: string } | null;

type Conn = "connecting" | "open" | "error";

/** 工具参数原文（JSON 字符串）压成一行摘要，长参截断——完整值看 SSE 原文即可 */
function briefArgs(raw: string): string {
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const s = Object.entries(parsed)
      .map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`)
      .join("  ");
    return s.length > 160 ? `${s.slice(0, 160)}…` : s;
  } catch {
    return raw.length > 160 ? `${raw.slice(0, 160)}…` : raw;
  }
}

export default function ChatPanel({ agentOnline }: { agentOnline: boolean }) {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [conn, setConn] = useState<Conn>("connecting");
  const [pending, setPending] = useState<Pending>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!agentOnline) return;
    const es = new EventSource("/api/chat/stream");
    es.onopen = () => setConn("open");
    es.onerror = () => setConn("error"); // EventSource 会自动重连
    es.onmessage = (e) => {
      let ev: SseEvent;
      try {
        ev = JSON.parse(e.data as string) as SseEvent;
      } catch {
        return;
      }
      handleEvent(ev, setEntries, setPending, setBusy);
    };
    return () => es.close();
  }, [agentOnline]);

  // 新内容自动滚到底部
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [entries, pending]);

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    const text = input.trim();
    if (!text || busy) return;
    setEntries((prev) => [...prev, { kind: "user", text }]);
    setInput("");
    setBusy(true);
    const r = await fetch("/api/chat", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text }),
    });
    if (!r.ok) {
      setBusy(false);
      const body = await r.text();
      setEntries((prev) => [
        ...prev,
        { kind: "notice", level: "error", text: `发送失败：HTTP ${r.status} ${body}` },
      ]);
    }
  };

  const answerConfirm = async (approved: boolean) => {
    if (!pending) return;
    const { tool_call_id, name } = pending;
    setPending(null);
    await fetch("/api/chat/confirm", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tool_call_id, approved }),
    });
    setEntries((prev) => [
      ...prev,
      { kind: "notice", level: "info", text: approved ? `已批准工具 ${name}` : `已拒绝工具 ${name}` },
    ]);
  };

  const cancel = async () => {
    await fetch("/api/chat/cancel", { method: "POST" });
  };

  return (
    <section className="card chat" data-od-id="home-chat">
      <h2 className="chat-title">
        子木会话
        <span className="chat-sub">P2-T3 · SSE 链路验证</span>
        <span className="row chat-conn">
          <span
            className={`dot ${conn === "open" ? "dot-ok" : conn === "error" ? "dot-bad" : "dot-wait"}`}
            aria-hidden="true"
          />
          {conn === "open" ? "事件流已连接" : conn === "error" ? "事件流断开（重连中）" : "连接中…"}
        </span>
      </h2>

      {!agentOnline && (
        <p className="row muted">
          Agent 未启动——后端缺 robit 配置（~/.robit/config.toml 与 .env），/api/chat* 返回 503。
        </p>
      )}

      {agentOnline && (
        <>
          <div className="chat-log" ref={logRef} data-od-id="home-chat-log">
            {entries.length === 0 && (
              <p className="muted chat-empty">
                输入消息开始对话；试试「请调用 echo 工具回显：你好子木」。
              </p>
            )}
            {entries.map((entry, i) => (
              <EntryView key={i} entry={entry} />
            ))}
            {pending && (
              <div className="chat-confirm" data-od-id="home-chat-confirm">
                <p>
                  ⚠️ 工具 <code>{pending.name}</code> 请求确认：{briefArgs(pending.arguments)}
                </p>
                <p className="row">
                  <button className="btn btn-accent" onClick={() => void answerConfirm(true)}>
                    批准
                  </button>
                  <button className="btn" onClick={() => void answerConfirm(false)}>
                    拒绝
                  </button>
                </p>
              </div>
            )}
          </div>

          <form className="chat-input-row" onSubmit={send} data-od-id="home-chat-input">
            <input
              type="text"
              value={input}
              placeholder={busy ? "等待本轮回复…" : "对子木说点什么…"}
              onChange={(e) => setInput(e.target.value)}
              disabled={busy}
              autoComplete="off"
            />
            <button type="submit" className="btn btn-accent" disabled={busy || !input.trim()}>
              发送
            </button>
            <button type="button" className="btn" onClick={() => void cancel()} disabled={!busy}>
              取消
            </button>
          </form>
        </>
      )}
    </section>
  );
}

function EntryView({ entry }: { entry: Entry }) {
  switch (entry.kind) {
    case "user":
      return <p className="chat-user">我：{entry.text}</p>;
    case "assistant":
      return (
        <div className={`chat-assistant${entry.done ? "" : " chat-streaming"}`}>
          子木：{entry.text}
        </div>
      );
    case "tool":
      return (
        <div className="chat-tool">
          <p className="chat-tool-name">🔧 {entry.name}</p>
          <p className="chat-tool-args">{briefArgs(entry.args)}</p>
          {entry.result && (
            <p className={entry.result.ok ? "chat-tool-ok" : "chat-tool-err"}>
              {entry.result.ok ? "✓" : "✗"} {entry.result.content}
            </p>
          )}
        </div>
      );
    case "notice":
      return (
        <p className={entry.level === "error" ? "chat-notice chat-notice-err" : "chat-notice"}>
          {entry.text}
        </p>
      );
  }
}

/** SSE 事件 → 面板状态。纯函数式更新，StrictMode 双挂载下也安全。 */
function handleEvent(
  ev: SseEvent,
  setEntries: React.Dispatch<React.SetStateAction<Entry[]>>,
  setPending: React.Dispatch<React.SetStateAction<Pending>>,
  setBusy: React.Dispatch<React.SetStateAction<boolean>>,
) {
  switch (ev.type) {
    case "connected":
      setEntries((prev) => [
        ...prev,
        {
          kind: "notice",
          level: "info",
          text: `Agent 已连接：${ev.model}（profile ${ev.profile}，工具 ${ev.tools.join("、")}）`,
        },
      ]);
      break;
    case "text_delta":
      setEntries((prev) => {
        const last = prev[prev.length - 1];
        if (last && last.kind === "assistant" && !last.done) {
          return [...prev.slice(0, -1), { ...last, text: last.text + ev.text }];
        }
        return [...prev, { kind: "assistant", text: ev.text, done: false }];
      });
      break;
    case "tool_call":
      setEntries((prev) => [
        ...prev,
        { kind: "tool", id: ev.tool_call_id, name: ev.name, args: ev.arguments },
      ]);
      break;
    case "tool_result":
      setEntries((prev) => {
        // 从后往前找同 id 的工具条目（中间可能隔着确认/通知）。
        // 手写倒序循环：findLastIndex 需要 lib es2023，不动 tsconfig 基线。
        let idx = -1;
        for (let i = prev.length - 1; i >= 0; i--) {
          const e = prev[i];
          if (e.kind === "tool" && e.id === ev.tool_call_id) {
            idx = i;
            break;
          }
        }
        if (idx < 0) return prev;
        const next = [...prev];
        const target = next[idx] as Extract<Entry, { kind: "tool" }>;
        next[idx] = { ...target, result: { ok: !ev.is_error, content: ev.content } };
        return next;
      });
      break;
    case "confirm_request":
      setPending({
        tool_call_id: ev.tool_call_id,
        name: ev.name,
        arguments: ev.arguments,
      });
      break;
    case "skill_triggered":
      setEntries((prev) => [
        ...prev,
        { kind: "notice", level: "info", text: `[Skill] ${ev.name} — ${ev.description}` },
      ]);
      break;
    case "async_tool_completed":
      setEntries((prev) => [
        ...prev,
        {
          kind: "notice",
          level: ev.is_error ? "error" : "info",
          text: `异步任务 ${ev.task_id} 完成（is_error=${String(ev.is_error)}）`,
        },
      ]);
      break;
    case "turn_complete":
      setEntries((prev) => {
        // 只收口末尾未完成的流式段
        const last = prev[prev.length - 1];
        if (last && last.kind === "assistant" && !last.done) {
          return [...prev.slice(0, -1), { ...last, done: true }];
        }
        return prev;
      });
      setPending(null);
      setBusy(false);
      break;
    case "error":
      setEntries((prev) => [...prev, { kind: "notice", level: "error", text: ev.message }]);
      setBusy(false);
      break;
  }
}
