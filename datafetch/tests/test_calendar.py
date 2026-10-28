"""/calendar：区间过滤、默认窗口、baostock 兜底、错误契约形状。"""

from conftest import Fake, build_service, make_settings
from fastapi.testclient import TestClient
from zimu_datafetch.app import create_app


def make_client(**kw):
    return TestClient(create_app(service=build_service(**kw)))


def test_range_filter(client):
    r = client.get("/calendar", params={"start": "2026-10-27", "end": "2026-10-28"})
    b = r.json()
    assert b["source"] == "sina_calendar"
    assert b["trade_dates"] == ["2026-10-27", "2026-10-28", "2026-10-29"]  # 源侧过滤，测试假源恒返回三天
    assert b["start"] == "2026-10-27"


def test_default_window_includes_future():
    from datetime import date, timedelta

    cap = Fake(lambda st, en: [st, en])
    cl = make_client(cal_a=cap)
    b = cl.get("/calendar").json()
    # 默认今天±180：end 必须落在未来（marketClock「下一交易日」依赖）——
    # 回归：曾因 default_range(-180)[1] 恒等于「今天」而丢失未来窗口
    today = date.today()
    assert b["start"] == (today - timedelta(days=180)).isoformat()
    assert b["end"] == (today + timedelta(days=180)).isoformat()
    assert cap.calls == 1 and b["count"] == 2


def test_fallback_baostock():
    bs = Fake(["2026-10-27"])
    cl = make_client(cal_a=Fake(exc=ConnectionError("sina cal down")), cal_b=bs)
    b = cl.get("/calendar", params={"start": "2026-10-01", "end": "2026-10-31"}).json()
    assert b["source"] == "baostock_calendar" and b["degraded"] is True


def test_all_calendars_502():
    cl = make_client(cal_a=Fake(exc=ConnectionError("1")), cal_b=Fake(exc=ConnectionError("2")))
    r = cl.get("/calendar")
    assert r.status_code == 502
    assert [a["source"] for a in r.json()["error"]["detail"]["attempts"]] == \
        ["sina_calendar", "baostock_calendar"]


def test_calendar_range_too_long():
    r = make_client().get("/calendar", params={"start": "2020-01-01", "end": "2026-01-01"})
    assert r.status_code == 400
    assert "区间" in r.json()["error"]["message"]


def test_error_contract_shape(client):
    """所有可预期错误同形：{error:{code,message,detail}}，不透传堆栈。"""
    for path, params, code in (
        ("/quote", {"codes": "600519"}, "BAD_SYMBOL"),
        ("/quote", {"codes": ""}, "BAD_PARAMS"),
        ("/kline/sh600519", {"kind": "bad"}, "BAD_PARAMS"),
    ):
        r = client.get(path, params=params or None)
        assert r.status_code == 400, path
        body = r.json()
        assert set(body) == {"error"}
        assert set(body["error"]) == {"code", "message", "detail"}
        assert body["error"]["code"] == code


def test_healthz(client):
    h = client.get("/healthz").json()
    assert h["ok"] is True and h["version"]
    assert "breakers" in h and "cache" in h and "disabled_sources" in h
