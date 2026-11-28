import { useEffect, useRef, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { clockView, marketClock } from "../lib/clock";
import { dirClass, fixed, fmt, money } from "../lib/format";
import { NAV, WECHAT_NAV, routeFor } from "../routes";
import { pending, useAlerts } from "../store/alerts";
import { INDEX_SYMBOLS, useMarket } from "../store/market";
import { usePortfolio } from "../store/portfolio";
import { useUi } from "../store/ui";
import { useWatchlist } from "../store/watchlist";
import { ICONS, IconBell, IconFilter, IconRefresh, IconSearch, IconSpark } from "./Icons";

/**
 * 共享外壳 —— 复刻 design/assets/data.js 的 chrome()（侧栏 / 顶栏 / 行情条 / 移动底栏）。
 * 布局形态（2026-11-28 双分区改造）：`.main` 恰满视口高度，顶栏 + 行情条是**不滚动的
 * 固定区**，children 包在 `.content` 独立滚动区里——滚动条只覆盖内容区，头部不浮动
 * 其上（sticky 方案废除）；路由切换把内容区滚回顶部。智能问答的 page-fill 全高链
 * 以 .content 为新链首（chat.css）。
 * P5-T2 接真实数据：
 * - 市场时钟 = 后端 03 §9 生产口径（/api/market/status），倒计时本地走针；
 *   后端不可用退本地启发式（仅展示降级，不作交易判定）。
 * - 行情条指数 = /api/market/quote 实时行（sidecar 挂 → 显示 "—"，不瘫）；
 *   总资产/今日盈亏 = 组合 store 的 recalc 汇总（后端唯一真值）。
 * - 铃铛 = toast 汇总待处理预警（原型同款）；刷新按钮触发 onRefresh（App 接线
 *   POST /api/market/refresh + 指数/时钟/预警重拉）。
 * - 全局导航（docs/02 表，P5-T2 收尾补齐）：搜索回车命中持仓/自选 → /analysis?code=，
 *   否则 → /chat?q=（规格查持仓+自选，比原型只查持仓更完整）；数字键 1–6 切页
 *   （输入框内不拦截）；⌘K / Ctrl+K 聚焦搜索。
 */

interface ChromeProps {
  now: Date;
  onRefresh: () => void;
  children: React.ReactNode;
}

export default function Chrome({ now, onRefresh, children }: ChromeProps) {
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();
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

  // 内容区是独立滚动区（双分区）：换屏把滚动位置带回顶部，
  // 替代此前依赖文档滚动的天然归顶行为
  useEffect(() => {
    contentRef.current?.scrollTo({ top: 0 });
  }, [pathname]);

  // 全局搜索需要自选池（命中「持仓/自选」跳分析，docs/02）——外壳兜底拉一次，
  // 用户直接落在任何屏都能搜；已在其它屏加载过则跳过（loaded 标记）
  useEffect(() => {
    if (!useWatchlist.getState().loaded) void useWatchlist.getState().load();
  }, []);

  // 快捷键（原型 bindChrome 同款）：⌘K/Ctrl+K 聚焦搜索；数字 1–6 切页（输入框内不拦截）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        searchRef.current?.focus();
        return;
      }
      const t = e.target as HTMLElement | null;
      // 原型只守卫 INPUT/TEXTAREA；补 SELECT——加减仓抽屉里操作下拉时数字键不应切页
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
      const k = parseInt(e.key, 10);
      if (k >= 1 && k <= 6) navigate(k <= 5 ? NAV[k - 1].path : WECHAT_NAV.path);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [navigate]);

  /** 搜索回车：命中持仓/自选（代码全等或名称包含）→ 分析页；否则带着问题去问答 */
  const onSearchEnter = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "Enter") return;
    const q = searchQuery.trim();
    if (!q) return;
    const held = usePortfolio.getState().view?.holdings ?? [];
    const watch = useWatchlist.getState().rows;
    const hit =
      held.find((h) => h.code === q || h.name.includes(q)) ??
      watch.find((w) => w.code === q || w.name.includes(q));
    setSearchQuery("");
    searchRef.current?.blur();
    if (hit) navigate(`/analysis?code=${hit.code}`);
    else navigate(`/chat?q=${encodeURIComponent(q)}`);
  };

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
          {/* 「快捷 → 问一句」组已移除（2026-11-28，用户拍板用处不大；其 ⌘K
              角标本就指向搜索框，属误导——直达问答走热键 5 / 顶栏搜索未命中兜底） */}
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
              <input
                id="global-search"
                ref={searchRef}
                placeholder="搜代码 / 名称 / 问一句"
                autoComplete="off"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={onSearchEnter}
              />
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

          {/* 双分区的内容滚动区：滚动条只覆盖这里，上方顶栏/行情条不参与滚动 */}
          <div className="content" id="main-scroll" ref={contentRef}>
            {children}
          </div>
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
