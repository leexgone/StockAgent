"""/sector_map（P7-T6/ADR-0016）：全量映射、部分板降级、spot/空映射 502、
singleflight 缓存、禁用注入、pacing 契约。假源返回 (code→板名, 失败板明细)。"""

from conftest import Fake, build_service, make_settings
from fastapi.testclient import TestClient
from zimu_datafetch.app import create_app
from zimu_datafetch.config import SINA_SECTOR


def make_client(**kw):
    return TestClient(create_app(service=build_service(**kw)))


MAP = {"600519": "酿酒行业", "600011": "电力行业", "601166": "金融行业"}


def test_full_map():
    cl = make_client(sector=Fake((MAP, [])))
    r = cl.get("/sector_map")
    assert r.status_code == 200
    b = r.json()
    assert b["count"] == 3 and b["source"] == "sina_sector"
    assert b["degraded"] is False and b["attempts"] == []
    assert b["sectors"] == MAP


def test_partial_board_failure_degrades():
    # 单板 detail 失败 → 跳过该板、降级缓存部分映射（缺码留空次日再补）
    cl = make_client(sector=Fake(({"600519": "酿酒行业"}, ["new_xxx: ConnectionError: down"])))
    b = cl.get("/sector_map").json()
    assert b["degraded"] is True
    assert b["sectors"] == {"600519": "酿酒行业"}
    assert any("板块成分缺失" in a["error"] for a in b["attempts"])


def test_spot_fail_502():
    cl = make_client(sector=Fake(exc=ConnectionError("sina sector down")))
    r = cl.get("/sector_map")
    assert r.status_code == 502
    assert r.json()["error"]["code"] == "UPSTREAM_ALL_FAILED"
    assert cl.get("/healthz").status_code == 200  # 服务不崩


def test_empty_map_502():
    cl = make_client(sector=Fake(({}, [])))
    r = cl.get("/sector_map")
    assert r.status_code == 502
    assert r.json()["error"]["code"] == "UPSTREAM_ALL_FAILED"


def test_cache_singleflight():
    fake = Fake((MAP, []))
    cl = make_client(sector=fake)
    b1 = cl.get("/sector_map").json()
    b2 = cl.get("/sector_map").json()
    assert b1["sectors"] == b2["sectors"]
    assert fake.calls == 1  # 二次命中缓存，不重复建映射


def test_disable_injection():
    cl = TestClient(create_app(service=build_service(
        settings=make_settings(disabled=frozenset({SINA_SECTOR})),
        sector=Fake((MAP, [])))))
    r = cl.get("/sector_map")
    assert r.status_code == 502
    assert r.json()["error"]["code"] == "UPSTREAM_ALL_FAILED"


def test_gap_passed():
    fake = Fake((MAP, []))
    cl = TestClient(create_app(service=build_service(
        settings=make_settings(gates={SINA_SECTOR: 0.7}, gate_default=0.0),
        sector=fake)))
    assert cl.get("/sector_map").status_code == 200
    # pacing 契约：源函数收到闸值（函数内 N 次外呼自行 sleep，RateGate 只管源级间隔）
    assert fake.args_log[0][0][0] == 0.7