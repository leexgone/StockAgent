"""/quote：兜底链、熔断、限频注入、缓存、逐标的归一。"""

from conftest import Fake, build_service, default_quote_rows, make_settings, qrow
from zimu_datafetch.config import EM_PUSH2, SINA_HQ


def test_primary_ok(client):
    r = client.get("/quote", params={"codes": "sh600519,sh510300"})
    assert r.status_code == 200
    body = r.json()
    assert body["count"] == 2 and body["degraded"] is False and body["attempts"] == []
    assert [q["symbol"] for q in body["quotes"]] == ["sh600519", "sh510300"]
    q = body["quotes"][0]
    assert q["status"] == "ok" and q["chg_pct"] == 11.1111  # (10-9)/9*100


def test_fallback_to_em(client):
    bad = Fake(exc=ConnectionError("sina down"))
    good = Fake(lambda syms: [qrow(s, source="fake-em") for s in syms])
    c = make_client(sina=bad, em=good)
    r = c.get("/quote", params={"codes": "sh600519"})
    assert r.status_code == 200
    body = r.json()
    assert body["degraded"] is True
    assert body["quotes"][0]["source"] == "fake-em"
    att = body["attempts"]
    assert att[0]["source"] == SINA_HQ and "ConnectionError" in att[0]["error"]


def test_all_failed_502_but_alive(client):
    c = make_client(sina=Fake(exc=ConnectionError("sina down")),
                    em=Fake(exc=OSError("em refused")))
    r = c.get("/quote", params={"codes": "sh600519"})
    assert r.status_code == 502
    body = r.json()["error"]
    assert body["code"] == "UPSTREAM_ALL_FAILED"
    assert [a["source"] for a in body["detail"]["attempts"]] == [SINA_HQ, EM_PUSH2]
    # 「不崩」：服务照常处理其它请求
    assert c.get("/healthz").status_code == 200


def make_client(**kw):
    from fastapi.testclient import TestClient
    from zimu_datafetch.app import create_app
    return TestClient(create_app(service=build_service(**kw)))


def test_breaker_skips_failing_primary():
    bad = Fake(exc=ConnectionError("down"))
    good = Fake(default_quote_rows)
    # quote_ttl=0 关闭缓存：否则第 2、3 次请求全走缓存，压根碰不到源
    c = make_client(settings=make_settings(breaker_threshold=2, quote_ttl=0.0),
                    sina=bad, em=good)
    for _ in range(3):
        r = c.get("/quote", params={"codes": "sh600519"})
        assert r.status_code == 200  # 每次都靠 em 兜住
    assert bad.calls == 2            # 第 3 次直接熔断跳过，不再空等主源
    assert "熔断" in r.json()["attempts"][0]["error"]


def test_disable_injection(client):
    """ZIMU_DF_DISABLE 语义：主源视为不可用，直达备份（验收「主源故障降级」的开关面）。"""
    em = Fake(default_quote_rows)
    sina = Fake(default_quote_rows)
    c = make_client(settings=make_settings(disabled=frozenset({SINA_HQ})), sina=sina, em=em)
    r = c.get("/quote", params={"codes": "sh600519"})
    assert r.status_code == 200 and sina.calls == 0 and em.calls == 1
    body = r.json()
    assert body["degraded"] is True and "disabled" in body["attempts"][0]["error"]


def test_cache_per_symbol():
    sina = Fake(default_quote_rows)
    c = make_client(sina=sina)
    c.get("/quote", params={"codes": "sh600519"})
    r2 = c.get("/quote", params={"codes": "sh600519,sh510300"})
    assert sina.calls == 2                       # 一次全缺、一次只拉缺失
    assert sina.args_log[1][0][0] == ["sh510300"]
    c.get("/quote", params={"codes": "sh510300"})
    assert sina.calls == 2                       # 全命中缓存
    assert r2.json()["quotes"][0]["symbol"] == "sh600519"


def test_missing_symbol_becomes_no_data():
    sina = Fake(lambda syms: [qrow("sh600519")])  # 只回一半
    c = make_client(sina=sina)
    r = c.get("/quote", params={"codes": "sh600519,sz999999"})
    q = r.json()["quotes"][1]
    assert q["status"] == "no_data" and q["price"] is None


def test_max_symbols():
    c = make_client(settings=make_settings(max_quote_symbols=2),
                    sina=Fake(default_quote_rows))
    r = c.get("/quote", params={"codes": "sh600519,sh510300,sh000001"})
    assert r.status_code == 400
    assert "最多" in r.json()["error"]["message"]


def test_dedup(client):
    r = client.get("/quote", params={"codes": "sh600519,sh600519"})
    assert r.json()["count"] == 1
