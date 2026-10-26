"""datafetch sidecar —— P2-T1 spike 最小骨架（勿当生产代码）

验证「uv + FastAPI + akshare/直连 HTTP」的 sidecar 形态。生产化在 P4-T3
（缓存、限频、baostock 兜底、错误契约、/cb_list 等正式端点）。

通道选择依据 spike 实测（2026-10-26，详见 docs/04 附录）：
- 实时主通道：新浪 hq.sinajs.cn 直连——一次请求多标的（股票/ETF/指数/转债），字段无需缩放。
- 实时备通道：东财 push2 ulist 直连（必须带浏览器 UA；akshare 的东财实时封装被指纹拒绝）。
- 日线：股票→akshare stock_zh_a_hist（东财，spike 时通）；ETF→fund_etf_hist_sina；
        指数→stock_zh_index_daily；转债→bond_zh_hs_cov_daily（后三者均新浪，实测通）。

运行：
    cd datafetch
    uv run uvicorn app:app --host 127.0.0.1 --port 8300
    curl "http://127.0.0.1:8300/quote?codes=sh600519,sh510300,sh000001"
"""

from __future__ import annotations

import re
from datetime import date, timedelta

import akshare as ak
import pandas as pd
import requests
from fastapi import FastAPI, HTTPException, Query

app = FastAPI(title="zimu-datafetch", version="0.0.1-spike")

UA = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
                  "(KHTML, like Gecko) Chrome/130.0 Safari/537.36"
}
SINA_HQ = "https://hq.sinajs.cn/list={codes}"
SINA_REFERER = {"Referer": "https://finance.sina.com.cn"}
EM_ULIST = "https://push2.eastmoney.com/api/qt/ulist.np/get"
# f2最新 f3涨跌幅 f4涨跌额 f5成交量(手) f6成交额(元) f12代码 f14名称 f15高 f16低 f17今开 f18昨收
# 价格与百分比字段为 ×100 的缩放整数
EM_FIELDS = "f2,f3,f4,f5,f6,f12,f14,f15,f16,f17,f18"

_LINE_RE = re.compile(r'var hq_str_(\w+)="([^"]*)"')


# ---------- 工具 ----------

def _normalize_symbol(code: str) -> str:
    """把入参规整为带市场前缀的 symbol（sh600519）。

    spike 权宜：裸代码按号段猜前缀。生产版必须用 ZM 契约的显式 `mkt` 字段（backlog B7）。
    """
    c = code.strip().lower()
    if c.startswith(("sh", "sz", "bj")):
        return c
    if c.startswith(("5", "6", "9")) or c.startswith(("110", "113", "118")):
        return "sh" + c
    return "sz" + c


def _infer_kind(symbol: str) -> str:
    code = symbol[2:]
    if symbol.startswith("sh000") or symbol.startswith("sz399"):
        return "index"
    if code.startswith(("110", "113", "118", "123", "127", "128")):
        return "cb"
    if code.startswith(("51", "58", "15", "16", "18")):
        return "etf"
    return "stock"


# ---------- 实时行情 ----------

def _quote_sina(symbols: list[str]) -> list[dict]:
    r = requests.get(SINA_HQ.format(codes=",".join(symbols)),
                     headers={**UA, **SINA_REFERER}, timeout=8)
    r.encoding = "gbk"
    out = []
    for sym, payload in _LINE_RE.findall(r.text):
        f = payload.split(",")
        if len(f) < 32 or not payload:
            out.append({"symbol": sym, "status": "empty_or_delisted", "source": "sina-hq"})
            continue
        price, prev = float(f[3]), float(f[2])
        halted = (price == 0) or (float(f[1]) == 0 and float(f[8]) == 0)
        out.append({
            "symbol": sym, "code": sym[2:], "mkt": sym[:2].upper(), "name": f[0],
            "price": price, "prev": prev,
            "open": float(f[1]), "high": float(f[4]), "low": float(f[5]),
            "volume": float(f[8]), "amount": float(f[9]),
            "chg_pct": round((price - prev) / prev * 100, 4) if prev else None,
            "date": f[30], "time": f[31],
            "status": "halted_or_delisted" if halted else "ok",
            "source": "sina-hq",
        })
    return out


def _quote_em(symbols: list[str]) -> list[dict]:
    """东财备份通道：价格/百分比字段是 ×100 缩放整数。指数缩放未验证（第三轮补）。"""
    secids = ",".join(("1." if s.startswith("sh") else "0.") + s[2:] for s in symbols)
    r = requests.get(EM_ULIST, headers=UA, params={"secids": secids, "fields": EM_FIELDS}, timeout=8)
    diff = (r.json().get("data") or {}).get("diff") or []
    out = []
    for it in diff:
        out.append({
            "code": it.get("f12"), "name": it.get("f14"),
            "price": it.get("f2", 0) / 100, "prev": it.get("f18", 0) / 100,
            "open": it.get("f17", 0) / 100, "high": it.get("f15", 0) / 100,
            "low": it.get("f16", 0) / 100,
            "chg_pct": it.get("f3", 0) / 100,
            "volume": it.get("f5"), "amount": it.get("f6"),
            "status": "ok", "source": "em-push2",
        })
    return out


@app.get("/quote")
def quote(codes: str = Query(..., description="逗号分隔，sh600519 / 600519 均可")) -> dict:
    symbols = [_normalize_symbol(c) for c in codes.split(",") if c.strip()]
    if not symbols:
        raise HTTPException(400, "codes 为空")
    try:
        rows = _quote_sina(symbols)
    except Exception:  # noqa: BLE001
        try:
            rows = _quote_em(symbols)
        except Exception as e:  # noqa: BLE001
            raise HTTPException(502, f"行情源均不可用: {type(e).__name__}: {e}") from e
    return {"quotes": rows}


# ---------- 日线 ----------

def _kline_stock(symbol: str, start: str, end: str) -> list[dict]:
    """股票日线：新浪主（实测稳定、含换手率/流通股本），东财备（间歇被拦）。"""
    t0, t1 = pd.to_datetime(start), pd.to_datetime(end)
    try:
        df = ak.stock_zh_a_daily(symbol=symbol, adjust="qfq")
        df = df[(pd.to_datetime(df["date"]) >= t0) & (pd.to_datetime(df["date"]) <= t1)]
        return [{"date": str(r["date"]), "open": float(r["open"]), "high": float(r["high"]),
                 "low": float(r["low"]), "close": float(r["close"]), "volume": float(r["volume"])}
                for _, r in df.iterrows()]
    except Exception:  # noqa: BLE001
        df = ak.stock_zh_a_hist(symbol=symbol[2:], period="daily", start_date=start,
                                end_date=end, adjust="qfq")
        return [{"date": str(r["日期"]), "open": float(r["开盘"]), "high": float(r["最高"]),
                 "low": float(r["最低"]), "close": float(r["收盘"]), "volume": float(r["成交量"])}
                for _, r in df.iterrows()]


def _kline_sina_df(symbol: str, fn) -> list[dict]:
    df = fn(symbol=symbol)
    df = df.tail(400)
    return [{"date": str(r["date"]), "open": float(r["open"]), "high": float(r["high"]),
             "low": float(r["low"]), "close": float(r["close"]), "volume": float(r["volume"])}
            for _, r in df.iterrows()]


@app.get("/kline/{code}")
def kline(code: str, kind: str | None = None,
          start: str = (date.today() - timedelta(days=400)).strftime("%Y%m%d"),
          end: str = date.today().strftime("%Y%m%d")) -> dict:
    symbol = _normalize_symbol(code)
    k = kind or _infer_kind(symbol)
    try:
        if k == "stock":
            rows = _kline_stock(symbol, start, end)
        elif k == "etf":
            rows = _kline_sina_df(symbol, ak.fund_etf_hist_sina)
        elif k == "index":
            rows = _kline_sina_df(symbol, ak.stock_zh_index_daily)
        elif k == "cb":
            rows = _kline_sina_df(symbol, ak.bond_zh_hs_cov_daily)
        else:
            raise HTTPException(400, f"未知 kind: {k}")
    except HTTPException:
        raise
    except Exception as e:  # noqa: BLE001
        raise HTTPException(502, f"日线获取失败({k}/{symbol}): {type(e).__name__}: {e}") from e
    return {"symbol": symbol, "kind": k, "count": len(rows), "kline": rows}


# ---------- 辅助 ----------

@app.get("/calendar")
def calendar(days: int = 30) -> dict:
    df = ak.tool_trade_date_hist_sina()
    dates = [str(d) for d in df["trade_date"].tail(days)]
    return {"count": len(dates), "trade_dates": dates}


@app.get("/healthz")
def healthz() -> dict:
    return {"ok": True, "akshare": ak.__version__, "stage": "P2-T1 spike"}
