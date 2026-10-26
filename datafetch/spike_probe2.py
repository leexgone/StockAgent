#!/usr/bin/env python3
"""P2-T1 · datafetch spike 第二轮：东财故障定位 + 替代源验证

第一轮（spike_probe.py）结论：东财实时系接口全部 RemoteDisconnected，
但东财历史日线可通；新浪系（指数实时/转债实时/转债日线/日历）全部可通。
本轮回答三个问题：
  1. 东财是网络层拒绝还是 akshare 封装问题？（带浏览器 UA 直连 API 验证）
  2. 失败项有没有新浪/同花顺替代？（A股实时快照、ETF 实时/日线、指数日线、行业归属）
  3. hq.sinajs.cn 直连（单次请求拿多标的实时价）是否可作 sidecar 的轻量主通道？
"""

from __future__ import annotations

import sys
import time

sys.stdout.reconfigure(encoding="utf-8")

import requests  # noqa: E402

UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36"}

print("# spike 第二轮 · 东财诊断与替代源")

# ---------- 1. 东财直连诊断 ----------
print("\n## 1. 东财直连诊断（带浏览器 UA，绕开 akshare 封装）")
em_tests = [
    ("push2 实时", "https://push2.eastmoney.com/api/qt/ulist.np/get?secids=1.600519,1.510300&fields=f2,f3,f4,f12,f14"),
    ("push2his 日线", "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=1.600519&klt=101&fqt=1&beg=20261001&end=20500101&fields1=f1,f2,f3&fields2=f51,f53"),
]
for name, url in em_tests:
    t0 = time.perf_counter()
    try:
        r = requests.get(url, headers=UA, timeout=10)
        body = r.text[:200].replace("\n", " ")
        print(f"  [{name}] HTTP {r.status_code} {time.perf_counter()-t0:.2f}s :: {body}")
    except Exception as e:  # noqa: BLE001
        print(f"  [{name}] FAIL {time.perf_counter()-t0:.2f}s :: {type(e).__name__}: {e}")

# ---------- 2. 新浪 hq 直连（轻量多标的一次拿） ----------
print("\n## 2. 新浪 hq.sinajs.cn 直连（股票+ETF+指数+转债一次请求）")
codes = "sh600519,sh510300,sh000001,sh110074,sz127022"
try:
    r = requests.get(f"https://hq.sinajs.cn/list={codes}",
                     headers={**UA, "Referer": "https://finance.sina.com.cn"}, timeout=10)
    r.encoding = "gbk"
    for line in r.text.strip().split("\n"):
        print(f"  {line[:160]}")
except Exception as e:  # noqa: BLE001
    print(f"  FAIL :: {type(e).__name__}: {e}")

# ---------- 3. akshare 替代源 ----------
print("\n## 3. akshare 替代源")
import akshare as ak  # noqa: E402
import pandas as pd  # noqa: E402


def probe(desc: str, fn, **kw):
    print(f"\n### {desc}")
    t0 = time.perf_counter()
    try:
        df = fn(**kw)
        secs = time.perf_counter() - t0
        print(f"  [ok] {secs:.2f}s shape={getattr(df, 'shape', None)}")
        if isinstance(df, pd.DataFrame):
            print(f"  columns: {list(df.columns)}")
            with pd.option_context("display.max_columns", None, "display.width", 200):
                print(df.head(3).to_string())
        return df
    except Exception as e:  # noqa: BLE001
        print(f"  [fail] {time.perf_counter()-t0:.2f}s {type(e).__name__}: {str(e)[:200]}")
        return None


# 3.1 A股全市场快照（新浪）——sidecar 轮询候选
spot = probe("A股全市场实时快照（新浪，全量翻页较慢）", getattr(ak, "stock_zh_a_spot", None))
if spot is not None:
    row = spot[spot["代码"].astype(str).str.contains("600519")]
    if len(row):
        print("  sample(sh600519):")
        for k, v in row.iloc[0].items():
            print(f"    {k} = {v}")

# 3.2 东财实时重试一次（验证间歇性）
probe("东财单只实时重试（验证是否间歇性）", getattr(ak, "stock_bid_ask_em", None), symbol="600519")

# 3.3 ETF 实时（同花顺）
probe("ETF全市场实时（同花顺）", getattr(ak, "fund_etf_spot_ths", None))

# 3.4 ETF 日线（新浪）
probe("ETF日线（新浪）", getattr(ak, "fund_etf_hist_sina", None), symbol="sh510300")

# 3.5 指数日线（新浪 / 东财备选）
probe("指数日线（新浪）", getattr(ak, "stock_zh_index_daily", None), symbol="sh000001")
probe("指数日线（东财）", getattr(ak, "stock_zh_index_daily_em", None), symbol="sh000001")

# 3.6 行业归属（sector 映射来源）
probe("行业板块列表（新浪）", getattr(ak, "stock_sector_spot", None), indicator="新浪行业")
probe("行业板块列表（东财）", getattr(ak, "stock_board_industry_name_em", None))

# 3.7 转债现存活样本（替换演示组合里已退市的 110074 用）
cb = probe("转债实时（新浪）前 8 行——挑现存活代码", getattr(ak, "bond_zh_hs_cov_spot", None))
if cb is not None and len(cb) >= 8:
    with pd.option_context("display.max_columns", None, "display.width", 200):
        print(cb.head(8).to_string())

print("\n# 第二轮结束")
