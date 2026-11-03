import { useCallback, useEffect, useRef, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

/**
 * 子木会话面板 —— 真实 Agent SSE 线程（P2-T3 验证链路；P5-T2 起即 chat 屏的
 * 会话线程本体，布局与右栏上下文/边界/会话三卡见 screens/ChatView.tsx）。
 * 流式回答 + 工具调用/结果 + 确认请求 + skill 触发如实展示——「依据来源」就是
 * Agent 实调的行情/持仓工具，不伪造引用。会话可持久化（persistKey，docs/02）。
 * 事件形状与后端 SseEvent 对齐（backend/crates/zimu-server/src/agent_bridge.rs）。
 * 助手消息按 GFM Markdown 渲染（ADR-0010）；用户/通知/工具条目保持纯文本。
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

/* ---------- 助手消息 Markdown 渲染（ADR-0010） ----------
   react-markdown 产出 React 元素，原始 HTML 默认不渲染——LLM 输出混入
   <script>/on*= 只是被丢弃，XSS 免疫是结构性的，不靠净化器兜底。
   守卫三条：链接新标签页 + noopener（不安全协议由内置 urlTransform 默认剥除）、
   图片一律不渲染（回答不该出现图，出现即按注入面丢弃）、表格外套横向滚动容器。 */
const MD_PLUGINS = [remarkGfm];
const MD_COMPONENTS: Components = {
  a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" />,
  img: () => null,
  table: ({ node: _node, ...props }) => (
    <div className="chat-md-scroll">
      <table {...props} />
    </div>
  ),
};

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

export interface ChatPanelProps {
  agentOnline: boolean;
  /**
   * 预填输入框（suggest chips / ?q= 深链）；nonce 变化即写入并聚焦，默认不自动发送
   * （真实 LLM，交用户确认）。`send: true` = 意图明确的深链（analysis 空态
   * 「去问答触发 /建议」，chat?q=…&send=1）：agent 上线且空闲时自动发送该文本一次。
   */
  prefill?: { text: string; nonce: number; send?: boolean };
  /** 递增即清空会话（含 localStorage）；0/undefined = 不触发 */
  clearNonce?: number;
  /** 设置则把会话持久化到 localStorage[key]（上限 30 条，docs/02 chat 约定） */
  persistKey?: string;
  /**
   * SSE `connected` 事件上抛（model/profile/工具面）——连接信息不进对话框，
   * 由宿主（ChatView）挂在「Agent 在线」徽章上作悬停 hint（docs/02 展示口径）。
   * 重连会再次触发，宿主覆盖即可。
   */
  onConnected?: (info: { model: string; profile: string; tools: string[] }) => void;
}

/** 连接通知文本前缀（历史存档清除的判定——新逻辑不再往对话框写此类条目） */
const CONNECT_NOTICE = "Agent 已连接";

/** 从 localStorage 读回历史（坏数据 → 空）；流式未完成段归一为 done */
function loadPersisted(key: string): Entry[] {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return [];
    const arr = JSON.parse(raw) as Entry[];
    return arr
      .map((e) => (e.kind === "assistant" ? { ...e, done: true } : e))
      // 历史版本写进对话框的「Agent 已连接」通知读回即清除（2026-11-03 拍板：
      // 连接信息改走「Agent 在线」徽章的悬停 hint，不属于对话内容）——旧存档自愈。
      .filter((e) => !(e.kind === "notice" && e.text.startsWith(CONNECT_NOTICE)));
  } catch {
    return [];
  }
}

export default function ChatPanel({ agentOnline, prefill, clearNonce, persistKey, onConnected }: ChatPanelProps) {
  const [entries, setEntries] = useState<Entry[]>(() => (persistKey ? loadPersisted(persistKey) : []));
  const [conn, setConn] = useState<Conn>("connecting");
  const [pending, setPending] = useState<Pending>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  /** 已自动发送过的 prefill nonce（send=1 深链只发一次，busy/离线恢复后不重放） */
  const autoSent = useRef(0);
  // onConnected 走 ref：SSE effect 只依赖 agentOnline，宿主换回调不重连事件流
  const onConnectedRef = useRef(onConnected);
  useEffect(() => {
    onConnectedRef.current = onConnected;
  }, [onConnected]);

  // 持久化（上限 30 条）
  useEffect(() => {
    if (!persistKey) return;
    try {
      localStorage.setItem(persistKey, JSON.stringify(entries.slice(-30)));
    } catch {
      /* 隐私模式/配额满：静默，不影响会话 */
    }
  }, [entries, persistKey]);

  // 预填（chips / 深链）：写输入框并聚焦，交由用户确认后发送
  useEffect(() => {
    if (prefill && prefill.nonce > 0) {
      setInput(prefill.text);
      inputRef.current?.focus();
    }
  }, [prefill]);

  // 清空会话
  useEffect(() => {
    if (clearNonce && clearNonce > 0) {
      setEntries([]);
      setPending(null);
      if (persistKey) {
        try {
          localStorage.removeItem(persistKey);
        } catch {
          /* 忽略 */
        }
      }
    }
  }, [clearNonce, persistKey]);

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
      // 连接信息上抛宿主（徽章悬停 hint），不进对话流——见 handleEvent connected 分支
      if (ev.type === "connected") {
        onConnectedRef.current?.({ model: ev.model, profile: ev.profile, tools: ev.tools });
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

  /** 发送核心：手动提交与 send=1 深链自动发送共用（入口不同，链路与留痕完全一致） */
  const submit = useCallback(
    async (raw: string) => {
      const text = raw.trim();
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
    },
    [busy],
  );

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    await submit(input);
  };

  // send=1 深链（如「去问答触发 /建议」）：等 agent 上线且空闲时自动发送一次；
  // agent 离线时停留在预填态（输入框已有文本，上线后本 effect 因 agentOnline 变化补发）
  useEffect(() => {
    if (!prefill?.send || prefill.nonce <= 0) return;
    if (autoSent.current === prefill.nonce) return;
    if (!agentOnline || busy) return;
    autoSent.current = prefill.nonce;
    void submit(prefill.text);
  }, [prefill, agentOnline, busy, submit]);

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
        <span className="chat-sub">实时 Agent · 流式回答 · 工具调用可见</span>
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
                  <button className="btn btn-primary" onClick={() => void answerConfirm(true)}>
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
              ref={inputRef}
              value={input}
              placeholder={busy ? "等待本轮回复…" : "对子木说点什么…"}
              onChange={(e) => setInput(e.target.value)}
              disabled={busy}
              autoComplete="off"
            />
            <button type="submit" className="btn btn-primary" disabled={busy || !input.trim()}>
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
        <div className="chat-assistant">
          <span className="chat-who">子木：</span>
          {/* 累积整段重渲（气泡文本尺寸下每 delta 开销可忽略）；流式无闪烁光标——
              entry.done 仍归状态机使用（空白帧守卫/持久化归一），渲染层不再消费 */}
          <div className="chat-md">
            <Markdown remarkPlugins={MD_PLUGINS} components={MD_COMPONENTS}>
              {entry.text}
            </Markdown>
          </div>
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
      // 2026-11-03 拍板：连接信息（model/profile/工具面）不是对话内容，不写 entries——
      // 由 onmessage 上抛宿主作「Agent 在线」徽章的悬停 hint（docs/02 chat 展示口径）。
      break;
    case "text_delta":
      setEntries((prev) => {
        const last = prev[prev.length - 1];
        if (last && last.kind === "assistant" && !last.done) {
          return [...prev.slice(0, -1), { ...last, text: last.text + ev.text }];
        }
        // 空白首帧（模型流的角色帧 / 思考模式空内容帧）不建占位气泡——
        // 只有真正可见的文字才开流式段，否则留下空白「子木：」加常驻光标。
        if (ev.text.trim() === "") return prev;
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
      setEntries((prev) =>
        prev
          // 收口**所有**未完成流式段（工具调用前的段落曾被漏掉 → 光标常驻）
          .map((e) => (e.kind === "assistant" && !e.done ? { ...e, done: true } : e))
          // 整段没产出可见文字的段落直接移除（空白气泡没有信息量）
          .filter((e) => !(e.kind === "assistant" && e.text.trim() === "")),
      );
      setPending(null);
      setBusy(false);
      break;
    case "error":
      setEntries((prev) => [...prev, { kind: "notice", level: "error", text: ev.message }]);
      setBusy(false);
      break;
  }
}
