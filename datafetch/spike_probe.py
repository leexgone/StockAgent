#!/usr/bin/env python3
"""P2-T1 · datafetch spike 探测脚本

目的：验证 akshare 对 股票 / ETF / 可转债 / 指数 四类标的的实时与日线数据、
交易日历、行业归属、转债强赎状态的**真实字段**，测量延迟与限频表现，
为 docs/04 附录的「akshare → ZM 契约」字段对照表提供原料。

用法：
    cd datafetch
    uv run spike_probe.py            # 全量探测
    uv run spike_probe.py --quick    # 跳过全市场快照类重接口

输出：stdout（markdown 风格分段），不写任何文件、不落盘。
说明：akshare 接口名随版本漂移，探针带候选名与自动发现（失败时按关键字
扫 dir(ak) 给出现行名称建议），探测失败本身也是有价值的结论。
"""

from __future__ import annotations

import argparse
import sys
import time
import traceback
from datetime import date, timedelta

sys.stdout.reconfigure(encoding="utf-8")

import akshare as ak  # noqa: E402
import pandas as pd  # noqa: E402

TODAY = date.today()
START = (TODAY - timedelta(days=45)).strftime("%Y%m%d")
END = TODAY.strftime("%Y%m%d")

SAMPLES = {
    "stock": {"code": "600519", "name": "贵州茅台", "mkt": "SH"},
    "etf": {"code": "510300", "name": "沪深300ETF", "mkt": "SH"},
    "cb": {"code": "110074", "name": "精达转债", "mkt": "SH"},
    "index": {"code": "000001", "name": "上证指数", "mkt": "SH"},
}

results: list[tuple[str, str, float, str]] = []  # (id, status, secs, shape)


def suggest(keyword: str, limit: int = 8) -> list[str]:
    """按关键字在 akshare 现行命名空间里找候选函数名。"""
    kws = [k for k in keyword.lower().replace("_", " ").split() if len(k) > 2]
    hits = []
    for name in dir(ak):
        low = name.lower()
        if any(k in low for k in kws) and not name.startswith("_"):
            hits.append(name)
    return hits[:limit]


def find_row(df: pd.DataFrame, code: str) -> pd.Series | None:
    """在快照 DataFrame 里找到 code 所在行（自动识别代码列）。"""
    for col in df.columns:
        if "代码" in str(col) or str(col).lower() in ("code", "symbol"):
            hit = df[df[col].astype(str).str.contains(code, na=False)]
            if len(hit):
                return hit.iloc[0]
    return None


def run(pid: str, desc: str, calls: list[tuple[str, dict]], heavy: bool = False,
        sample_code: str | None = None, quick: bool = False) -> pd.DataFrame | None:
    """执行一个探针：依次尝试候选 (函数名, kwargs)，第一个成功的输出详情。"""
    print(f"\n### {pid} · {desc}")
    if heavy and quick:
        print("  [skip] --quick 模式跳过重接口")
        results.append((pid, "SKIP", 0.0, "-"))
        return None
    last_err = None
    for fn_name, kwargs in calls:
        fn = getattr(ak, fn_name, None)
        if fn is None:
            alt = suggest(fn_name)
            print(f"  [miss] ak.{fn_name} 不存在；现行候选: {alt or '无'}")
            last_err = f"AttributeError: {fn_name}"
            continue
        t0 = time.perf_counter()
        try:
            df = fn(**kwargs)
            secs = time.perf_counter() - t0
            shape = getattr(df, "shape", None) or (len(df),)
            print(f"  [ok]   ak.{fn_name}({kwargs or ''})  {secs:.2f}s  shape={shape}")
            if isinstance(df, pd.DataFrame):
                print(f"  columns: {list(df.columns)}")
                if sample_code:
                    row = find_row(df, sample_code)
                    if row is not None:
                        print(f"  sample({sample_code}):")
                        for k, v in row.items():
                            print(f"    {k} = {v}")
                    else:
                        print(f"  sample({sample_code}): 未在结果中找到")
                else:
                    with pd.option_context("display.max_columns", None, "display.width", 200):
                        print(df.head(3).to_string())
            else:
                print(f"  value: {df}")
            results.append((pid, "OK", secs, str(shape)))
            return df
        except Exception as e:  # noqa: BLE001
            secs = time.perf_counter() - t0
            last_err = f"{type(e).__name__}: {e}"
            print(f"  [fail] ak.{fn_name}({kwargs or ''})  {secs:.2f}s  {last_err[:300]}")
            results.append((pid, "FAIL", secs, last_err[:120]))
    if last_err and "AttributeError" in str(last_err):
        print(f"  => 该数据类目在当前 akshare 版本下无可用入口（最后错误：{last_err}）")
    return None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="跳过全市场快照类重接口")
    ap.add_argument("--repeat", type=int, default=5, help="限频测试的连打次数")
    args = ap.parse_args()

    print(f"# akshare spike · {TODAY} · akshare={ak.__version__} pandas={pd.__version__} python={sys.version.split()[0]}")

    s, e, c, i = SAMPLES["stock"], SAMPLES["etf"], SAMPLES["cb"], SAMPLES["index"]

    print("\n## 一、实时 / 快照类")
    run("P01", "A股全市场实时快照（东财）", [("stock_zh_a_spot_em", {})],
        heavy=True, sample_code=s["code"], quick=args.quick)
    run("P02", "单只股票买卖盘/行情（东财）", [("stock_bid_ask_em", {"symbol": s["code"]})],
        sample_code=None)
    run("P03", "单只股票实时（雪球，备选源）", [("stock_individual_spot_xq", {"symbol": "SH" + s["code"]})])
    run("P04", "个股信息（行业/股本，sector 映射来源）", [("stock_individual_info_em", {"symbol": s["code"]})])
    run("P05", "ETF全市场实时快照（东财）", [("fund_etf_spot_em", {})],
        heavy=True, sample_code=e["code"], quick=args.quick)
    run("P06", "沪深可转债实时（新浪）", [("bond_zh_hs_cov_spot", {})],
        heavy=True, sample_code=c["code"], quick=args.quick)
    run("P07", "可转债比价表（转股溢价率等）", [("bond_zh_cov", {})],
        heavy=True, sample_code=c["code"], quick=args.quick)
    run("P08", "转债强赎状态（集思录）", [("bond_cb_redeem_jsl", {})])
    run("P09", "指数实时快照（东财）", [
        ("stock_zh_index_spot_em", {"symbol": "沪深重要指数"}),
        ("stock_zh_index_spot_em", {}),
        ("stock_zh_index_spot_sina", {}),
    ], sample_code=i["code"])

    print("\n## 二、日线 / 历史类")
    run("P10", "股票日线（东财，qfq）", [("stock_zh_a_hist", {
        "symbol": s["code"], "period": "daily", "start_date": START, "end_date": END, "adjust": "qfq"})])
    run("P11", "ETF日线（东财）", [("fund_etf_hist_em", {
        "symbol": e["code"], "period": "daily", "start_date": START, "end_date": END, "adjust": "qfq"})])
    run("P12", "可转债日线（新浪）", [("bond_zh_hs_cov_daily", {"symbol": "sh" + c["code"]})])
    run("P13", "指数日线（东财）", [("index_zh_a_hist", {
        "symbol": i["code"], "period": "daily", "start_date": START, "end_date": END})])

    print("\n## 三、辅助")
    run("P14", "交易日历（新浪，marketClock 节假日扩展来源）", [("tool_trade_date_hist_sina", {})])
    run("P15", "A股代码-名称表", [("stock_info_a_code_name", {})], heavy=True, quick=args.quick)

    print("\n## 四、限频与稳定性（连续调用，无间隔）")
    lat: list[tuple[str, float, str]] = []
    for n in range(args.repeat):
        t0 = time.perf_counter()
        try:
            ak.stock_bid_ask_em(symbol=s["code"])
            lat.append((f"bid_ask#{n+1}", time.perf_counter() - t0, "ok"))
        except Exception as ex:  # noqa: BLE001
            lat.append((f"bid_ask#{n+1}", time.perf_counter() - t0, f"{type(ex).__name__}"))
    if not args.quick:
        for n in range(2):
            t0 = time.perf_counter()
            try:
                ak.stock_zh_a_spot_em()
                lat.append((f"spot_em#{n+1}", time.perf_counter() - t0, "ok"))
            except Exception as ex:  # noqa: BLE001
                lat.append((f"spot_em#{n+1}", time.perf_counter() - t0, f"{type(ex).__name__}"))
    for name, secs, st in lat:
        print(f"  {name}: {secs:.2f}s {st}")

    print("\n## 汇总")
    print("| 探针 | 状态 | 耗时(s) | 形状/错误 |")
    print("|---|---|---|---|")
    for pid, st, secs, shape in results:
        print(f"| {pid} | {st} | {secs:.2f} | {shape} |")
    fails = [r for r in results if r[1] == "FAIL"]
    print(f"\n共 {len(results)} 项：OK {sum(1 for r in results if r[1]=='OK')}，"
          f"FAIL {len(fails)}，SKIP {sum(1 for r in results if r[1]=='SKIP')}")
    if fails:
        print("失败项：" + ", ".join(r[0] for r in fails))


if __name__ == "__main__":
    try:
        main()
    except Exception:  # noqa: BLE001
        traceback.print_exc()
        sys.exit(1)
