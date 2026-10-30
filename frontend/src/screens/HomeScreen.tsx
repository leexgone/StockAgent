import { Link } from "react-router-dom";
import {
  IconChat,
  IconChevronRight,
  IconGrid,
  IconShield,
  IconSpark,
  IconWallet,
  IconWechat,
} from "../components/Icons";
import { perf } from "../domain";
import { clockView, marketClock } from "../lib/clock";
import { dirClass, fixed, fmt, money, pct } from "../lib/format";
import { coveredCount, useAdvice } from "../store/advice";
import { useAlerts } from "../store/alerts";
import { useEquity } from "../store/equity";
import { useHealth } from "../store/health";
import { useMarket } from "../store/market";
import { usePortfolio } from "../store/portfolio";
import { usePushLog } from "../store/push_log";

/**
 * 工作台（index 屏，P5-T2 真实数据移植）—— 对照基准 design/index.html。
 * 数字全部来自后端 API（recalc 汇总 / 预警 / 快照 / 建议留档 / 推送留痕），
 * 前端不重算派生值（ADR-0008）；回撤走 src/domain 的 perf（黄金向量同款纯函数，
 * 序列长度 <2 时安全返回 0）。摘要句与指标卡从真实 payload 现算——
 * 「不写死数字」约定的样板位置；空持仓态按 docs/02 index 条目处理。
 */

interface HomeScreenProps {
  now: Date;
}

/* push_log.kind → 时间线文案（封闭词汇随渠道落地扩展，03 §12） */
const KIND_LABEL: Record<string, string> = {
  post_market_review: "盘后复盘",
  alert_push: "预警推送",
};

/* 待决事项排序：风险（down）> 动作提醒（warn）> 信息（up）；同级新在前 */
const LEVEL_RANK: Record<string, number> = { down: 0, warn: 1, up: 2 };
/* level → pill 语义类（好坏轴 .ok/.warn/.danger——涨跌方向色不进 pill，03 §11） */
const LEVEL_PILL: Record<string, string> = { down: "danger", warn: "warn", up: "ok" };

interface AttentionItem {
  t: string;
  d: string;
  pill: string;
  cls: string;
  to: string;
}

export default function HomeScreen({ now }: HomeScreenProps) {
  const view = usePortfolio((s) => s.view);
  const pError = usePortfolio((s) => s.error);
  const loading = usePortfolio((s) => s.loading);
  const alertRows = useAlerts((s) => s.rows);
  const equityRows = useEquity((s) => s.rows);
  const adviceRows = useAdvice((s) => s.rows);
  const pushRows = usePushLog((s) => s.rows);
  const health = useHealth((s) => s.health);
  const status = useMarket((s) => s.status);

  const clock = status ? clockView(status.clock, now) : marketClock(now);
  const h = now.getHours();
  const hello = h < 11 ? "早上好" : h < 14 ? "中午好" : h < 18 ? "下午好" : "晚上好";
  const dateLine =
    now.toLocaleDateString("zh-CN", { year: "numeric", month: "long", day: "numeric", weekday: "long" }) +
    " · " +
    clock.state;

  const agg = view?.agg;
  const holdings = view?.holdings ?? [];
  const per = view?.per ?? [];
  const isEmpty = view !== null && holdings.length === 0;
  const pend = alertRows.filter((a) => !a.done);

  /* 摘要句素材：仓位最重标的 + 浮亏最深标的（数字全部来自后端 per 行） */
  const paired = holdings
    .map((hd, i) => ({ name: hd.name, p: per[i] }))
    .filter((x): x is { name: string; p: NonNullable<typeof x.p> } => x.p !== undefined);
  const topW = paired.reduce<{ name: string; weight: number } | null>(
    (m, x) => (!m || x.p.weight > m.weight ? { name: x.name, weight: x.p.weight } : m),
    null,
  );
  const worst = paired.reduce<{ name: string; pnlPct: number } | null>(
    (m, x) => (!m || x.p.pnl_pct < m.pnlPct ? { name: x.name, pnlPct: x.p.pnl_pct } : m),
    null,
  );

  const desc = !view
    ? pError
      ? `后端不可用：${pError}。启动 zimu-server（cargo run -p zimu-server）后点顶栏刷新重试。`
      : loading
        ? "正在读取组合与行情…"
        : "尚未连接后端。"
    : isEmpty
      ? `组合还是空的——总资产 = 现金 ${money(agg?.cash ?? 0)}。先录入第一笔交易，或去问答页让子木带你上手。`
      : `${clock.live ? "现在" + clock.state : clock.state}。${pend.length} 条预警待处理` +
        (topW ? `，${topW.name} 占资产 ${topW.weight.toFixed(1)}%` : "") +
        (worst && worst.pnlPct < 0 ? `，${worst.name} 浮亏 ${pct(worst.pnlPct)}` : "") +
        "。";

  /* 回撤：真实净值序列走 domain perf（03 §7）；快照 <2 天不展示（不伪装 0=安全） */
  const mdd =
    equityRows.length >= 2
      ? perf({ nav: equityRows.map((r) => r.total_assets), bench: [] }).mdd
      : null;
  const covered = coveredCount(adviceRows);

  /* 六张界面入口卡（stat 全部现算自真实数据；模板与原型 SCREENS 逐一对应） */
  const screens: { path: string; Icon: () => React.JSX.Element; name: string; desc: string; stat: string }[] = [
    {
      path: "/dashboard", Icon: IconGrid, name: "总览仪表盘",
      desc: "资产曲线、当日盈亏归因、待处理预警与自选行情，一屏看完今天。",
      stat: `${equityRows.length} 日净值 · ${pend.length} 条待处理预警`,
    },
    {
      path: "/holdings", Icon: IconWallet, name: "持仓管理",
      desc: "股票 / ETF / 可转债分组明细，加减仓会实时重算摊薄成本与仓位。",
      stat: !view ? "—" : isEmpty ? `现金 ${money(agg?.cash ?? 0)} · 待录入第一笔` : `${holdings.length} 只标的 · ${money(agg?.cash ?? 0)} 现金`,
    },
    {
      path: "/analysis", Icon: IconSpark, name: "AI 分析建议",
      desc: "买入 / 持有 / 卖出结论，拆到技术、基本面、资金、消息四个维度并给出目标与止损。",
      stat: covered > 0 ? `已覆盖 ${covered} 只标的` : "暂无留档 · 盘后复盘自动生成",
    },
    {
      path: "/chat", Icon: IconChat, name: "智能问答",
      desc: "用大白话问股，回答会引用你的持仓、行情与公告原文。",
      stat: health ? (health.agent_online ? "Agent 在线 · 流式回答" : "Agent 离线 · 检查 robit 配置") : "连接中…",
    },
    {
      path: "/diagnosis", Icon: IconShield, name: "组合诊断",
      desc: "行业集中度、波动率、最大回撤，以及和沪深 300 的滚动对比与再平衡清单。",
      stat: mdd !== null && agg ? `HHI ${agg.hhi.toFixed(3)} · 回撤 ${fixed(mdd, 1)}%` : "待净值积累 · 每交易日 15:05 快照",
    },
    {
      path: "/wechat", Icon: IconWechat, name: "微信助理接入",
      desc: "绑定微信后，盘中预警、收盘复盘与建议确认都送到手机上。",
      stat: "渠道待接入 · C4 待拍板",
    },
  ];

  /* 助理最近的动作 = push_log 留痕（新在前取 5；ok=1 只表示已受理，03 §12） */
  const timeline = pushRows.slice(0, 5).map((r) => ({
    t: r.ts,
    who: r.channel === "agent" ? "定时任务" : r.channel,
    text: `${KIND_LABEL[r.kind] ?? r.kind} · ${r.ok ? "已受理" : "受理失败"}`,
    reply: r.ok ? undefined : r.detail,
    read: r.ok,
  }));

  /* 今天需要你决定 = 待处理预警按紧迫度取 3；空态给引导（docs/02 index 条目） */
  const attention: AttentionItem[] = pend.length
    ? [...pend]
        .sort((a, b) => (LEVEL_RANK[a.level] ?? 9) - (LEVEL_RANK[b.level] ?? 9) || b.ts.localeCompare(a.ts))
        .slice(0, 3)
        .map((a) => ({
          t: `${a.name}（${a.code}）`,
          d: a.text,
          pill: a.type,
          cls: LEVEL_PILL[a.level] ?? "warn",
          to: `/analysis?code=${a.code}`,
        }))
    : isEmpty
      ? [{ t: "录入第一笔持仓", d: "持仓页抽屉逐笔录入，自动校验整手、涨跌停与现金余额", pill: "引导", cls: "ok", to: "/holdings?add=1" }]
      : view
        ? [{ t: "今天没有待决事项", d: "盘中预警触发后会在这里按紧迫度汇总", pill: "正常", cls: "ok", to: "/dashboard" }]
        : [];

  return (
    <main className="page" id="main" data-od-id="home-page">
      <div className="page-head">
        <div>
          <div className="eyebrow" id="greet-eyebrow">{dateLine}</div>
          <h1 className="page-title" id="greet-title">{hello}</h1>
          <p className="page-desc" id="greet-desc">{desc}</p>
        </div>
        <div className="page-actions">
          <Link className="btn" to="/chat">问一句</Link>
          {isEmpty ? (
            <Link className="btn btn-primary" to="/holdings?add=1">录入第一笔</Link>
          ) : (
            <Link className="btn btn-primary" to="/dashboard">进入总览仪表盘</Link>
          )}
        </div>
      </div>

      {/* 后端不可用：错误横幅 + 重试（旧数字不伪装，宁缺不假） */}
      {!view && (
        <section className="card" data-od-id="home-error" style={{ marginBottom: 16 }}>
          <div className="card-body">
            <div className="metric-label">后端连接</div>
            <p style={{ margin: "6px 0 0", fontSize: 13, color: "var(--muted)" }}>
              {pError ?? (loading ? "正在连接 zimu-server…" : "尚未加载。")}
            </p>
            {pError && (
              <button
                className="btn btn-sm"
                style={{ marginTop: 10 }}
                onClick={() => void usePortfolio.getState().load()}
              >
                重试
              </button>
            )}
          </div>
        </section>
      )}

      {/* 今日快照（数字 = 后端 recalc 汇总原值；未加载显示 "—"） */}
      <section className="card" data-od-id="home-snapshot" style={{ marginBottom: 16 }}>
        <div
          className="card-body"
          style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit,minmax(150px,1fr))", gap: "22px 26px" }}
        >
          <div data-od-id="home-snapshot-assets">
            <div className="metric-label">总资产（含现金）</div>
            <div className="metric-value num" id="s-assets">{agg ? money(agg.total_assets) : "—"}</div>
            <div className="metric-meta">
              <span className="muted">
                {agg ? `持仓市值 ${money(agg.total_mv)} · 仓位 ${agg.exposure.toFixed(1)}%` : "待加载"}
              </span>
            </div>
          </div>
          <div data-od-id="home-snapshot-day">
            <div className="metric-label">当日盈亏</div>
            <div className={`metric-value num ${agg ? dirClass(agg.day_pnl) : ""}`} id="s-day">
              {agg ? (agg.day_pnl > 0 ? "+" : "") + fmt(agg.day_pnl, 2) : "—"}
            </div>
            <div className="metric-meta">
              {agg && <span className={`delta ${dirClass(agg.day_pnl)}`}>{pct(agg.day_pnl_pct)} 相对昨日</span>}
            </div>
          </div>
          <div data-od-id="home-snapshot-total">
            <div className="metric-label">累计盈亏</div>
            <div className={`metric-value num ${agg ? dirClass(agg.total_pnl) : ""}`} id="s-total">
              {agg ? (agg.total_pnl > 0 ? "+" : "") + fmt(agg.total_pnl, 0) : "—"}
            </div>
            <div className="metric-meta">
              {agg && (
                <>
                  <span className={`delta ${dirClass(agg.total_pnl)}`}>{pct(agg.total_pnl_pct)}</span>{" "}
                  <span>投入成本 {money(agg.cost_total)}</span>
                </>
              )}
            </div>
          </div>
          <div data-od-id="home-snapshot-alerts">
            <div className="metric-label">待处理预警</div>
            <div className="metric-value num" id="s-alerts">{pend.length}</div>
            <div className="metric-meta">
              <Link to="/dashboard" style={{ textDecoration: "underline", textUnderlineOffset: 2 }}>
                查看清单
              </Link>
            </div>
          </div>
        </div>
      </section>

      {/* 六个界面 */}
      <h2 style={{ fontSize: 15, margin: "26px 0 12px" }} data-od-id="home-screens-title">
        界面导航
      </h2>
      <div className="grid g-even" id="screens" data-od-id="home-screens">
        {screens.map((s, i) => (
          <Link
            key={s.name}
            className="card"
            to={s.path}
            data-od-id={`screen-card-${i}`}
            style={{ display: "block", padding: 18, transition: "border-color .14s, box-shadow .14s" }}
            onMouseEnter={(e) => {
              e.currentTarget.style.borderColor = "var(--accent-line)";
              e.currentTarget.style.boxShadow = "var(--shadow-md)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.borderColor = "var(--border)";
              e.currentTarget.style.boxShadow = "var(--shadow-sm)";
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span className="ic">
                <s.Icon />
              </span>
              <span style={{ fontFamily: "var(--font-display)", fontSize: 15, fontWeight: 590, letterSpacing: "-.015em" }}>
                {s.name}
              </span>
              <span style={{ marginLeft: "auto", color: "var(--muted)", display: "grid", placeItems: "center" }}>
                <IconChevronRight />
              </span>
            </div>
            <p style={{ marginTop: 11, fontSize: 12.5, lineHeight: 1.6, color: "var(--muted)" }}>{s.desc}</p>
            <p className="num" style={{ marginTop: 12, fontSize: 11.5, color: "var(--fg-2)", letterSpacing: 0 }}>
              {s.stat}
            </p>
          </Link>
        ))}
      </div>

      {/* 助理最近的动作 / 今天需要你决定 */}
      <div className="grid g-2" style={{ marginTop: 16 }}>
        <section className="card" data-od-id="home-timeline">
          <div className="card-head">
            <div>
              <div className="card-title">助理最近的动作</div>
              <div className="card-sub">盘后复盘 · 预警推送 · 留痕即事实（ok=已受理）</div>
            </div>
            <div className="head-actions">
              <Link className="btn btn-sm" to="/wechat">
                接入设置
              </Link>
            </div>
          </div>
          <div className="card-body">
            {timeline.length === 0 ? (
              <p style={{ margin: 0, fontSize: 12.5, lineHeight: 1.7, color: "var(--muted)" }}>
                还没有助理动作。交易日 15:05 盘后复盘自动触发并在这里留痕；
                预警推送随微信渠道（C4 拍板后）接入。
              </p>
            ) : (
              <div className="tl" id="home-tl">
                {timeline.map((s, i) => (
                  <div className={`tl-item${s.read ? "" : " hot"}`} key={i}>
                    <div className="tl-time">
                      {s.t} · {s.who}
                    </div>
                    <div className="tl-title">{s.text}</div>
                    {s.reply && <div className="tl-body">{s.reply}</div>}
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>

        <section className="card" data-od-id="home-attention">
          <div className="card-head">
            <div>
              <div className="card-title">今天需要你决定</div>
              <div className="card-sub">按紧迫度排序{attention.length > 0 ? `，共 ${attention.length} 件` : ""}</div>
            </div>
          </div>
          <div className="list" id="home-attention">
            {attention.map((a, i) => (
              <Link className="list-item" to={a.to} data-od-id={`attention-item-${i}`} key={i}>
                <span className="li-main">
                  <span className="li-title">{a.t}</span>
                  <span className="li-desc">{a.d}</span>
                </span>
                <span className="li-right">
                  <span className={`pill ${a.cls}`}>{a.pill}</span>
                </span>
              </Link>
            ))}
          </div>
        </section>
      </div>

      <div className="footnote" data-od-id="home-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          行情来自公开数据源（可能有延迟或错漏）；所有「买入 / 持有 / 卖出」结论由规则打分 + 模型文案生成（ADR-0007），
          <b>不构成投资建议</b>；程序不连接券商接口、不代下单，每一笔委托仍需你本人确认。
        </span>
      </div>
    </main>
  );
}
