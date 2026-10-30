import { useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { clockView, marketClock } from "../lib/clock";
import { dirClass, fixed, fmt, money } from "../lib/format";
import { NAV, WECHAT_NAV, routeFor } from "../routes";
import { pending, useAlerts } from "../store/alerts";
import { INDEX_SYMBOLS, useMarket } from "../store/market";
import { usePortfolio } from "../store/portfolio";
import { useUi } from "../store/ui";
import { ICONS, IconBell, IconFilter, IconRefresh, IconSearch, IconSpark } from "./Icons";

/**
 * 共享外壳 —— 复刻 design/assets/data.js 的 chrome()（侧栏 / 顶栏 / 行情条 / 移动底栏）。
 * P5-T2 接真实数据：
 * - 市场时钟 = 后端 03 §9 生产口径（/api/market/status），倒计时本地走针；
 *   后端不可用退本地启发式（仅展示降级，不作交易判定）。
 * - 行情条指数 = /api/market/quote 实时行（sidecar 挂 → 显示 "—"，不瘫）；
 *   总资产/今日盈亏 = 组合 store 的 recalc 汇总（后端唯一真值）。
 * - 铃铛 = toast 汇总待处理预警（原型同款）；刷新按钮触发 onRefresh（App 接线
 *   POST /api/market/refresh + 指数/时钟/预警重拉）。
 * 未移植：全局搜索回车、数字键/⌘K 快捷键（T2 后段）。
 */

interface ChromeProps {
  now: Date;
  onRefresh: () => void;
  children: React.ReactNode;
}

export default function Chrome({ now, onRefresh, children }: ChromeProps) {
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const { pathname } = useLocation();
  const status = useMarket((s) => s.status);
  const quotes = useMarket((s) => s.quotes);
  const quotesError = useMarket((s) => s.quotesError);
  const view = usePortfolio((s) => s.view);
  const crumb = routeFor(pathname)?.crumb ?? "子木";
  const clock = status ? clockView(status.clock, now) : marketClock(now);
  const agg = view?.agg;
  const account = view?.account;
  const close = () => setSidebarOpen(false);

  const bellToast = () => {
    const pend = pending(useAlerts.getState());
    useUi
      .getState()
      .pushToast(
        pend.length
          ? `${pend.length} 条待处理预警 · 最近：${pend[0].text}`
          : "没有待处理预警",
      );
  };

  const navItem = (item: (typeof NAV)[number], key: string) => {
    const Icon = ICONS[item.icon];
    const current = item.path === pathname;
    return (
      <Link
        key={key}
        className="nav-item"
        to={item.path}
        aria-current={current ? "page" : undefined}
        onClick={close}
      >
        <Icon />
        <span>{item.label}</span>
        {item.kbd && <span className="kbd">{item.kbd}</span>}
      </Link>
    );
  };

  return (
    <>
      <div className="shell">
        <aside className={`sidebar${sidebarOpen ? " on" : ""}`} id="sidebar">
          <Link className="brand" to="/" onClick={close}>
            <span className="brand-mark">
              <IconSpark />
            </span>
            <span>
              <span className="brand-name">子木 · 股票智能助理</span>
              <span className="brand-sub">个人投研工作台</span>
            </span>
          </Link>
          <div className="nav-group">工作台</div>
          {NAV.map((n) => navItem(n, n.label))}
          {navItem(WECHAT_NAV, WECHAT_NAV.label)}
          <div className="nav-group">快捷</div>
          <Link className="nav-item" to="/chat" onClick={close}>
            <ICONS.chat />
            <span>问一句</span>
            <span className="kbd">⌘K</span>
          </Link>
          <div className="side-foot">
            <div className="side-acct">
              <span className="side-acct-av">{account?.init ?? "我"}</span>
              <span className="side-acct-text">
                <span className="side-acct-name">{account?.name ?? "我的账户"}</span>
                <span className="side-acct-sub">{account?.broker ?? "未连接后端"}</span>
              </span>
            </div>
            <div className="side-note">行情来自公开数据源，可能延迟；建议由模型生成，不构成投资意见。</div>
          </div>
        </aside>

        <div className="main">
          <header className="topbar" id="topbar">
            <button
              className="icon-btn menu-toggle"
              aria-label="打开导航"
              onClick={() => setSidebarOpen((v) => !v)}
            >
              <IconFilter />
            </button>
            <span className="crumb">
              <b>{crumb}</b>
            </span>
            <label className="search">
              <IconSearch />
              <input id="global-search" placeholder="搜代码 / 名称 / 问一句" autoComplete="off" />
              <span className="kbd">⌘K</span>
            </label>
            <div className="mkt" id="mkt-clock" title={quotesError ?? undefined}>
              <span className={`dot ${clock.dot}`} />
              <span>{clock.state}</span>
              <span className="mkt-time">{clock.time}</span>
              <span className="mkt-next">{clock.next}</span>
            </div>
            <button className="icon-btn" aria-label="预警" title="待处理预警汇总" onClick={bellToast}>
              <IconBell />
            </button>
            <button
              className="icon-btn"
              aria-label="刷新行情"
              onClick={onRefresh}
              title="拉最新行情 → 回写 → recalc 重算（POST /api/market/refresh）"
            >
              <IconRefresh />
            </button>
          </header>

          <div className="strip" id="strip">
            {INDEX_SYMBOLS.map((ix) => {
              const q = quotes[ix.symbol];
              // 只有 status=ok 且 price>0 才算有数据：spot 源对个别指数回空行
              // （如中证转债 sh000832 → halted_or_delisted，07-backlog B14），显示 "—" 不显示 0
              const usable = q !== undefined && q.status === "ok" && (q.price ?? 0) > 0;
              const price = usable ? q.price ?? null : null;
              const chg = usable ? q.chg_pct ?? null : null;
              return (
                <span className="strip-item" key={ix.symbol}>
                  <span className="strip-name">{q?.name ?? ix.name}</span>
                  <span className={`strip-v ${chg !== null ? dirClass(chg) : "muted"}`}>
                    {price !== null ? fmt(price, 2) : "—"}
                  </span>
                  {chg !== null && (
                    <span className={`delta ${dirClass(chg)}`}>
                      {(chg > 0 ? "+" : "") + fixed(chg, 2)}%
                    </span>
                  )}
                </span>
              );
            })}
            <span className="strip-item" style={{ borderRight: 0 }}>
              <span className="strip-name">我的总资产</span>
              <span className="strip-v">{agg ? money(agg.total_assets) : "—"}</span>
              {agg && (
                <span className={`delta ${dirClass(agg.day_pnl)}`}>
                  今日 {(agg.day_pnl > 0 ? "+" : "") + fmt(agg.day_pnl, 0)}
                </span>
              )}
            </span>
          </div>

          {children}
        </div>
      </div>

      <nav className="mtabs" id="mtabs">
        {NAV.concat([WECHAT_NAV]).map((n) => {
          const Icon = ICONS[n.icon];
          const current = n.path === pathname;
          return (
            <Link
              key={n.label}
              className="mtab"
              to={n.path}
              aria-current={current ? "page" : undefined}
              onClick={close}
            >
              <Icon />
              <span>{n.short}</span>
            </Link>
          );
        })}
      </nav>
    </>
  );
}
