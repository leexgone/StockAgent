import { useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { NAV, WECHAT_NAV, agg, dirClass, fmt, indices, marketClock, money } from "../data/demo";
import { routeFor } from "../routes";
import { ICONS, IconBell, IconFilter, IconRefresh, IconSearch, IconSpark } from "./Icons";

/**
 * 共享外壳 —— 复刻 design/assets/data.js 的 chrome()（侧栏 / 顶栏 / 行情条 / 移动底栏）。
 * P5-T1：导航改 react-router 路由（单一来源 routes.ts），未移植屏渲染占位屏，
 * 面包屑与高亮由 pathname 派生。
 * 与原型的行为差异（静态复刻边界）：
 * - 顶栏刷新只重对时钟，不做价格跳动（tickPrices 属原型演示逻辑，真实行情
 *   随 P5-T2 接 /api/market/refresh）。
 * - 铃铛 toast、全局搜索回车、数字键/⌘K 快捷键未移植。
 */

interface ChromeProps {
  now: Date;
  onRefresh: () => void;
  children: React.ReactNode;
}

export default function Chrome({ now, onRefresh, children }: ChromeProps) {
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const { pathname } = useLocation();
  const clock = marketClock(now);
  const crumb = routeFor(pathname)?.crumb ?? "子木";
  const close = () => setSidebarOpen(false);

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
              <span className="side-acct-av">我</span>
              <span className="side-acct-text">
                <span className="side-acct-name">我的账户</span>
                <span className="side-acct-sub">华泰证券 · 沪深 A 股账户（*8842）</span>
              </span>
            </div>
            <div className="side-note">数据为演示样本，建议由模型生成，不构成投资意见。</div>
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
            <div className="mkt" id="mkt-clock">
              <span className={`dot ${clock.dot}`} />
              <span>{clock.state}</span>
              <span className="mkt-time">{clock.time}</span>
              <span className="mkt-next">{clock.next}</span>
            </div>
            <button className="icon-btn" aria-label="预警" title="待处理预警汇总（toast 未移植）">
              <IconBell />
            </button>
            <button className="icon-btn" aria-label="刷新行情" onClick={onRefresh} title="重新对时（价格跳动 P5-T2 接真实行情后启用）">
              <IconRefresh />
            </button>
          </header>

          <div className="strip" id="strip">
            {indices.map((i) => (
              <span className="strip-item" key={i.code}>
                <span className="strip-name">{i.name}</span>
                <span className={`strip-v ${dirClass(i.chg)}`}>{fmt(i.v, 2)}</span>
                <span className={`delta ${dirClass(i.chg)}`}>
                  {(i.chg > 0 ? "+" : "") + i.chg.toFixed(2)}%
                </span>
              </span>
            ))}
            <span className="strip-item" style={{ borderRight: 0 }}>
              <span className="strip-name">我的总资产</span>
              <span className="strip-v">{money(agg.totalAssets)}</span>
              <span className={`delta ${dirClass(agg.dayPnl)}`}>
                今日 {(agg.dayPnl > 0 ? "+" : "") + fmt(agg.dayPnl, 0)}
              </span>
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
