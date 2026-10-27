import {
  P,
  agg,
  adviceCodes,
  alerts,
  dirClass,
  equity,
  fmt,
  holdings,
  marketClock,
  money,
  pct,
  qa,
  sessions,
} from "../data/demo";
import type { View } from "../data/demo";
import {
  IconChat,
  IconChevronRight,
  IconGrid,
  IconShield,
  IconSpark,
  IconWallet,
  IconWechat,
} from "../components/Icons";

/**
 * 工作台（index 屏静态复刻，P2-T4）—— 对照基准 design/index.html。
 * 摘要句与指标卡的数字全部现算自 demo.ts 的派生快照（不写死字面量），
 * 问候语与市场状态随真实时钟每秒重绘（与原型 renderHead + marketClock 同口径）。
 */

interface HomeScreenProps {
  now: Date;
  onNavigate: (v: View) => void;
}

/* 界面入口卡（stat 与原型 SCREENS 的模板逐一对应） */
const SCREENS: {
  view: View | null;
  Icon: () => React.JSX.Element;
  name: string;
  desc: string;
  stat: string;
}[] = [
  { view: null, Icon: IconGrid, name: "总览仪表盘", desc: "资产曲线、当日盈亏归因、待处理预警与自选行情，一屏看完今天。", stat: `${equity.dates.length} 日净值 · ${alerts.length} 条预警` },
  { view: null, Icon: IconWallet, name: "持仓管理", desc: "股票 / ETF / 可转债分组明细，加减仓会实时重算摊薄成本与仓位。", stat: `${holdings.length} 只标的 · ${money(agg.cash)} 现金` },
  { view: null, Icon: IconSpark, name: "AI 分析建议", desc: "买入 / 持有 / 卖出结论，拆到技术、基本面、资金、消息四个维度并给出目标与止损。", stat: `已覆盖 ${adviceCodes.length} 只标的` },
  { view: "chat", Icon: IconChat, name: "智能问答", desc: "用大白话问股，回答会引用你的持仓、行情与公告原文。", stat: `${qa.length} 个示例问题 · 3 段历史会话` },
  { view: null, Icon: IconShield, name: "组合诊断", desc: "行业集中度、波动率、最大回撤，以及和沪深 300 的滚动对比与再平衡清单。", stat: `HHI ${agg.hhi.toFixed(3)} · 回撤 ${P.mdd.toFixed(1)}%` },
  { view: null, Icon: IconWechat, name: "微信助理接入", desc: "绑定微信后，盘中预警、收盘复盘与建议确认都送到手机上。", stat: "1 个微信账号 · 6 类推送" },
];

/* 待决事项（与原型 ATT 数组同款；pill 用语义类：交易方向 .buy/.sell、
   好坏轴 .ok/.warn/.danger——B5 语义分离已随 P1-T4 两边同步，up/down 只留给涨跌数字与标记） */
const ATTENTION = [
  { t: "把比亚迪仓位降到 15% 附近", d: "减 200 股（整手）即可，不必拆很多笔", pill: "减仓", cls: "sell" },
  { t: "科创50ETF 触及 +12% 止盈线", d: "可分批止盈 3,000 份，或把止盈线上移到 +18%", pill: "止盈", cls: "warn" },
  { t: "11-20 前处理博威转债", d: "强赎最后交易日前卖出或转股，逾期将承担额外损失", pill: "限期", cls: "danger" },
];

/* 时间线 = 最近会话 + 一条盘前任务（与原型 concat 逻辑一致） */
const TIMELINE = sessions
  .concat([
    { t: "今日 08:30", who: "定时任务", text: "盘前要闻 6 条已生成，其中 1 条与你持仓相关", reply: "博威转债强赎提示", read: true },
  ])
  .slice(0, 5);

export default function HomeScreen({ now, onNavigate }: HomeScreenProps) {
  const clock = marketClock(now);
  const h = now.getHours();
  const hello = h < 11 ? "早上好" : h < 14 ? "中午好" : h < 18 ? "下午好" : "晚上好";
  const dateLine =
    now.toLocaleDateString("zh-CN", { year: "numeric", month: "long", day: "numeric", weekday: "long" }) +
    " · " +
    clock.state;

  const pend = alerts.filter((a) => !a.done);
  const byd = holdings.find((x) => x.code === "002594")!;
  const maxWeight = holdings.reduce((m, x) => Math.max(m, x.weight), 0);
  const desc =
    `${clock.state === "已收盘" ? "今日已收盘" : clock.live ? "现在" + clock.state : clock.next}。` +
    `${pend.length} 条预警待处理，${byd.name} 浮亏扩大至 ${pct(byd.pnlPct)}，` +
    `组合最大单票占资产 ${maxWeight.toFixed(1)}%。`;

  const goChat = (e: React.MouseEvent) => {
    e.preventDefault();
    onNavigate("chat");
  };
  const inert = (e: React.MouseEvent) => e.preventDefault();

  return (
    <main className="page" id="main" data-od-id="home-page">
      <div className="page-head">
        <div>
          <div className="eyebrow" id="greet-eyebrow">{dateLine}</div>
          <h1 className="page-title" id="greet-title">{hello}</h1>
          <p className="page-desc" id="greet-desc">{desc}</p>
        </div>
        <div className="page-actions">
          <a className="btn" href="#" onClick={goChat}>问一句</a>
          <a className="btn btn-primary" href="#" onClick={inert} title="P5 移植中，暂未接入">
            进入总览仪表盘
          </a>
        </div>
      </div>

      {/* 今日快照 */}
      <section className="card" data-od-id="home-snapshot" style={{ marginBottom: 16 }}>
        <div
          className="card-body"
          style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit,minmax(150px,1fr))", gap: "22px 26px" }}
        >
          <div data-od-id="home-snapshot-assets">
            <div className="metric-label">总资产（含现金）</div>
            <div className="metric-value num" id="s-assets">{money(agg.totalAssets)}</div>
            <div className="metric-meta">
              <span className="muted">
                持仓市值 {money(agg.totalMv)} · 仓位 {agg.exposure.toFixed(1)}%
              </span>
            </div>
          </div>
          <div data-od-id="home-snapshot-day">
            <div className="metric-label">当日盈亏</div>
            <div className={`metric-value num ${dirClass(agg.dayPnl)}`} id="s-day">
              {(agg.dayPnl > 0 ? "+" : "") + fmt(agg.dayPnl, 2)}
            </div>
            <div className="metric-meta">
              <span className={`delta ${dirClass(agg.dayPnl)}`}>{pct(agg.dayPnlPct)} 相对昨日</span>
            </div>
          </div>
          <div data-od-id="home-snapshot-total">
            <div className="metric-label">累计盈亏</div>
            <div className={`metric-value num ${dirClass(agg.totalPnl)}`} id="s-total">
              {(agg.totalPnl > 0 ? "+" : "") + fmt(agg.totalPnl, 0)}
            </div>
            <div className="metric-meta">
              <span className={`delta ${dirClass(agg.totalPnl)}`}>{pct(agg.totalPnlPct)}</span>{" "}
              <span>投入成本 {money(agg.costTotal)}</span>
            </div>
          </div>
          <div data-od-id="home-snapshot-alerts">
            <div className="metric-label">待处理预警</div>
            <div className="metric-value num" id="s-alerts">{pend.length}</div>
            <div className="metric-meta">
              <a href="#" onClick={inert} title="P5 移植中，暂未接入" style={{ textDecoration: "underline", textUnderlineOffset: 2 }}>
                查看清单
              </a>
            </div>
          </div>
        </div>
      </section>

      {/* 六个界面 */}
      <h2 style={{ fontSize: 15, margin: "26px 0 12px" }} data-od-id="home-screens-title">
        界面导航
      </h2>
      <div className="grid g-even" id="screens" data-od-id="home-screens">
        {SCREENS.map((s, i) => (
          <a
            key={s.name}
            className="card"
            href="#"
            data-od-id={`screen-card-${i}`}
            title={s.view ? undefined : "P5 移植中，暂未接入"}
            style={{ display: "block", padding: 18, transition: "border-color .14s, box-shadow .14s" }}
            onClick={s.view ? goChat : inert}
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
          </a>
        ))}
      </div>

      {/* 助理最近的动作 / 今天需要你决定 */}
      <div className="grid g-2" style={{ marginTop: 16 }}>
        <section className="card" data-od-id="home-timeline">
          <div className="card-head">
            <div>
              <div className="card-title">助理最近的动作</div>
              <div className="card-sub">盘中监控 · 盘后复盘 · 微信应答</div>
            </div>
            <div className="head-actions">
              <a className="btn btn-sm" href="#" onClick={inert} title="P5 移植中，暂未接入">
                接入设置
              </a>
            </div>
          </div>
          <div className="card-body">
            <div className="tl" id="home-tl">
              {TIMELINE.map((s, i) => (
                <div className={`tl-item${s.read ? "" : " hot"}`} key={i}>
                  <div className="tl-time">
                    {s.t} · {s.who}
                  </div>
                  <div className="tl-title">{s.text}</div>
                  {s.reply && <div className="tl-body">{s.reply}</div>}
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="card" data-od-id="home-attention">
          <div className="card-head">
            <div>
              <div className="card-title">今天需要你决定</div>
              <div className="card-sub">按紧迫度排序，只有 3 件</div>
            </div>
          </div>
          <div className="list" id="home-attention">
            {ATTENTION.map((a, i) => (
              <a className="list-item" href="#" onClick={inert} title="P5 移植中，暂未接入" data-od-id={`attention-item-${i}`} key={i}>
                <span className="li-main">
                  <span className="li-title">{a.t}</span>
                  <span className="li-desc">{a.d}</span>
                </span>
                <span className="li-right">
                  <span className={`pill ${a.cls}`}>{a.pill}</span>
                </span>
              </a>
            ))}
          </div>
        </section>
      </div>

      <div className="footnote" data-od-id="home-disclaimer">
        <span style={{ color: "var(--muted)" }}>i</span>
        <span>
          本页行情与持仓均为<b>演示样本</b>，用于验证界面与信息架构。所有「买入 / 持有 / 卖出」结论由模型基于样本数据生成，
          <b>不构成投资建议</b>；接入真实行情后仍需你本人确认每一笔委托。
        </span>
      </div>
    </main>
  );
}
