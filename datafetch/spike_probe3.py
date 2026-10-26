#!/usr/bin/env python3
"""P2-T1 · datafetch spike 第三轮：补齐缺口

1) 个股→行业映射：stock_sector_spot 拿板块 label，stock_sector_detail 拿成分股
2) 东财 push2 ulist 批量直连（带 UA）：验证备份通道的字段与缩放
3) baostock 兜底通道：登录、股票/指数日线字段
4) 用现存活转债（sh110076 华海转债）复验新浪转债日线
"""

from __future__ import annotations

import sys
import time

sys.stdout.reconfigure(encoding="utf-8")

import requests  # noqa: E402

UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36"}

print("# spike 第三轮")

# ---------- 1. 行业映射 ----------
print("\n## 1. 个股→行业映射（新浪板块）")
import akshare as ak  # noqa: E402
import pandas as pd  # noqa: E402

try:
    t0 = time.perf_counter()
    detail = ak.stock_sector_detail(sector="new_blhy")  # 玻璃行业
    print(f"  [ok] stock_sector_detail(new_blhy) {time.perf_counter()-t0:.2f}s shape={detail.shape}")
    print(f"  columns: {list(detail.columns)}")
    with pd.option_context("display.max_columns", None, "display.width", 200):
        print(detail.head(3).to_string())
except Exception as e:  # noqa: BLE001
    print(f"  [fail] {type(e).__name__}: {str(e)[:200]}")

# ---------- 2. 东财 push2 批量 ----------
print("\n## 2. 东财 push2 ulist 批量直连（备份通道字段验证）")
# secid: 1.=沪 0.=深；f2最新 f3涨跌幅 f4涨跌额 f5成交量(手) f6成交额 f7振幅 f8换手 f9PE f10量比
# f12代码 f14名称 f15高 f16低 f17今开 f18昨收
fields = "f2,f3,f4,f5,f6,f7,f8,f10,f12,f14,f15,f16,f17,f18"
secids = "1.600519,1.510300,1.000001,0.399001,1.110076,0.127022"
url = f"https://push2.eastmoney.com/api/qt/ulist.np/get?secids={secids}&fields={fields}"
try:
    t0 = time.perf_counter()
    r = requests.get(url, headers=UA, timeout=10)
    data = r.json()
    print(f"  [ok] HTTP {r.status_code} {time.perf_counter()-t0:.2f}s total={data.get('data', {}).get('total')}")
    for item in (data.get("data") or {}).get("diff") or []:
        print(f"    {item}")
except Exception as e:  # noqa: BLE001
    print(f"  [fail] {type(e).__name__}: {str(e)[:200]}")

# ---------- 3. baostock 兜底 ----------
print("\n## 3. baostock 兜底通道")
try:
    import baostock as bs

    lg = bs.login()
    print(f"  login: code={lg.error_code} msg={lg.error_msg}")
    rs = bs.query_history_k_data_plus(
        "sh.600519", "date,code,open,high,low,close,preclose,volume,amount,pctChg",
        start_date="2026-09-01", end_date="2026-10-26", frequency="d", adjustflag="2")
    rows = []
    while rs.error_code == "0" and rs.next():
        rows.append(rs.get_row_data())
    print(f"  [ok] 股票日线 rows={len(rows)} fields={rs.fields}")
    for row in rows[-3:]:
        print(f"    {row}")
    rs2 = bs.query_history_k_data_plus(
        "sh.000001", "date,code,close,pctChg", start_date="2026-10-01",
        end_date="2026-10-26", frequency="d")
    rows2 = []
    while rs2.error_code == "0" and rs2.next():
        rows2.append(rs2.get_row_data())
    print(f"  [ok] 指数日线 rows={len(rows2)}；转债支持：baostock 不覆盖可转债（需确认）")
    bs.logout()
except ImportError:
    print("  [skip] baostock 未安装")
except Exception as e:  # noqa: BLE001
    print(f"  [fail] {type(e).__name__}: {str(e)[:200]}")

# ---------- 4. 现存活转债复验 ----------
print("\n## 4. 转债日线复验（sh110076 华海转债，现存活）")
try:
    t0 = time.perf_counter()
    df = ak.bond_zh_hs_cov_daily(symbol="sh110076")
    print(f"  [ok] {time.perf_counter()-t0:.2f}s shape={df.shape}")
    print(df.tail(3).to_string())
except Exception as e:  # noqa: BLE001
    print(f"  [fail] {type(e).__name__}: {str(e)[:200]}")

print("\n# 第三轮结束")
