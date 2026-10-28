"""/cb_list：spot×redeem join、jsl 失败降级、spot 失败 502。"""

from conftest import Fake, build_service
from fastapi.testclient import TestClient
from zimu_datafetch.app import create_app


def make_client(**kw):
    return TestClient(create_app(service=build_service(**kw)))


SPOT_ROWS = [
    {"symbol": "sh110076", "code": "110076", "mkt": "SH", "kind": "cb", "name": "华海转债",
     "price": 109.906, "prev": 109.906, "open": 0.0, "high": 0.0, "low": 0.0,
     "volume": 0.0, "amount": 0.0, "chg_pct": 0.0, "ticktime": "15:35:55",
     "status": "halted_or_delisted", "spot_source": "fake-spot"},
    {"symbol": "sz128136", "code": "128136", "mkt": "SZ", "kind": "cb", "name": "Z讯转债",
     "price": 107.933, "prev": 107.942, "open": 107.935, "high": 107.988, "low": 107.916,
     "volume": 3108251.0, "amount": 335465983.5, "chg_pct": -0.01, "ticktime": "15:00:00",
     "status": "ok", "spot_source": "fake-spot"},
]

REDEEM = {"128136": {"stock_code": "002475", "stock_name": "立讯精密", "convert_price": 55.45,
                     "trigger_pct": 130.0, "trigger_price": 72.085, "stock_price": 49.7,
                     "redeem_price": None, "last_trade_date": "2026-10-28",
                     "maturity_date": "2026-11-03", "scale": 30.0, "scale_left": 29.758,
                     "redeem_status": "已公告强赎", "redeem_hint": "临近到期 2026-10-28 最后交易"}}


def test_join():
    cl = make_client(cb_spot=Fake(SPOT_ROWS), cb_redeem=Fake(REDEEM))
    r = cl.get("/cb_list")
    assert r.status_code == 200
    b = r.json()
    assert b["count"] == 2 and b["spot_source"] == "sina_cb_spot"
    assert b["redeem_source"] == "jsl_cb_redeem" and b["degraded"] is False
    it = {i["code"]: i for i in b["items"]}
    assert it["128136"]["redeem_status"] == "已公告强赎"
    assert it["128136"]["trigger_price"] == 72.085
    # 未进强赎表的券：字段齐但值为 null，消费方不用 .get 防御
    assert it["110076"]["redeem_status"] is None
    assert it["110076"]["last_trade_date"] is None
    assert it["110076"]["redeem_source"] == "jsl_cb_redeem"


def test_redeem_fail_degrades():
    cl = make_client(cb_spot=Fake(SPOT_ROWS), cb_redeem=Fake(exc=ConnectionError("jsl 403")))
    b = cl.get("/cb_list").json()
    assert b["redeem_source"] is None
    assert b["degraded"] is True
    assert b["attempts"][0]["source"] == "jsl_cb_redeem"
    assert all(i["redeem_status"] is None for i in b["items"])
    assert all(i["redeem_source"] is None for i in b["items"])


def test_spot_fail_502():
    cl = make_client(cb_spot=Fake(exc=ConnectionError("sina table down")))
    r = cl.get("/cb_list")
    assert r.status_code == 502
    assert cl.get("/healthz").status_code == 200  # 不崩


def test_cache(client):
    b1 = client.get("/cb_list").json()
    b2 = client.get("/cb_list").json()
    assert b1["items"] == b2["items"]
