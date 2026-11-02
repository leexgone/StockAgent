import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import ChatPanel from "../components/ChatPanel";
import { clockView, marketClock } from "../lib/clock";
import { fixed, money } from "../lib/format";
import { useAlerts } from "../store/alerts";
import { useHealth } from "../store/health";
import { useMarket } from "../store/market";
import { usePortfolio } from "../store/portfolio";
import { usePushLog } from "../store/push_log";
import { useUi } from "../store/ui";

/**
 * 智能问答（chat 屏，P5-T2 功能对等移植）—— 对照基准 design/chat.html。
 * 线程 = 真实 Agent SSE（ChatPanel，流式文本 + 工具调用/确认 + skill）——比原型
 * 演示问答库（固定三层脚本 + 假 cites）更强：依据来源就是 Agent 实调的行情/持仓
 * 工具（tool_call/tool_result 如实展示），不伪造引用。
 * 右栏三卡：context（助理现在能看到什么，读 portfolio/alerts/clock 原值）、
 * limit（能力边界自陈，静态）、sessions（最近会话 = push_log 定时任务留痕 +
 * 微信渠道待 C4 接入）。建议问题 chips / ?q= 深链只预填输入不自动发送（真实
 * LLM，交用户确认）；例外 = `?q=…&send=1`（analysis 空态「去问答触发 /建议」
 * 出口——斜杠指令意图明确，跳转即自动发送一次，agent 离线时停留预填态待上线补发）；
 * 会话持久化 localStorage（docs/02 chat 约定，上限 30 条）。
 */

/* push_log.kind → 会话文案（与 index 时间线同映射，03 §12 封闭词汇） */
const KIND_LABEL: Record<string, string> = {
  post_market_review: "盘后复盘",
  alert_push: "预警推送",
};

/* 建议问题（通用真实问法；点击预填输入框，不自动发送） */
const SUGGESTIONS = [
  "今天该做什么？",
  "我的组合最近表现如何？",
  "哪只持仓风险最高？",
  "第一笔持仓怎么录入？",
  "帮我留够下个月要用的钱",
];

export default function ChatView({ now }: { now: Date }) {
  const [params] = useSearchParams();
  const health = useHealth((s) => s.health);
  const view = usePortfolio((s) => s.view);
  const alertRows = useAlerts((s) => s.rows);
  const status = useMarket((s) => s.status);
  const pushRows = usePushLog((s) => s.rows);

  const agentOnline = health?.agent_online ?? false;
  const [prefill, setPrefill] = useState<{ text: string; nonce: number; send?: boolean }>({ text: "", nonce: 0 });
  const [clearNonce, setClearNonce] = useState(0);
  const toast = (m: string) => useUi.getState().pushToast(m);

  // ?q= 深链（analysis「就这只问我」出口）：预填一次，不自动发送；
  // &send=1（analysis「去问答触发 /建议」出口）：预填 + agent 上线后自动发送一次
  const deepQ = params.get("q");
  const deepSend = params.get("send") === "1";
  useEffect(() => {
    if (deepQ) setPrefill({ text: deepQ, nonce: 1, send: deepSend });
  }, [deepQ, deepSend]);

  const agg = view?.agg;
  const holdings = view?.holdings ?? [];
  const per = view?.per ?? [];
  const account = view?.account;
  const clock = status ? clockView(status.clock, now) : marketClock(now);
  const pend = alertRows.filter((a) => !a.done).length;

  const top3 = useMemo(
    () =>
      per
        .map((p, i) => ({ name: holdings[i]?.name ?? p.code, w: p.weight }))
        .sort((a, b) => b.w - a.w)
        .slice(0, 3),
    [per, holdings],
  );

  const ask = (text: string) => setPrefill((p) => ({ text, nonce: p.nonce + 1 }));
  const ctxRow = (k: string, v: React.ReactNode) => (
    <div className="ctx-row">
      <span>{k}</span>
      <span className="v">{v}</span>
    </div>
  );

  return (
    <main className="page" data-od-id="chat-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">智能问答</div>
          <h1 className="page-title">用大白话问，它拿你的持仓回答</h1>
          <p className="page-desc">
            回答只引用你这个账户里的数据：持仓明细、行情快照与仓位纪律。Agent 调用行情/持仓工具的过程如实展示，即为依据来源。
          </p>
        </div>
        <div className="page-actions">
          <span className={`pill ${agentOnline ? "ok" : ""}`} style={{ marginRight: 4 }}>
            {health ? (agentOnline ? "Agent 在线" : "Agent 离线") : "连接中…"}
          </span>
          <button className="btn" id="clear-btn" onClick={() => { setClearNonce((n) => n + 1); toast("会话已清空（仅存在本机浏览器）"); }}>
            清空会话
          </button>
          <button className="btn" id="push-last" onClick={() => toast("微信推送随渠道接入（C4 拍板后）——当前仅本机会话")}>
            把最后一条推送到微信
          </button>
        </div>
      </div>

      <div className="chat-layout">
        {/* 左：建议问题 + 线程（真实 Agent SSE，ChatPanel 自带 card 外壳，不嵌套） */}
        <div className="grid" style={{ gap: 16 }} data-od-id="chat-thread-col">
          <section className="card" data-od-id="chat-suggest-card" style={{ padding: 0 }}>
            <div className="suggest" id="suggest">
              {SUGGESTIONS.map((s) => (
                <button type="button" key={s} onClick={() => ask(s)}>{s}</button>
              ))}
            </div>
          </section>
          <ChatPanel agentOnline={agentOnline} prefill={prefill} clearNonce={clearNonce} persistKey="zimu:chat:me" />
        </div>

        {/* 右：上下文 / 边界 / 最近会话 */}
        <div className="grid rail" style={{ gap: 16 }}>
          <section className="card" data-od-id="chat-context-card">
            <div className="card-head">
              <div>
                <div className="card-title">助理现在能看到什么</div>
                <div className="card-sub" id="ctx-ws">
                  {account ? `${account.name} · ${account.broker || "本地账户"}` : "未连接后端"}
                </div>
              </div>
              <div className="head-actions"><span className="pill">仅本账户</span></div>
            </div>
            <div className="card-body" id="ctx-body" style={{ paddingTop: 8 }}>
              {ctxRow("总资产", agg ? money(agg.total_assets) : "—")}
              {ctxRow("持仓只数", view ? `${holdings.length} 只` : "—")}
              {ctxRow("可用现金", agg ? money(agg.cash) : "—")}
              {ctxRow("仓位", agg ? `${fixed(agg.exposure, 1)}%` : "—")}
              {ctxRow("前三大", top3.length ? top3.map((t) => `${t.name} ${fixed(t.w, 1)}%`).join(" · ") : "暂无持仓")}
              {ctxRow("待处理预警", `${pend} 条`)}
              {ctxRow("市场时段", `${clock.state}${clock.next ? " · " + clock.next : ""}`)}
            </div>
          </section>

          <section className="card" data-od-id="chat-limit-card">
            <div className="card-head"><div><div className="card-title">它不会做的事</div></div></div>
            <div className="card-body">
              <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, lineHeight: 1.8, color: "var(--fg-2)" }}>
                <li>不代为下真实委托，只生成试算</li>
                <li>不预测涨跌幅百分比，只给条件与区间</li>
                <li>不引用你没授权的数据源</li>
                <li>不把置信度说成胜率或收益承诺</li>
              </ul>
            </div>
          </section>

          <section className="card" data-od-id="chat-sessions-card">
            <div className="card-head">
              <div>
                <div className="card-title">最近会话</div>
                <div className="card-sub">定时任务留痕 · 微信渠道待接入</div>
              </div>
            </div>
            <div className="list" id="recent">
              {pushRows.length ? (
                pushRows.slice(0, 6).map((r, i) => (
                  <div className="list-item" data-od-id={`recent-item-${i}`} key={r.id}>
                    <span className="li-main">
                      <span className="li-title">{r.channel === "agent" ? "定时任务" : r.channel} · {KIND_LABEL[r.kind] ?? r.kind}</span>
                      <span className="li-desc">{r.ok ? "已受理" : `受理失败：${r.detail}`}</span>
                    </span>
                    <span className="li-right">
                      <span className="num" style={{ fontSize: 11, color: "var(--muted)" }}>{r.ts}</span>
                      {!r.ok && <span className="pill warn" style={{ display: "block", marginTop: 5 }}>失败</span>}
                    </span>
                  </div>
                ))
              ) : (
                <div className="empty" style={{ padding: "22px 14px" }}>
                  还没有历史会话 · 盘后复盘（15:05）与微信消息完成后会归档在这里（微信随 C4 拍板接入）
                </div>
              )}
            </div>
          </section>
        </div>
      </div>

      <div className="footnote" data-od-id="chat-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          回答由子木 Agent 基于<b>你的真实账户数据与公开行情</b>生成（数字归规则、文字归模型，ADR-0007）；置信度反映样本内信号一致性，不代表未来胜率。
          本程序只给建议、不连接券商、<b>不代为下单</b>，每笔委托仍需你本人确认；不构成收益承诺。
        </span>
      </div>
    </main>
  );
}
