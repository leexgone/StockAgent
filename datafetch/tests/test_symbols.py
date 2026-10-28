"""symbol / 参数校验（B7 在 sidecar 侧的落实）。"""


def test_bare_code_rejected(client):
    r = client.get("/quote", params={"codes": "600519"})
    assert r.status_code == 400
    body = r.json()["error"]
    assert body["code"] == "BAD_SYMBOL"
    assert "市场前缀" in body["message"]
    assert body["detail"]["input"] == "600519"


def test_prefix_case_insensitive(client):
    r = client.get("/quote", params={"codes": "SH600519"})
    assert r.status_code == 200
    assert r.json()["quotes"][0]["symbol"] == "sh600519"


def test_bad_length_rejected(client):
    for bad in ("sh600", "sh6005199", "us600519", "sha0519", ""):
        r = client.get("/quote", params={"codes": bad or "x"})
        assert r.status_code == 400, bad


def test_codes_empty(client):
    r = client.get("/quote", params={"codes": " , "})
    assert r.status_code == 400
    assert r.json()["error"]["code"] == "BAD_PARAMS"


def test_kind_illegal(client):
    r = client.get("/kline/sh600519", params={"kind": "zzz"})
    assert r.status_code == 400
    assert r.json()["error"]["code"] == "BAD_PARAMS"


def test_bj_without_kind_400(client):
    # 北交所超出覆盖范围：推断返回 other → 必须显式 kind，且 kind 只有四类 → 拒
    r = client.get("/kline/bj830799")
    assert r.status_code == 400
    assert "推断" in r.json()["error"]["message"]


def test_date_formats(client):
    ok = client.get("/kline/sh600519", params={"start": "20260901", "end": "2026-10-28"})
    assert ok.status_code == 200
    assert ok.json()["start"] == "2026-09-01"
    for bad in ("2026-13-99", "20261301", "0901", "abc"):
        r = client.get("/kline/sh600519", params={"start": bad})
        assert r.status_code == 400, bad


def test_start_after_end(client):
    r = client.get("/kline/sh600519", params={"start": "2026-10-28", "end": "2026-10-01"})
    assert r.status_code == 400
    assert "晚于" in r.json()["error"]["message"]
