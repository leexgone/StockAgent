"""行情服务编排（P4-T3）：兜底链 + 缓存 + 熔断 + 限频的组合处。

编排规则（验收「主源故障时降级不崩」的实现处）：

1. 每个数据类目是一条**有序源链**；请求先查缓存，未命中按链依次尝试。
2. 单源尝试前：`ZIMU_DF_DISABLE` 注入跳过 → 熔断 open 跳过（记 attempt 不空等）
   → 限频闸排队 → 调用；异常记一次失败继续下一源。
3. 全链失败才抛 502 `UPSTREAM_ALL_FAILED`，`detail.attempts` 逐源留原因——
   错误是契约化的 JSON，不是堆栈；服务进程始终正常。
4. 有降级发生（attempts 非空）时响应带 `degraded: true` + attempts 明细，
   调用方（P4-T4 Rust 适配器）能区分「主源正常」与「凑合活着」。

源链与签名：`/quote` fn(symbols)；`/kline` fn(symbol, start, end)；
`/cb_list` fn()；`/calendar` fn(start, end)。测试从构造函数注入假源，
不打网络；生产缺省即用 `DEFAULT_*_CHAIN`（`create_app()` 装配）。

口径边界：这里只做**通道路由与原始行归一**，不定义任何业务口径——
涨跌停/费用/仓位全在 `zimu-domain`（03 唯一来源），ETF `limit_pct` 等
标的属性由数据层（holdings meta）携带，sidecar 不推断。
"""

from __future__ import annotations

import logging
from datetime import date, datetime, timedelta, timezone
from typing import Callable

from . import sources as SRC
from .config import (
    BAOSTOCK_CALENDAR,
    BAOSTOCK_DAILY,
    BAOSTOCK_INDEX,
    EM_PUSH2,
    EM_STOCK_HIST,
    JSL_CB_REDEEM,
    SINA_CB_DAILY,
    SINA_CB_SPOT,
    SINA_CALENDAR,
    SINA_ETF_DAILY,
    SINA_HQ,
    SINA_INDEX_DAILY,
    SINA_STOCK_DAILY,
    TENCENT_QT,
    Settings,
)
from .errors import all_failed, attempt, bad_params
from .governor import Breaker, RateGate, TTLCache
from .symbols import (
    default_range,
    infer_kind,
    kind_for_kline,
    normalize_date,
    normalize_symbol,
)

log = logging.getLogger("zimu.datafetch")

# 默认源链（顺序即优先级；spike/复测结论见 docs/04 附录）
# B14（2026-10-29）：腾讯进二通道——新浪不 carry 部分指数（中证转债 sh000832
# 回空名零价行），quote 编排按**标的粒度补齐**（见 MarketService.quote），
# 未覆盖标的自动落到下一源，而不是整批成功即止。
DEFAULT_QUOTE_CHAIN: list[tuple[str, Callable]] = [
    (SINA_HQ, SRC.sina_hq_quote),
    (TENCENT_QT, SRC.tencent_qt_quote),
    (EM_PUSH2, SRC.em_push2_quote),
]
DEFAULT_KLINE_CHAINS: dict[str, list[tuple[str, Callable]]] = {
    "stock": [(SINA_STOCK_DAILY, SRC.sina_stock_daily),
              (BAOSTOCK_DAILY, SRC.baostock_stock_daily),
              (EM_STOCK_HIST, SRC.em_stock_hist)],
    # ETF/转债 baostock 不覆盖，东财系被指纹拦——仅新浪系（全链失败即 502，诚实降级）
    "etf": [(SINA_ETF_DAILY, SRC.sina_etf_daily)],
    "index": [(SINA_INDEX_DAILY, SRC.sina_index_daily),
              (BAOSTOCK_INDEX, SRC.baostock_index_daily)],
    "cb": [(SINA_CB_DAILY, SRC.sina_cb_daily)],
}
DEFAULT_CB_SPOT_CHAIN: list[tuple[str, Callable]] = [(SINA_CB_SPOT, SRC.cb_spot_sina)]
DEFAULT_CB_REDEEM_CHAIN: list[tuple[str, Callable]] = [(JSL_CB_REDEEM, SRC.cb_redeem_jsl)]
DEFAULT_CALENDAR_CHAIN: list[tuple[str, Callable]] = [
    (SINA_CALENDAR, SRC.calendar_sina),
    (BAOSTOCK_CALENDAR, SRC.calendar_baostock),
]


def _now_iso() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


class MarketService:
    """四个数据类目的编排。构造可整体注入假源（测试）；生产走默认链。"""

    def __init__(
        self,
        settings: Settings | None = None,
        *,
        quote_sources=None,
        kline_sources=None,
        cb_spot_sources=None,
        cb_redeem_sources=None,
        calendar_sources=None,
    ):
        self.s = settings or Settings.from_env()
        self.cache = TTLCache()
        self.breaker = Breaker(self.s.breaker_threshold, self.s.breaker_cooloff)
        self.gate = RateGate(self.s.gates, default=self.s.gate_default)
        self.quote_sources = quote_sources or DEFAULT_QUOTE_CHAIN
        self.kline_sources = kline_sources or DEFAULT_KLINE_CHAINS
        self.cb_spot_sources = cb_spot_sources or DEFAULT_CB_SPOT_CHAIN
        self.cb_redeem_sources = cb_redeem_sources or DEFAULT_CB_REDEEM_CHAIN
        self.calendar_sources = calendar_sources or DEFAULT_CALENDAR_CHAIN

    # ---------- 治理 ----------

    def _chain(self, chain, invoke, *, required: bool = True):
        """按链尝试；返回 (value, used_source, attempts)。全败且 required → 502。"""
        attempts: list[dict] = []
        for name, fn in chain:
            if name in self.s.disabled:
                attempts.append(attempt(name, "disabled（ZIMU_DF_DISABLE 故障注入）"))
                continue
            allowed, why = self.breaker.check(name)
            if not allowed:
                attempts.append(attempt(name, f"熔断跳过：{why}"))
                continue
            self.gate.wait(name)
            try:
                value = invoke(fn)
            except Exception as e:  # noqa: BLE001 —— 任何源异常都进兜底链，绝不让它逃出端点
                self.breaker.failure(name)
                attempts.append(attempt(name, f"{type(e).__name__}: {e}"))
                log.warning("source %s failed: %s: %s", name, type(e).__name__, e)
                continue
            self.breaker.success(name)
            return value, name, attempts
        if required:
            raise all_failed(attempts)
        return None, None, attempts

    # ---------- /quote ----------

    def quote(self, codes_csv: str) -> dict:
        items = [c for c in (codes_csv or "").split(",") if c.strip()]
        if not items:
            raise bad_params("codes 不能为空")
        syms: list[str] = []
        for it in items:
            sym = normalize_symbol(it)
            if sym not in syms:
                syms.append(sym)
        if len(syms) > self.s.max_quote_symbols:
            raise bad_params(f"单次最多 {self.s.max_quote_symbols} 个标的，收到 {len(syms)}")

        rows: dict[str, dict] = {}
        missing: list[str] = []
        for sym in syms:
            hit = self.cache.get(f"q:{sym}")
            if hit is not None:
                rows[sym] = hit
            else:
                missing.append(sym)

        attempts: list[dict] = []
        if missing:
            # 逐标的补齐（B14）：与 _chain 的整批语义不同——某源成功但**未覆盖**
            # 部分标的（如新浪没有中证转债指数）时，剩余标的继续走下一源；
            # 只有「没有任何源成功过」才 502（UPSTREAM_ALL_FAILED 语义不变）。
            by_sym: dict[str, dict] = {}
            pending = list(missing)
            any_success = False
            chain = self.quote_sources
            for idx, (name, fn) in enumerate(chain):
                if not pending:
                    break
                if name in self.s.disabled:
                    attempts.append(attempt(name, "disabled（ZIMU_DF_DISABLE 故障注入）"))
                    continue
                allowed, why = self.breaker.check(name)
                if not allowed:
                    attempts.append(attempt(name, f"熔断跳过：{why}"))
                    continue
                self.gate.wait(name)
                try:
                    fetched = fn(pending)
                except Exception as e:  # noqa: BLE001 —— 任何源异常都进兜底链，绝不让它逃出端点
                    self.breaker.failure(name)
                    attempts.append(attempt(name, f"{type(e).__name__}: {e}"))
                    log.warning("source %s failed: %s: %s", name, type(e).__name__, e)
                    continue
                self.breaker.success(name)
                any_success = True
                got = {r["symbol"]: r for r in fetched}
                still: list[str] = []
                for sym in pending:
                    row = got.get(sym)
                    if row is None or row.get("status") == "no_data":
                        still.append(sym)  # 源未返回/未收录 → 交给下一源
                    else:
                        by_sym[sym] = row
                if still and idx < len(chain) - 1:
                    attempts.append(attempt(
                        name,
                        f"未返回 {len(still)} 个标的（{'、'.join(still[:3])}），交由下一源补齐"))
                pending = still
            if pending and not any_success:
                raise all_failed(attempts)
            for sym in pending:
                # 链尽仍缺（非法代码/停牌除名/全链未收录）→ 合成 no_data 行，
                # 保持逐标的可归一；不算降级（诚实缺行）
                by_sym[sym] = {
                    "symbol": sym, "code": sym[2:], "mkt": sym[:2].upper(),
                    "kind": infer_kind(sym), "name": None,
                    "price": None, "prev": None, "open": None, "high": None,
                    "low": None, "volume": None, "amount": None, "chg_pct": None,
                    "date": None, "time": None, "status": "no_data", "source": None,
                }
            for sym in missing:
                rows[sym] = by_sym[sym]
                self.cache.set(f"q:{sym}", rows[sym], self.s.quote_ttl)

        return {
            "as_of": _now_iso(),
            "count": len(syms),
            "degraded": bool(attempts),
            "attempts": attempts,
            "quotes": [rows[s] for s in syms],
        }

    # ---------- /kline ----------

    def kline(self, symbol_raw: str, kind: str | None = None,
              start: str | None = None, end: str | None = None) -> dict:
        sym = normalize_symbol(symbol_raw)
        s_iso = normalize_date(start, "start") if start else default_range(400)[0]
        e_iso = normalize_date(end, "end") if end else default_range(0)[1]
        if s_iso > e_iso:
            raise bad_params(f"start {s_iso} 晚于 end {e_iso}")
        k, routed = kind_for_kline(sym, kind)

        key = f"k:{k}:{sym}:{s_iso}:{e_iso}"
        hit = self.cache.get(key)
        if hit is not None:
            return hit

        rows, used, attempts = self._chain(
            self.kline_sources[k], lambda fn: fn(sym, s_iso, e_iso)
        )
        payload = {
            "symbol": sym, "kind": k, "kind_routed": routed,
            "adjusted": "qfq" if k in ("stock",) else "none",
            "start": s_iso, "end": e_iso,
            "count": len(rows), "source": used,
            "degraded": bool(attempts), "attempts": attempts,
            "kline": rows,
        }
        self.cache.set(key, payload, self.s.kline_ttl)
        return payload

    # ---------- /cb_list ----------

    def cb_list(self) -> dict:
        hit = self.cache.get("cb:list")
        if hit is not None:
            return hit

        spot, used_spot, attempts_spot = self._chain(
            self.cb_spot_sources, lambda fn: fn()
        )
        redeem, used_redeem, attempts_redeem = self._chain(
            self.cb_redeem_sources, lambda fn: fn(), required=False
        )
        items = []
        for row in spot:
            extra = (redeem or {}).get(row["code"]) or {
                k: None for k in (
                    "stock_code", "stock_name", "convert_price", "trigger_pct",
                    "trigger_price", "stock_price", "redeem_price", "last_trade_date",
                    "maturity_date", "scale", "scale_left", "redeem_status",
                    "redeem_hint",
                )
            }
            items.append({**row, **extra,
                          "redeem_source": used_redeem if redeem is not None else None})
        attempts = attempts_spot + attempts_redeem
        payload = {
            "as_of": _now_iso(), "count": len(items),
            "spot_source": used_spot, "redeem_source": used_redeem,
            "degraded": bool(attempts), "attempts": attempts,
            "items": items,
        }
        self.cache.set("cb:list", payload, self.s.cb_ttl)
        return payload

    # ---------- /calendar ----------

    def calendar(self, start: str | None = None, end: str | None = None) -> dict:
        # 默认窗口 = 今天 ±180 天：end 必须在未来——marketClock 的「下一交易日」依赖它
        s_iso = normalize_date(start, "start") if start else default_range(180)[0]
        e_iso = normalize_date(end, "end") if end else (
            date.today() + timedelta(days=180)).isoformat()
        if s_iso > e_iso:
            raise bad_params(f"start {s_iso} 晚于 end {e_iso}")
        if (datetime.strptime(e_iso, "%Y-%m-%d")
                - datetime.strptime(s_iso, "%Y-%m-%d")).days > 730 + 180:
            raise bad_params("日历区间最多约 910 天（新浪全量表按区间过滤，无需拉满 1990 起）")

        key = f"cal:{s_iso}:{e_iso}"
        hit = self.cache.get(key)
        if hit is not None:
            return hit

        days, used, attempts = self._chain(
            self.calendar_sources, lambda fn: fn(s_iso, e_iso)
        )
        payload = {
            "start": s_iso, "end": e_iso, "count": len(days), "trade_dates": days,
            "source": used, "degraded": bool(attempts), "attempts": attempts,
        }
        self.cache.set(key, payload, self.s.calendar_ttl)
        return payload
