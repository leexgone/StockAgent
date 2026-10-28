#!/usr/bin/env python3
"""P4-T3 联网冒烟：真实通道走一遍四端点 + 故障注入验收「主源故障时降级不崩」。

用法（在 datafetch/ 目录）：
    uv run python live_smoke.py

与离线单测的分工：单测锁编排语义（假源），本脚本验通道现实性——
新浪/东财真实响应、akshare 列名漂移、baostock 登录、退市样本三态。
输出 PASS/FAIL 汇总，非全绿时退出码 1。进程内 TestClient，不占端口。
"""

from __future__ import annotations

import os
import sys

sys.stdout.reconfigure(encoding="utf-8")

from fastapi.testclient import TestClient  # noqa: E402
from zimu_datafetch.app import create_app  # noqa: E402

CHECKS: list[tuple[str, bool, str]] = []


def check(name: str, ok: bool, info: str = "") -> None:
    CHECKS.append((name, ok, info))
    print(f"  [{'PASS' if ok else 'FAIL'}] {name}  {info}")


def app_with(disable: str) -> TestClient:
    os.environ["ZIMU_DF_DISABLE"] = disable
    try:
        return TestClient(create_app())
    finally:
        os.environ.pop("ZIMU_DF_DISABLE", None)


def main() -> int:
    print("== A. 常规通道 ==")
    c = app_with("")
    h = c.get("/healthz").json()
    check("healthz", h["ok"], f"version={h['version']} akshare={h['akshare']}")

    codes = "sh600519,sh510300,sh000001,sh110076,sh110074,sz127022"
    q = c.get("/quote", params={"codes": codes}).json()
    check("/quote 六标的归一", q["count"] == 6 and not q["degraded"])
    for row in q["quotes"]:
        print(f"       {row['symbol']:>9} {row['status']:<18} price={row['price']} prev={row['prev']} src={row['source']}")
    by = {r["symbol"]: r for r in q["quotes"]}
    check("活券状态 ok", by["sh600519"]["status"] == "ok" and by["sh510300"]["status"] == "ok")
    check("退市样本三态", by["sh110074"]["status"] == "halted_or_delisted"
          and by["sz127022"]["status"] == "empty_or_delisted",
          "回归基准 110074/127022（B10）")

    for sym in ("sh600519", "sh510300", "sh000001", "sz128136"):
        k = c.get(f"/kline/{sym}").json()
        last = k["kline"][-1]["date"] if k["count"] else "-"
        check(f"/kline {sym}（{k['kind']}/{k['source']}）", k["count"] > 0, f"count={k['count']} last={last}")

    cb = c.get("/cb_list").json()
    hit = next((i for i in cb["items"] if i["code"] == "128136"), {})
    check("/cb_list", cb["count"] > 300 and cb["redeem_source"] == "jsl_cb_redeem",
          f"count={cb['count']} spot={cb['spot_source']}")
    check("强赎 join + HTML 清洗", hit.get("redeem_status") == "已公告强赎"
          and "<" not in (hit.get("redeem_hint") or ""),
          f"hint={hit.get('redeem_hint')!r}")
    check("北交所定转被过滤", all(not i["symbol"].startswith("bj") for i in cb["items"]))

    cal = c.get("/calendar", params={"start": "2026-10-01", "end": "2026-11-30"}).json()
    fut = [d for d in cal["trade_dates"] if d > "2026-10-28"]
    check("/calendar 含未来交易日", cal["count"] > 20 and fut, f"count={cal['count']} 未来={fut[:3]}")

    print("\n== B. 主源故障注入（ZIMU_DF_DISABLE=sina_hq） ==")
    c2 = app_with("sina_hq")
    q2 = c2.get("/quote", params={"codes": "sh600519,sh510300,sh000001"}).json()
    ok = q2["count"] == 3 and q2["degraded"] and q2["quotes"][0]["source"] == "em-push2"
    check("降级到东财不崩", ok, f"attempts={q2['attempts']}")
    em, sina = q2["quotes"][0], q["quotes"][0]
    if em["price"] and sina["price"]:
        dev = abs(em["price"] - sina["price"]) / sina["price"]
        check("东财 fltt=2 缩放与新浪一致", dev < 0.02, f"em={em['price']} sina={sina['price']} dev={dev:.4f}")

    print("\n== C. 实时链路全挂（disable sina_hq,em_push2） ==")
    c3 = app_with("sina_hq,em_push2")
    r = c3.get("/quote", params={"codes": "sh600519"})
    body = r.json()
    check("502 错误契约", r.status_code == 502 and body["error"]["code"] == "UPSTREAM_ALL_FAILED"
          and len(body["error"]["detail"]["attempts"]) == 2)
    k3 = c3.get("/kline/sh600519").json()
    check("日线链不受影响", k3["count"] > 0, f"source={k3['source']}")
    check("服务进程无恙", c3.get("/healthz").status_code == 200)

    bad = c3.get("/quote", params={"codes": "600519"})
    check("裸代码仍 400（B7）", bad.status_code == 400 and bad.json()["error"]["code"] == "BAD_SYMBOL")

    n_ok = sum(1 for _, ok, _ in CHECKS if ok)
    print(f"\n汇总：{n_ok}/{len(CHECKS)} PASS")
    for name, ok, info in CHECKS:
        if not ok:
            print(f"  FAILED: {name}  {info}")
    return 0 if n_ok == len(CHECKS) else 1


if __name__ == "__main__":
    sys.exit(main())
