"""/kline：链序、显式 kind、降级标注、空结果语义。"""

from conftest import Fake, default_kline_rows, make_settings
from fastapi.testclient import TestClient
from zimu_datafetch.app import create_app
from conftest import build_service


def make_client(**kw):
    return TestClient(create_app(service=build_service(**kw)))


def test_stock_inferred_chain(client):
    r = client.get("/kline/sh600519")
    assert r.status_code == 200
    b = r.json()
    assert b["kind"] == "stock" and b["kind_routed"] == "inferred"
    assert b["source"] == "stock_a" and b["adjusted"] == "qfq"
    assert b["count"] == 1 and b["kline"][0]["close"] == 1.1


def test_explicit_kind_etf(client):
    r = client.get("/kline/sh510300", params={"kind": "etf"})
    b = r.json()
    assert b["kind_routed"] == "explicit" and b["source"] == "etf_a"
    assert b["adjusted"] == "none"


def test_stock_fallback_order_sina_baostock_em():
    rows = default_kline_rows("sh600519", "2026-09-01", "2026-10-28")
    a = Fake(exc=ConnectionError("sina down"))
    b = Fake(rows)
    c = Fake(rows)
    cl = make_client(stock_a=a, stock_b=b, stock_c=c)
    r = cl.get("/kline/sh600519")
    assert r.status_code == 200
    body = r.json()
    assert body["degraded"] is True and body["source"] == "stock_b"
    assert a.calls == 1 and b.calls == 1 and c.calls == 0  # 备胎成功就不惊动东财
    assert body["attempts"][0]["source"] == "stock_a"


def test_stock_last_resort_em():
    rows = default_kline_rows("sh600519", "2026-09-01", "2026-10-28")
    cl = make_client(stock_a=Fake(exc=ConnectionError("x")),
                     stock_b=Fake(exc=RuntimeError("baostock login 10002007")),
                     stock_c=Fake(rows))
    r = cl.get("/kline/sh600519")
    assert r.status_code == 200 and r.json()["source"] == "stock_c"


def test_all_kline_sources_502():
    cl = make_client(stock_a=Fake(exc=ConnectionError("1")),
                     stock_b=Fake(exc=ConnectionError("2")),
                     stock_c=Fake(exc=ConnectionError("3")))
    r = cl.get("/kline/sh600519")
    assert r.status_code == 502
    assert len(r.json()["error"]["detail"]["attempts"]) == 3
    assert cl.get("/healthz").status_code == 200


def test_empty_result_is_200_count0(client):
    cl = make_client(stock_a=Fake([]))
    r = cl.get("/kline/sh600519", params={"start": "1990-01-01", "end": "1990-01-03"})
    assert r.status_code == 200
    assert r.json()["count"] == 0 and r.json()["kline"] == []


def test_date_passthrough_normalized():
    cap = Fake(lambda sym, st, en: [])
    cl = make_client(stock_a=cap)
    r = cl.get("/kline/sh600519", params={"start": "20260901"})
    assert r.status_code == 200
    args = cap.args_log[0][0]
    assert args[0] == "sh600519" and args[1] == "2026-09-01"
    assert args[2] == r.json()["end"]  # 默认今天


def test_cb_symbol_infers_cb(client):
    r = client.get("/kline/sh110076")
    assert r.json()["kind"] == "cb" and r.json()["source"] == "cb_a"
