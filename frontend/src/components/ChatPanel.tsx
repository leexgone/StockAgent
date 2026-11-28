import { memo, useCallback, useEffect, useRef, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { formatFileSize, MAX_FILE_BYTES, MAX_FILES, type FileMeta } from "../lib/fileAnnotations";

/**
 * 子木会话面板 —— 真实 Agent SSE 线程（P2-T3 验证链路；P5-T2 起即 chat 屏的
 * 会话线程本体，布局与右栏上下文/边界/会话三卡见 screens/ChatView.tsx）。
 * 流式回答 + 思考过程（thinking_delta，R7/robit 0.1.28，三态折叠对齐 robit-gui）
 * + 工具调用/结果 + 确认请求 + skill 触发如实展示——「依据来源」就是
 * Agent 实调的行情/持仓工具，不伪造引用。
 * 会话真值在服务端（T4/ADR-0011）：`initialEntries` 为档案水合入口；
 * localStorage（persistKey）降级为「服务端不可达时的只读展示缓存」，思考段
 * 两处都不入档（docs/02 展示口径）。
 * 事件形状与后端 SseEvent 对齐（backend/crates/zimu-server/src/agent_bridge.rs）。
 * 助手消息按 GFM Markdown 渲染（ADR-0010）；用户/通知/工具/思考条目保持纯文本。
 */

type SseEvent =
  | { type: "connected"; profile: string; model: string; tools: string[] }
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; text: string }
  | { type: "tool_call"; tool_call_id: string; name: string; arguments: string }
  | { type: "tool_result"; tool_call_id: string; is_error: boolean; content: string }
  | { type: "skill_triggered"; name: string; description: string }
  | { type: "async_tool_completed"; task_id: string; tool_call_id: string; is_error: boolean }
  | { type: "confirm_request"; tool_call_id: string; name: string; arguments: string }
  | { type: "turn_complete" }
  | { type: "error"; message: string };

type Entry =
  | { kind: "user"; text: string; files?: FileMeta[] }
  | { kind: "assistant"; text: string; done: boolean }
  /** 思考段（R7）：closed=false 流式展开中，true=已折叠（头行可点开回看）；不持久化 */
  | { kind: "thinking"; text: string; closed: boolean }
  | { kind: "tool"; id: string; name: string; args: string; result?: { ok: boolean; content: string } }
  | { kind: "notice"; level: "info" | "error"; text: string };

/** 对外暴露条目形态（chatHydrate 的映射目标；导出类型不导出组件内部逻辑） */
export type ChatEntry = Entry;

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
  /** 递增即清空会话（含 localStorage 缓存）；0/undefined = 不触发 */
  clearNonce?: number;
  /** localStorage 缓存键（上限 30 条）——T4 起定位是**服务端不可达时的只读展示**，真值在档案 */
  persistKey?: string;
  /**
   * 服务端会话档案水合（T4）：宿主取到 GET /api/chat/history 后传入
   * （chatHydrate 映射）。首次到达且无在途轮次时覆盖本地一次；此后本地事件流
   * 继续追加（recorder 会把同轮写进档案，下次打开对齐）。undefined = 未就绪
   * （维持 localStorage 降级展示）。
   */
  initialEntries?: ChatEntry[];
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

export default function ChatPanel({ agentOnline, prefill, clearNonce, persistKey, initialEntries, onConnected }: ChatPanelProps) {
  const [entries, setEntries] = useState<Entry[]>(() => (persistKey ? loadPersisted(persistKey) : []));
  const [conn, setConn] = useState<Conn>("connecting");
  const [pending, setPending] = useState<Pending>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  /** 待发送附件（ADR-0015）：选中即显示 chips，点发送才真正上传（multipart） */
  const [files, setFiles] = useState<File[]>([]);
  const logRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  /** 已自动发送过的 prefill nonce（send=1 深链只发一次，busy/离线恢复后不重放） */
  const autoSent = useRef(0);
  // onConnected 走 ref：SSE effect 只依赖 agentOnline，宿主换回调不重连事件流
  const onConnectedRef = useRef(onConnected);
  useEffect(() => {
    onConnectedRef.current = onConnected;
  }, [onConnected]);

  // 持久化（上限 30 条；思考段除外——展示性瞬态不入档，与 robit「thinking 不写
  // history/不落盘」语义一致，会话重开即消失）
  useEffect(() => {
    if (!persistKey) return;
    try {
      localStorage.setItem(
        persistKey,
        JSON.stringify(entries.filter((e) => e.kind !== "thinking").slice(-30)),
      );
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

  // 服务端档案水合（T4）：首次就绪且无在途轮次时覆盖本地一次——档案是真值，
  // localStorage 只是缓存。若此刻正有进行中的轮次（流式未定稿/思考段），不抢：
  // 那轮结束时会由 recorder 写入档案，下次打开自然对齐。
  const hydratedRef = useRef(false);
  useEffect(() => {
    if (hydratedRef.current || !initialEntries) return;
    const streaming =
      busy ||
      entries.some((e) => (e.kind === "assistant" && !e.done) || e.kind === "thinking");
    if (streaming) return;
    hydratedRef.current = true;
    setEntries(initialEntries);
  }, [initialEntries, entries, busy]);

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

  // 新内容自动滚到底部。agentOnline 必须在依赖里：滚动容器 .chat-log 包在
  // `{agentOnline && …}` 内，刷新时 health 探测未回→容器未挂载→此时水合触发的
  // 本 effect 摸到 logRef.current=null 直接跳过；agentOnline 翻真容器才挂载，
  // 若不进依赖则 entries/pending 未再变、effect 不重跑，页面永远停在顶部第一条
  //（2026-11-28 复盘：刷新回顶部的根因）。
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [entries, pending, agentOnline]);

  // 流式期间输入框虽不再上锁，但焦点可能被发送/取消按钮点击或页面操作夺走——
  // 本轮回复结束（busy true→false 边沿）把光标交还输入框，主人可无缝续打下一条。
  const prevBusy = useRef(false);
  useEffect(() => {
    if (prevBusy.current && !busy) inputRef.current?.focus();
    prevBusy.current = busy;
  }, [busy]);

  // Agent 上线时聚焦输入框：输入框随 `{agentOnline && …}` 挂载，刷新/进聊天屏
  // 后光标直接在输入框里（与滚动 effect 补 agentOnline 依赖同源——容器挂载时序）。
  useEffect(() => {
    if (agentOnline) inputRef.current?.focus();
  }, [agentOnline]);

  /** 发送核心：手动提交与 send=1 深链自动发送共用（入口不同，链路与留痕完全一致）。
   *  ADR-0015：multipart/form-data（text + files），浏览器自设 boundary 不手设
   *  Content-Type；失败保留输入与附件供重试，成功才清空（乐观条目含附件 chips）。 */
  const submit = useCallback(
    async (raw: string, attach: File[] = []) => {
      const text = raw.trim();
      if ((!text && attach.length === 0) || busy) return;
      const metas: FileMeta[] = attach.map((f) => ({ name: f.name, size: f.size }));
      setBusy(true);
      const fd = new FormData();
      fd.append("text", text);
      for (const f of attach) fd.append("files", f);
      let r: Response;
      try {
        r = await fetch("/api/chat", { method: "POST", body: fd });
      } catch (e) {
        setBusy(false);
        setEntries((prev) => [
          ...prev,
          { kind: "notice", level: "error", text: `发送失败：${String(e)}` },
        ]);
        return;
      }
      if (!r.ok) {
        setBusy(false);
        const body = await r.text();
        setEntries((prev) => [
          ...prev,
          { kind: "notice", level: "error", text: `发送失败：HTTP ${r.status} ${body}` },
        ]);
        return; // 保留 input + files 供重试
      }
      setEntries((prev) => [
        ...prev,
        { kind: "user", text, ...(metas.length ? { files: metas } : {}) },
      ]);
      setInput("");
      setFiles([]);
      // 发送成功即回到输入框：按钮点击路径焦点原本留在「发送」上，Enter 路径
      // 也显式归位——配合输入框不再 disabled，流式期间可直接预打下条内容。
      inputRef.current?.focus();
    },
    [busy],
  );

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    await submit(input, files);
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

  /** 收附件（📎 选择 / 拖拽 / 粘贴共用）：客户端预检大小与数量、去重；
   *  服务端是最终裁判（超限 413）。拒绝走 notice 条目（组件内错误惯例）。 */
  const addFiles = useCallback(
    (list: FileList | File[] | null) => {
      const incoming = Array.from(list ?? []);
      if (incoming.length === 0) return;
      const next = [...files];
      const rejected: string[] = [];
      for (const f of incoming) {
        if (next.length >= MAX_FILES) {
          rejected.push(`${f.name}（超出 ${MAX_FILES} 个上限）`);
          continue;
        }
        if (f.size > MAX_FILE_BYTES) {
          rejected.push(`${f.name}（超过 ${formatFileSize(MAX_FILE_BYTES)}）`);
          continue;
        }
        if (next.some((x) => x.name === f.name && x.size === f.size)) continue;
        next.push(f);
      }
      setFiles(next);
      if (rejected.length) {
        setEntries((prev) => [
          ...prev,
          { kind: "notice", level: "error", text: `已拒绝附件：${rejected.join("；")}` },
        ]);
      }
    },
    [files],
  );

  const removeFile = (idx: number) => setFiles((prev) => prev.filter((_, j) => j !== idx));

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    if (agentOnline) addFiles(e.dataTransfer.files);
  };

  return (
    <section
      className="card chat"
      data-od-id="home-chat"
      onDrop={onDrop}
      onDragOver={(e) => e.preventDefault()}
    >
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
                输入消息开始对话；试试「今天行情如何」，或 /复盘 /盘前 /建议 /诊断。
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

          {files.length > 0 && (
            <div className="chat-attachments" data-od-id="chat-attachments">
              {files.map((f, i) => (
                <span className="pill plain chat-file-chip" key={`${f.name}-${i}`}>
                  📎 {f.name}
                  {f.size > 0 && <span className="chat-file-size">{formatFileSize(f.size)}</span>}
                  <button
                    type="button"
                    className="chat-file-remove"
                    aria-label={`移除 ${f.name}`}
                    onClick={() => removeFile(i)}
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          )}

          <form className="chat-input-row" onSubmit={send} data-od-id="home-chat-input">
            <button
              type="button"
              className="icon-btn chat-attach-btn"
              aria-label="上传文件"
              disabled={busy}
              onClick={() => fileInputRef.current?.click()}
            >
              📎
            </button>
            <input
              ref={fileInputRef}
              type="file"
              multiple
              hidden
              onChange={(e) => {
                addFiles(e.target.files);
                e.target.value = ""; // 允许再次选择同一文件
              }}
            />
            <input
              type="text"
              ref={inputRef}
              value={input}
              placeholder={busy ? "等待本轮回复…" : "对子木说点什么…（可拖拽/粘贴文件）"}
              onChange={(e) => setInput(e.target.value)}
              onPaste={(e) => {
                if (e.clipboardData.files.length > 0) addFiles(e.clipboardData.files);
              }}
              autoComplete="off"
            />
            <button
              type="submit"
              className="btn btn-primary"
              disabled={busy || (!input.trim() && files.length === 0)}
            >
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

// memo：SSE 每个 delta 都会 setEntries 产新数组，但只有末条流式 assistant 的
// entry 引用变化——其余条（尤其历史 assistant 的整棵 react-markdown 树）props
// 引用相等即跳过重渲染。未 memo 时每 chunk 全表重解析，大历史（~200KB / 30+ 条）
// 下主线程被 markdown 解析占满 → 页面卡死（2026-11-28 事故复盘）。
const EntryView = memo(function EntryView({ entry }: { entry: Entry }) {
  switch (entry.kind) {
    case "user":
      return (
        <div className="chat-user-wrap">
          <p className="chat-user">我：{entry.text}</p>
          {entry.files && entry.files.length > 0 && (
            <div className="chat-user-files" data-od-id="chat-user-files">
              {entry.files.map((f, i) => (
                <span className="pill plain" key={i}>
                  📎 {f.name}
                  {f.size > 0 ? ` · ${formatFileSize(f.size)}` : ""}
                </span>
              ))}
            </div>
          )}
        </div>
      );
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
    case "thinking":
      return <ThinkingView text={entry.text} closed={entry.closed} />;
    case "tool":
      return <ToolView entry={entry} />;
    case "notice":
      return (
        <p className={entry.level === "error" ? "chat-notice chat-notice-err" : "chat-notice"}>
          {entry.text}
        </p>
      );
  }
});

/* ---------- 工具调用卡片（折叠展开，对齐思考段三态取舍） ----------
   执行中（result 未到）：恒展开——工具名 + 参数摘要 + 「⏳ 执行中…」进行指示，
   让用户看到在跑什么；执行完成（result 到达）：默认折叠为头行「▸ 🔧 名称 ✓/✗」，
   点击头行展开看参数与完整结果（结果原文纯文本、限高内滚，不走 Markdown）。
   折叠/展开是纯 UI 态（userOpen），不写回 entries——与思考段回看同款；完成态由
   result 是否存在派生，无需新增 SSE 事件或条目字段（水合的历史工具行恒带 result，
   读回即默认折叠）。wire 契约与会话档案 schema 零改动。 */
function ToolView({ entry }: { entry: Extract<Entry, { kind: "tool" }> }) {
  const [userOpen, setUserOpen] = useState(false);
  const result = entry.result;

  // 执行中：恒展开（无折叠按钮），参数 + 进行指示
  if (!result) {
    return (
      <div className="chat-tool">
        <p className="chat-tool-name">
          🔧 {entry.name} <span className="chat-tool-running">⏳ 执行中…</span>
        </p>
        <p className="chat-tool-args">{briefArgs(entry.args)}</p>
      </div>
    );
  }

  // 完成：默认折叠头行（名称 + ✓/✗），点击展开参数与完整结果
  const statusCls = result.ok ? "chat-tool-ok" : "chat-tool-err";
  return (
    <div className="chat-tool">
      <button
        type="button"
        className="chat-tool-head chat-tool-toggle"
        aria-expanded={userOpen}
        onClick={() => setUserOpen((o) => !o)}
      >
        <span className="chat-tool-caret" aria-hidden="true">
          {userOpen ? "▾" : "▸"}
        </span>
        <span className="chat-tool-name">🔧 {entry.name}</span>
        <span className={`chat-tool-status ${statusCls}`}>{result.ok ? "✓" : "✗"}</span>
      </button>
      {userOpen && (
        <div className="chat-tool-body">
          <p className="chat-tool-args">{briefArgs(entry.args)}</p>
          <p className={`chat-tool-result ${statusCls}`}>{result.content}</p>
        </div>
      )}
    </div>
  );
}

/* ---------- 思考过程段（R7，robit 0.1.28 thinking_delta；2026-11-06 拍板对齐
   robit-gui 三态）----------
   live：思考中展开流式灰字（自动滚到尾部）→ 首个正文/工具事件到达折叠为
   collapsed：「▸ 思考过程」头行，点击回看；turn_complete 兜底闭合。
   思考文本按纯文本渲染（pre-wrap），不走 Markdown——半截语法在流式中会抖动
   （GUI 参考实现同款取舍）。 */
function ThinkingView({ text, closed }: { text: string; closed: boolean }) {
  // 回看展开是纯 UI 态：closed 后用户点击才展开，不写回 entries
  const [userOpen, setUserOpen] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  // live 态流式自动滚到尾部（折叠/回看态不干预滚动位置）
  useEffect(() => {
    const el = bodyRef.current;
    if (!closed && el) el.scrollTop = el.scrollHeight;
  }, [text, closed]);

  if (!closed) {
    return (
      <div className="chat-thinking">
        <span className="chat-thinking-head">💭 思考中…</span>
        <div className="chat-thinking-body" ref={bodyRef}>
          {text}
        </div>
      </div>
    );
  }
  return (
    <div className="chat-thinking">
      <button
        type="button"
        className="chat-thinking-head chat-thinking-toggle"
        aria-expanded={userOpen}
        onClick={() => setUserOpen((o) => !o)}
      >
        {userOpen ? "▾" : "▸"} 思考过程
      </button>
      {userOpen && <div className="chat-thinking-body">{text}</div>}
    </div>
  );
}

/** 折叠所有未闭思考段（三态 live → collapsed；幂等，供正文/工具/收口事件复用） */
function closeThinking(entries: Entry[]): Entry[] {
  return entries.map((e) => (e.kind === "thinking" && !e.closed ? { ...e, closed: true } : e));
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
    case "thinking_delta":
      // 思考增量（R7，robit 0.1.28）：续写未闭思考段，否则开新段（工具调用后
      // 可能再起一段思考）。无显式结束标记——首个正文/工具事件或 turn_complete 折叠。
      setEntries((prev) => {
        if (ev.text === "") return prev;
        const last = prev[prev.length - 1];
        if (last && last.kind === "thinking" && !last.closed) {
          return [...prev.slice(0, -1), { ...last, text: last.text + ev.text }];
        }
        return [...prev, { kind: "thinking", text: ev.text, closed: false }];
      });
      break;
    case "text_delta":
      setEntries((prev) => {
        const last = prev[prev.length - 1];
        if (last && last.kind === "assistant" && !last.done) {
          return [...prev.slice(0, -1), { ...last, text: last.text + ev.text }];
        }
        // 空白首帧不建占位气泡——只有真正可见的文字才开流式段。robit 0.1.28 起
        // 空 content 帧已在发射端收口（R6 了断），此守卫保留为惰性兼容。
        if (ev.text.trim() === "") return prev;
        // 新开正文段：未闭思考段随之折叠（三态 live → collapsed，对齐 robit-gui）
        const closed = closeThinking(prev);
        return [...closed, { kind: "assistant", text: ev.text, done: false }];
      });
      break;
    case "tool_call":
      setEntries((prev) => [
        // 工具调用同样闭合思考段（GUI 参考实现以 ToolCallRequested 作段闭合信号）
        ...closeThinking(prev),
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
        closeThinking(prev)
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
