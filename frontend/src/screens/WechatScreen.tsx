import { useEffect, useState } from "react";
import { IconChat, IconGrid, IconWechat } from "../components/Icons";
import { fixed } from "../lib/format";
import { maskChatId } from "../lib/mask";
import { channelLabel, pushKindLabel } from "../lib/pushKind";
import { kindForChatId, useBindings, type ChannelBindingRow, type PendingAttempt } from "../store/bindings";
import { useEquity } from "../store/equity";
import { useHealth } from "../store/health";
import { usePushLog } from "../store/push_log";
import { useUi } from "../store/ui";

/**
 * 助理接入（wechat 屏，P5-T2 收尾 / ADR-0017，C4 关闭）—— 对照基准
 * design/wechat.html（冻结原型，服务号语义作废，生产形态见 docs/02 wechat 段）。
 *
 * 六卡生产形态：
 * - channel：QQ / 微信 / Web 三行状态**只读**（启用开关在服务器配置文件，拍板⑤⑥；
 *   online 与 enabled 区分「离线」与「未启用」，ADR-0017 D6）；无 switch、无「测一下」。
 * - push：**待启动态**——「当前生效（绑定即推）」如实列两条真实推送；订阅管理
 *   （6 类/时段/静音）另案 C15，禁用占位、不枚举六类标签（词汇冲突未决）。
 * - config：iLink Bot 脱敏只读（凭据只在服务器 state 文件，界面不显示任何 token）。
 * - bind：**通道无关白名单管理（QQ 同样受益）**——待绑定会话自动列出（D3 补记：
 *   未绑定来消息入观察队列，一键绑定，不用抄日志）+ 绑定/解绑（两步确认弹窗）/
 *   手工表单兜底；chat_id 默认掩码脱敏、逐行「查看/收起」切换（lib/mask，
 *   展示层投影不动 API 真值）；扫码不上 Web（登录 = 部署侧 CLI，停服约定见 backend/README）。
 * - preview：纯文本消息样例（无按钮——服务号模板消息语义生产不存在）；摘要 =
 *   最近 equity_snapshots 真实行按后端 summary_message 同款行格式（改后端须同步这里）。
 * - log：最近 6 条全通道 push_log（detail 原样，ok=1 只表示已受理，03 §12）。
 *
 * 纪律：B5 状态色（在线/已绑定 = --ok 绿，绝不用红）；--accent ≤2（仅绑定提交）；
 * 空账户六卡不报错；屏内 60s 轮询（卸载清理，不加全局负载）。
 */

/* 后端 summary_message 的 {:+.2} 镜像（fixed 剥负零，显式正号） */
const signed = (v: number, d: number): string => (v >= 0 ? "+" : "") + fixed(v, d);

export default function WechatScreen() {
  const health = useHealth((s) => s.health);
  const rows = useBindings((s) => s.rows);
  const pending = useBindings((s) => s.pending);
  const bindingsLoaded = useBindings((s) => s.loaded);
  const bindingsError = useBindings((s) => s.error);
  const pushRows = usePushLog((s) => s.rows);
  const equityRows = useEquity((s) => s.rows);

  const toast = (m: string) => useUi.getState().pushToast(m);

  // 绑定表单（channel 选 wechat 时 kind 锁死 private——微信仅 1:1）
  const [channel, setChannel] = useState<"qq" | "wechat">("wechat");
  const [chatId, setChatId] = useState("");
  const [kind, setKind] = useState<"private" | "group">("private");
  const [label, setLabel] = useState("");
  const [busy, setBusy] = useState(false);
  // 解绑两步确认弹窗（镜像原型 unbind-modal，Esc/scrim 可关）
  const [unbindTarget, setUnbindTarget] = useState<ChannelBindingRow | null>(null);
  // chat_id 展示脱敏：默认掩码，逐行「查看/收起」；组件态离屏即复位回掩码
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const idKey = (ch: string, id: string) => `${ch}:${id}`;
  const toggleReveal = (key: string) => setRevealed((s) => ({ ...s, [key]: !s[key] }));

  useEffect(() => {
    const loadAll = () => {
      void useBindings.getState().load();
      void useBindings.getState().loadPending();
      void useHealth.getState().load();
      void usePushLog.getState().load();
      void useEquity.getState().load();
    };
    loadAll();
    const t = setInterval(loadAll, 60_000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    if (!unbindTarget) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setUnbindTarget(null);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [unbindTarget]);

  const effKind = channel === "wechat" ? "private" : kind;

  const doBind = async () => {
    const id = chatId.trim();
    if (!id) {
      toast("请填写 chat_id（先给 bot 发一条消息，从服务端日志抄录）");
      return;
    }
    setBusy(true);
    const ok = await useBindings.getState().bind({
      channel,
      chat_id: id,
      kind: effKind,
      ...(label.trim() ? { label: label.trim() } : {}),
    });
    setBusy(false);
    if (ok) {
      toast(`已绑定 ${channelLabel(channel)} 会话 ${maskChatId(id)}`);
      setChatId("");
      setLabel("");
    }
  };

  // 待绑定一键绑定（D3 补记：观察自动化、授权仍人工——点击是主人的明示动作；
  // kind 由 chat_id 前缀推断，微信恒 private）
  const bindPending = async (p: PendingAttempt) => {
    const ok = await useBindings.getState().bind({
      channel: p.channel,
      chat_id: p.chat_id,
      kind: kindForChatId(p.chat_id),
    });
    if (ok) toast(`已绑定 ${channelLabel(p.channel)} 会话 ${maskChatId(p.chat_id)}，现在发消息即可应答`);
  };

  const doUnbind = async () => {
    if (!unbindTarget) return;
    const t = unbindTarget;
    setUnbindTarget(null);
    const ok = await useBindings.getState().unbind(t.channel, t.chat_id);
    if (ok) toast(`已解绑 ${channelLabel(t.channel)} 会话 ${maskChatId(t.chat_id)}（留痕 active=0，可随时重新绑定）`);
  };

  /* ---------- 通道状态（只读） ---------- */
  const channels = [
    {
      key: "wechat",
      name: "微信（iLink Bot）",
      desc: "1:1 私聊问答 + 预警/盘后摘要推送 · 出站长轮询无需公网回调",
      icon: <IconWechat />,
      enabled: health?.wechat_enabled ?? false,
      online: health?.wechat_online ?? false,
    },
    {
      key: "qq",
      name: "QQ",
      desc: "私聊/群聊问答 + 预警/盘后摘要推送 · 出站 WS 网关",
      icon: <IconChat />,
      enabled: health?.qq_enabled ?? false,
      online: health?.qq_online ?? false,
    },
    {
      key: "web",
      name: "Web 工作台",
      desc: "本页面 · 流式对话与全量功能（Agent 在线状态见问答屏）",
      icon: <IconGrid />,
      enabled: true,
      online: health !== null,
    },
  ];
  const onlineCount = channels.filter((c) => c.online).length;

  /* ---------- 摘要预览（数字 = 最近快照原值，行格式镜像后端 summary_message） ---------- */
  const lastSnap = equityRows.length > 0 ? equityRows[equityRows.length - 1] : null;
  const summaryPreview = lastSnap
    ? [
        `子木盘后摘要 ${lastSnap.date}`,
        `总资产 ${fixed(lastSnap.total_assets, 2)}`,
        `当日盈亏 ${signed(lastSnap.day_pnl, 2)}（${signed(lastSnap.day_pnl_pct, 2)}%）`,
        `累计盈亏 ${signed(lastSnap.total_pnl, 2)}（${signed(lastSnap.total_pnl_pct, 2)}%）`,
        `现金 ${fixed(lastSnap.cash, 2)} · 仓位 ${fixed(lastSnap.exposure, 1)}% · 集中度 ${fixed(lastSnap.hhi, 2)}`,
      ].join("\n")
    : null;

  return (
    <main className="page" data-od-id="wechat-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">助理接入</div>
          <h1 className="page-title">让预警和摘要送到手机上</h1>
          <p className="page-desc">
            微信（iLink Bot）与 QQ 双通道共享同一个助理：私聊问答、预警与盘后数字摘要推送。
            接入只负责「推送 + 回答」，不读取券商账户、不接管下单；绑定关系在本页管理，连接参数只在服务器配置文件。
          </p>
        </div>
        <div className="page-actions">
          <button
            className="btn"
            disabled
            title="测试推送端点另案——当前预警与盘后摘要自动推送绑定会话"
          >
            发送测试推送
          </button>
        </div>
      </div>

      <div className="wx-layout">
        <div className="grid" style={{ gap: 16 }}>
          {/* ---------- 接入通道（只读状态） ---------- */}
          <section className="card" data-od-id="wx-channel-card">
            <div className="card-head">
              <div>
                <div className="card-title">接入通道</div>
                <div className="card-sub">启用开关在服务器配置文件 [zimu.channels]，界面只读（ADR-0011 拍板⑤⑥）</div>
              </div>
              <div className="head-actions">
                <span className={`pill ${health ? (onlineCount >= 1 ? "ok" : "warn") : "plain"}`}>
                  {health ? `${onlineCount} / ${channels.length} 在线` : "连接中…"}
                </span>
              </div>
            </div>
            <div>
              {channels.map((c, i) => (
                <div className="chan" data-od-id={`channel-${i}`} key={c.key}>
                  <span className="ic">{c.icon}</span>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 550 }}>
                      {c.name}{" "}
                      {!health ? (
                        <span className="pill plain" style={{ marginLeft: 6 }}>…</span>
                      ) : !c.enabled ? (
                        <span className="pill plain" style={{ marginLeft: 6 }}>未启用</span>
                      ) : c.online ? (
                        <span className="pill ok" style={{ marginLeft: 6 }}>在线</span>
                      ) : (
                        <span className="pill warn" style={{ marginLeft: 6 }}>离线</span>
                      )}
                    </div>
                    <div style={{ fontSize: 11.5, color: "var(--muted)", marginTop: 3 }}>{c.desc}</div>
                  </div>
                </div>
              ))}
            </div>
          </section>

          {/* ---------- 推送订阅（待启动态，C15 另案） ---------- */}
          <section className="card" data-od-id="wx-push-card">
            <div className="card-head">
              <div>
                <div className="card-title">推送订阅</div>
                <div className="card-sub">推什么、什么时候推</div>
              </div>
              <div className="head-actions"><span className="pill plain">订阅管理待启用</span></div>
            </div>
            <div className="card-body" style={{ paddingTop: 10 }}>
              <div className="hint" style={{ margin: "0 0 8px" }}>当前生效（绑定即推，无订阅过滤）：</div>
              <div className="switch-row" style={{ padding: "10px 0" }}>
                <div style={{ flex: 1 }}>
                  <div className="t">预警推送</div>
                  <div className="d">03 §12 判定序触发（涨跌停、放量、强赎、止盈止损等），即推绑定的微信/QQ 会话，同日同类去重。</div>
                </div>
              </div>
              <div className="switch-row" style={{ padding: "10px 0", borderBottom: 0 }}>
                <div style={{ flex: 1 }}>
                  <div className="t">盘后数字摘要</div>
                  <div className="d">15:05 equity_snapshots 落库原值推送绑定会话；复盘正文留在 Web 会话。</div>
                </div>
              </div>
            </div>
            <div className="card-body" style={{ borderTop: "1px solid var(--border)" }}>
              <div className="hint" style={{ margin: "0 0 10px" }}>
                订阅管理（6 类订阅 / 推送时段 / 静音标的）另案落地（C15：词汇对齐 + schema v5 + 03 口径定稿）——以下为占位，暂不可配。
              </div>
              <div className="grid g-2" style={{ gap: "0 14px" }}>
                <div className="field" style={{ margin: 0 }}>
                  <label htmlFor="wx-t-from">推送时段 · 开始</label>
                  <input className="input" type="time" id="wx-t-from" defaultValue="08:30" disabled />
                </div>
                <div className="field" style={{ margin: 0 }}>
                  <label htmlFor="wx-t-to">推送时段 · 结束</label>
                  <input className="input" type="time" id="wx-t-to" defaultValue="21:00" disabled />
                </div>
              </div>
              <div className="field" style={{ margin: "16px 0 0" }}>
                <label htmlFor="wx-quiet">静音标的（可多选）</label>
                <select className="select" id="wx-quiet" multiple size={3} disabled>
                  <option>待启用 · 订阅体系落地后可选</option>
                </select>
              </div>
            </div>
          </section>

          {/* ---------- 接入配置（iLink Bot 脱敏只读） ---------- */}
          <section className="card" data-od-id="wx-config-card">
            <div className="card-head">
              <div>
                <div className="card-title">接入配置 · 微信 iLink Bot</div>
                <div className="card-sub">连接参数只在服务器配置文件，界面脱敏只读（ADR-0011 拍板⑤）</div>
              </div>
              <div className="head-actions">
                {health ? (
                  health.wechat_online ? <span className="pill ok">在线</span>
                    : health.wechat_enabled ? <span className="pill warn">离线</span>
                      : <span className="pill plain">未启用</span>
                ) : <span className="pill plain">…</span>}
              </div>
            </div>
            <div className="card-body wx-config" style={{ paddingTop: 8 }}>
              <div className="ctx-row"><span>渠道形态</span><span className="v">微信官方 iLink AI Bot · 个人助理（非服务号）</span></div>
              <div className="ctx-row"><span>凭据</span><span className="v">bot_token 存服务器 backend/.robit/wechat-state.json（永不入 git，界面不显示）</span></div>
              <div className="ctx-row"><span>登录</span><span className="v">部署侧停服运行 robit-wechat -w &lt;backend&gt; login（终端扫码）；token 失效后重扫，60s 内自动重建</span></div>
              <div className="ctx-row"><span>网络</span><span className="v">出站长轮询 · 无需公网回调 / 内网穿透</span></div>
              <div className="ctx-row"><span>启用开关</span><span className="v">[zimu.channels].wechat（服务器配置文件，缺省停用）</span></div>
            </div>
          </section>
        </div>

        {/* ---------- 右栏 ---------- */}
        <div className="grid" style={{ gap: 16 }}>
          {/* ---------- 会话绑定管理（通道无关白名单） ---------- */}
          <section className="card" data-od-id="wx-bind-card">
            <div className="card-head">
              <div>
                <div className="card-title">会话绑定管理</div>
                <div className="card-sub">入站白名单 · QQ / 微信共用（绑定即授权对话与推送）</div>
              </div>
              <div className="head-actions">
                <span className={`pill ${rows.length ? "ok" : "warn"}`}>
                  {bindingsLoaded ? (rows.length ? `已绑定 ${rows.length}` : "未绑定") : "…"}
                </span>
              </div>
            </div>
            <div className="list">
              {rows.length ? (
                rows.map((r, i) => {
                  const key = idKey(r.channel, r.chat_id);
                  const shown = revealed[key] ?? false;
                  return (
                    <div className="list-item" data-od-id={`binding-${i}`} key={key}>
                      <span className="li-main">
                        <span className="li-title">
                          <span className="pill plain" style={{ marginRight: 6 }}>{channelLabel(r.channel)}</span>
                          <span className="code num" style={{ fontSize: 12 }}>{shown ? r.chat_id : maskChatId(r.chat_id)}</span>
                        </span>
                        <span className="li-desc">
                          {[r.kind === "private" ? "私聊" : "群聊", r.label, `绑定于 ${r.bound_at}`].filter(Boolean).join(" · ")}
                        </span>
                      </span>
                      <span className="li-right">
                        <button className="btn btn-sm" aria-pressed={shown} title={shown ? "重新掩码" : "查看完整 chat_id"} onClick={() => toggleReveal(key)}>
                          {shown ? "收起" : "查看"}
                        </button>
                        <button className="btn btn-sm" style={{ color: "var(--danger)", borderColor: "var(--danger)" }} onClick={() => setUnbindTarget(r)}>
                          解绑
                        </button>
                      </span>
                    </div>
                  );
                })
              ) : (
                <div className="empty" style={{ padding: "18px 14px" }}>
                  {bindingsLoaded
                    ? "还没有活跃绑定 · 未绑定会话来消息被静默丢弃（服务端日志留 chat_id）；用下方表单绑定后开始对话与推送"
                    : "读取中…"}
                </div>
              )}
            </div>
            {/* 待绑定会话（D3 补记：未绑定来消息自动入列，一键绑定——不用抄日志） */}
            <div className="card-body" style={{ borderTop: "1px solid var(--border)", paddingBottom: 10 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
                <span style={{ fontSize: 12.5, fontWeight: 550 }}>待绑定会话</span>
                {pending.length > 0 && <span className="pill warn">{pending.length}</span>}
                <button
                  className="btn btn-sm"
                  style={{ marginLeft: "auto" }}
                  onClick={() => void useBindings.getState().loadPending()}
                >
                  刷新
                </button>
              </div>
              {pending.length ? (
                pending.map((p, i) => {
                  const key = idKey(p.channel, p.chat_id);
                  const shown = revealed[key] ?? false;
                  return (
                    <div className="list-item" data-od-id={`pending-${i}`} key={key} style={{ padding: "8px 0" }}>
                      <span className="li-main">
                        <span className="li-title">
                          <span className="pill plain" style={{ marginRight: 6 }}>{channelLabel(p.channel)}</span>
                          <span className="code num" style={{ fontSize: 12 }}>{shown ? p.chat_id : maskChatId(p.chat_id)}</span>
                        </span>
                        <span className="li-desc">
                          {`来消息 ${p.hits} 次 · 最近 ${p.last_seen}${p.last_text ? ` · 「${p.last_text}」` : ""}`}
                        </span>
                      </span>
                      <span className="li-right">
                        <button className="btn btn-sm" aria-pressed={shown} title={shown ? "重新掩码" : "查看完整 chat_id"} onClick={() => toggleReveal(key)}>
                          {shown ? "收起" : "查看"}
                        </button>
                        <button className="btn btn-sm" onClick={() => void bindPending(p)}>绑定</button>
                      </span>
                    </div>
                  );
                })
              ) : (
                <div className="hint" style={{ margin: 0 }}>
                  暂无待绑定会话 · 未绑定会话发来消息会自动出现在这里（点「刷新」或等 60s 轮询）
                </div>
              )}
            </div>
            <div className="card-body" style={{ borderTop: "1px solid var(--border)" }}>
              <div className="field" style={{ margin: "0 0 10px" }}>
                <label htmlFor="bind-channel">通道</label>
                <select
                  className="select"
                  id="bind-channel"
                  value={channel}
                  onChange={(e) => {
                    const c = e.target.value as "qq" | "wechat";
                    setChannel(c);
                    if (c === "wechat") setKind("private");
                  }}
                >
                  <option value="wechat">微信（仅 1:1 私聊）</option>
                  <option value="qq">QQ</option>
                </select>
              </div>
              <div className="field" style={{ margin: "0 0 10px" }}>
                <label htmlFor="bind-chatid">chat_id</label>
                <input
                  className="input num"
                  id="bind-chatid"
                  placeholder={channel === "wechat" ? "private:…" : "private:… / group:…"}
                  value={chatId}
                  onChange={(e) => setChatId(e.target.value)}
                />
              </div>
              <div className="grid g-2" style={{ gap: "0 10px" }}>
                <div className="field" style={{ margin: "0 0 10px" }}>
                  <label htmlFor="bind-kind">会话类型</label>
                  <select
                    className="select"
                    id="bind-kind"
                    value={effKind}
                    disabled={channel === "wechat"}
                    onChange={(e) => setKind(e.target.value as "private" | "group")}
                  >
                    <option value="private">私聊</option>
                    <option value="group">群聊</option>
                  </select>
                </div>
                <div className="field" style={{ margin: "0 0 10px" }}>
                  <label htmlFor="bind-label">备注（可选）</label>
                  <input className="input" id="bind-label" placeholder="如：本人微信" value={label} onChange={(e) => setLabel(e.target.value)} />
                </div>
              </div>
              {bindingsError && <div className="err" style={{ display: "block", marginBottom: 8 }}>{bindingsError}</div>}
              <button className="btn btn-primary" disabled={busy} onClick={() => void doBind()}>
                {busy ? "绑定中…" : "绑定"}
              </button>
              <div className="hint" style={{ marginTop: 10 }}>
                常规路径：给 bot 发一条消息 → 上方「待绑定会话」自动列出 → 点「绑定」。
                此处手工表单为兜底（服务端日志仍留 chat_id 案底）。微信登录（扫码）在部署侧 CLI 完成，见「接入配置」卡。
              </div>
            </div>
          </section>

          {/* ---------- 推送预览（纯文本样例，无按钮） ---------- */}
          <section className="card" data-od-id="wx-preview-card">
            <div className="card-head">
              <div>
                <div className="card-title">推送长这样</div>
                <div className="card-sub">纯文本 · 2000 字上限超长截断</div>
              </div>
            </div>
            <div className="card-body">
              <div className="hint" style={{ margin: "0 0 6px" }}>盘后数字摘要{lastSnap ? `（最近快照 ${lastSnap.date} 原值）` : ""}：</div>
              <div className="wxmsg">
                {summaryPreview ?? "还没有快照 · 交易日 15:05 盘后链写入后，这里按真实数字渲染"}
              </div>
              <div className="hint" style={{ margin: "12px 0 6px" }}>预警推送（样例，文案 = 判定产出原值）：</div>
              <div className="wxmsg">{"子木预警 · 样例标的 跌破止损线 0000.00（样例）"}</div>
            </div>
          </section>

          {/* ---------- 推送日志（全通道最近 6 条） ---------- */}
          <section className="card" data-od-id="wx-log-card">
            <div className="card-head">
              <div>
                <div className="card-title">推送日志</div>
                <div className="card-sub">最近 6 条 · 全通道（ok=1 只表示已受理）</div>
              </div>
            </div>
            <div className="list">
              {pushRows.length ? (
                pushRows.slice(0, 6).map((r, i) => (
                  <div className="list-item" data-od-id={`pushlog-${i}`} key={r.id}>
                    <span style={{ width: 8, height: 8, borderRadius: "50%", flex: "none", background: `var(--${r.ok ? "ok" : "warn"})` }} />
                    <span className="li-main">
                      <span className="li-title" style={{ fontWeight: 450 }}>
                        {channelLabel(r.channel)} · {pushKindLabel(r.kind)}
                      </span>
                      <span className="li-desc">{r.detail}</span>
                    </span>
                    <span className="li-right num" style={{ fontSize: 11, color: "var(--muted)" }}>{r.ts}</span>
                  </div>
                ))
              ) : (
                <div className="empty" style={{ padding: "22px 14px" }}>
                  还没有推送记录 · 绑定会话后，持仓触发首个条件或 15:05 盘后链开始留档
                </div>
              )}
            </div>
          </section>
        </div>
      </div>

      <div className="footnote" data-od-id="wx-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          接入仅做<b>消息推送与问答</b>，不读取券商账户、不代为提交委托；推送内容同样不构成投资建议。
          凭据（bot_token）只存服务器本地文件、永不入 git，界面不显示任何 token 值。
        </span>
      </div>

      {/* ---------- 解绑两步确认弹窗（镜像原型，danger；Esc/scrim 可关） ---------- */}
      <div className={`scrim ${unbindTarget ? "on" : ""}`} onClick={() => setUnbindTarget(null)} />
      <div className={`modal ${unbindTarget ? "on" : ""}`} role="dialog" aria-modal="true" aria-labelledby="wx-unbind-title">
        <div className="drawer-head">
          <div className="card-title" id="wx-unbind-title">
            解绑{unbindTarget ? channelLabel(unbindTarget.channel) : ""}会话？
          </div>
          <button className="icon-btn" aria-label="关闭" onClick={() => setUnbindTarget(null)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>
        <div className="card-body">
          <p style={{ fontSize: 13, lineHeight: 1.7, color: "var(--fg-2)" }}>
            解绑后不再向该会话推送预警与盘后摘要，来消息恢复静默丢弃；网页端功能与持仓数据不受影响。
          </p>
          <p className="hint" style={{ marginTop: 12 }}>
            随时可以重新绑定（绑定关系存服务器 zimu.db，解绑置 active=0 留痕不删行）。
          </p>
        </div>
        <div className="drawer-foot" style={{ justifyContent: "flex-end" }}>
          <button className="btn" onClick={() => setUnbindTarget(null)}>取消</button>
          <button className="btn btn-danger" onClick={() => void doUnbind()}>确认解绑</button>
        </div>
      </div>
    </main>
  );
}