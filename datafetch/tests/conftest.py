"""测试基座：MarketService 全链假源注入，不打网络。

约定：假源用**真实源名**（config.SINA_HQ 等）注册——`ZIMU_DF_DISABLE`、
熔断、限频闸的语义才能按契约被测到。
"""

from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from zimu_datafetch.config import (
    BAOSTOCK_CALENDAR,
    EM_PUSH2,
    JSL_CB_REDEEM,
    SINA_CB_SPOT,
    SINA_CALENDAR,
    SINA_HQ,
    Settings,
)
from zimu_datafetch.app import create_app
from zimu_datafetch.service import MarketService


class Fake:
    """可编程假源：result 为值或 callable(*args)；exc 非空则调用即抛。计数 calls。"""

    def __init__(self, result=None, exc: Exception | None = None):
        self.result = result
        self.exc = exc
        self.calls = 0
        self.args_log: list[tuple] = []

    def __call__(self, *args, **kwargs):
        self.calls += 1
        self.args_log.append((args, kwargs))
        if self.exc:
            raise self.exc
        if callable(self.result):
            return self.result(*args, **kwargs)
        return self.result


def qrow(sym: str, *, price=10.0, prev=9.0, status="ok", source="fake-sina") -> dict:
    return {
        "symbol": sym, "code": sym[2:], "mkt": sym[:2].upper(), "kind": "stock",
        "name": "测试标的", "price": price, "prev": prev,
        "open": None, "high": None, "low": None, "volume": None, "amount": None,
        "chg_pct": round((price - prev) / prev * 100, 4) if price and prev else None,
        "date": "2026-10-28", "time": "15:00:00", "status": status, "source": source,
    }


def default_quote_rows(symbols):
    return [qrow(s) for s in symbols]


def default_kline_rows(symbol, start, end):
    return [{"date": start, "open": 1.0, "high": 1.2, "low": 0.9, "close": 1.1,
             "volume": 100.0, "amount": 110.0}]


def make_settings(**over) -> Settings:
    from zimu_datafetch.config import DEFAULT_GATES

    base = dict(quote_ttl=30.0, kline_ttl=300.0, cb_ttl=300.0, calendar_ttl=600.0,
                breaker_threshold=3, breaker_cooloff=60.0, disabled=frozenset(),
                max_quote_symbols=50,
                # 测试环境不等限频闸（真实节奏由生产默认值承担，见 config.DEFAULT_GATES）
                gates={k: 0.0 for k in DEFAULT_GATES}, gate_default=0.0)
    base.update(over)
    return Settings(**base)


def build_service(settings: Settings | None = None, *,
                  sina=None, em=None,
                  stock_a=None, stock_b=None, stock_c=None,
                  etf_a=None, index_a=None, cb_a=None,
                  cb_spot=None, cb_redeem=None,
                  cal_a=None, cal_b=None):
    """全类目假源服务。未显式传入的类目用「恒成功单源链」占位。

    注意：假源必须在函数内构造——可变的 Fake 做默认参数会跨用例共享计数。
    """
    s = settings or make_settings()
    sina = sina or Fake(default_quote_rows)
    quote = [(SINA_HQ, sina)]
    if em is not None:
        quote.append((EM_PUSH2, em))
    kline = {
        "stock": [(f"stock_a", stock_a or Fake(default_kline_rows))],
        "etf": [("etf_a", etf_a or Fake(default_kline_rows))],
        "index": [("index_a", index_a or Fake(default_kline_rows))],
        "cb": [("cb_a", cb_a or Fake(default_kline_rows))],
    }
    if stock_a is not None or stock_b is not None or stock_c is not None:
        chain = [("stock_a", stock_a or Fake(exc=RuntimeError("absent")))]
        if stock_b is not None:
            chain.append(("stock_b", stock_b))
        if stock_c is not None:
            chain.append(("stock_c", stock_c))
        kline["stock"] = chain
    cal = [(SINA_CALENDAR, cal_a or Fake(
        lambda st, en: ["2026-10-27", "2026-10-28", "2026-10-29"]))]
    if cal_b is not None:
        cal.append((BAOSTOCK_CALENDAR, cal_b))
    return MarketService(
        s,
        quote_sources=quote,
        kline_sources=kline,
        cb_spot_sources=[(SINA_CB_SPOT, cb_spot or Fake([
            {"symbol": "sh110076", "code": "110076", "mkt": "SH", "kind": "cb",
             "name": "华海转债", "price": 109.9, "prev": 109.9, "open": None,
             "high": None, "low": None, "volume": 0, "amount": 0, "chg_pct": 0.0,
             "ticktime": "15:00:00", "status": "halted_or_delisted",
             "spot_source": "fake-spot"},
        ]))],
        cb_redeem_sources=[(JSL_CB_REDEEM, cb_redeem or Fake({
            "128136": {"stock_code": "002475", "stock_name": "立讯精密",
                       "convert_price": 55.45, "trigger_pct": 130.0,
                       "trigger_price": 72.085, "stock_price": 49.7,
                       "redeem_price": None, "last_trade_date": "2026-10-28",
                       "maturity_date": "2026-11-03", "scale": 30.0,
                       "scale_left": 29.758, "redeem_status": "已公告强赎",
                       "redeem_hint": "临近到期 2026-10-28 最后交易"},
        }))],
        calendar_sources=cal,
    )


@pytest.fixture
def service():
    return build_service()


@pytest.fixture
def client(service):
    return TestClient(create_app(service=service))
