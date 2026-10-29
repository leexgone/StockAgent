"""行情源适配（P4-T3）——纯 I/O，每函数一个源，抛异常即失败（兜底编排在 service）。

通道选择沿用 spike 实测结论（`docs/04` 附录）：

- 实时：新浪 hq 直连为主（一次多标的、字段无需缩放、需 Referer）；东财 push2
  直连为备（**必须浏览器 UA**，akshare 的东财实时封装被指纹拦截，不用它）。
  实测确认股票/ETF/指数/转债四类共用同一 33/34 位布局（P4-T3 补测活券，
  spike 的两只转债样本恰好都是退市券，没覆盖到活券布局）。
- 日线：股票 新浪 `stock_zh_a_daily`(qfq) → baostock(qfq, P4 复测登录恢复) →
  akshare 东财 hist（间歇被拦，垫底；成交量手→股 ×100 归一）；
  ETF/转债 仅新浪系；指数 新浪 → baostock。
- 转债列表：akshare 新浪现券表（327 只，含北交所定转——按沪深覆盖范围过滤）
  + 集思录强赎表 join（低频，失败降级为强赎字段置空）。
- 日历：akshare 新浪全量交易日历（含未来日）→ baostock 区间日历。

行内 `status` 三态（风险清单第 4 条）：`ok / halted_or_delisted /
empty_or_delisted`；上游整行缺失另标 `no_data`。退市样本 110074（定格价）/
127022（空串）是回归基准。
"""

from __future__ import annotations

import re
import threading

import akshare as ak
import pandas as pd
import requests

from . import symbols as S

UA = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
                  "(KHTML, like Gecko) Chrome/130.0 Safari/537.36"
}
SINA_HQ_URL = "https://hq.sinajs.cn/list={codes}"
SINA_HEADERS = {**UA, "Referer": "https://finance.sina.com.cn"}
TENCENT_QT_URL = "https://qt.gtimg.cn/q={codes}"
_T_LINE_RE = re.compile(r'v_(\w+)="([^"]*)"')
_T_TS_RE = re.compile(r"^20\d{12}$")
EM_ULIST = "https://push2.eastmoney.com/api/qt/ulist.np/get"
# f2 最新 f3 涨跌幅% f5 成交量 f6 成交额 f12 代码 f14 名称 f15 高 f16 低 f17 今开 f18 昨收
EM_FIELDS = "f2,f3,f5,f6,f12,f14,f15,f16,f17,f18"
EM_TIMEOUT = 8
SINA_TIMEOUT = 8

_LINE_RE = re.compile(r'var hq_str_(\w+)="([^"]*)"')
_TAG_RE = re.compile(r"<[^>]+>")

_BS_LOCK = threading.Lock()  # baostock 是进程级全局 socket，登录/查询/登出必须串行


def _num(v) -> float | None:
    """宽松数值解析：''/'-'/'--'/'nan'/NaT → None。"""
    if v is None:
        return None
    s = str(v).strip()
    if not s or s in {"-", "--", "nan", "NaN", "None", "NaT"}:
        return None
    try:
        return float(s)
    except ValueError:
        return None


def _date_str(v) -> str | None:
    s = str(v)[:10] if v is not None else ""
    return s if re.match(r"^\d{4}-\d{2}-\d{2}$", s) else (s or None)


def _chg(price, prev) -> float | None:
    if price is None or not prev:
        return None
    return round((price - prev) / prev * 100, 4)


def _quote_row(symbol, *, source, status, name=None, price=None, prev=None, open=None,
               high=None, low=None, volume=None, amount=None, date_=None, time_=None):
    """统一实时行形状（所有通道出同一组键，契约消费方不感知源差异）。"""
    return {
        "symbol": symbol, "code": S.code_of(symbol), "mkt": S.mkt_of(symbol),
        "kind": S.infer_kind(symbol), "name": name,
        "price": price, "prev": prev, "open": open, "high": high, "low": low,
        "volume": volume, "amount": amount, "chg_pct": _chg(price, prev),
        "date": date_, "time": time_, "status": status, "source": source,
    }


# ---------- 实时 ----------

def _parse_sina_hq(text: str, symbols: list[str]) -> list[dict]:
    """新浪 hq 响应解析（纯函数，测试直接喂样本文本）。33/34 位股票系布局。"""
    got = dict(_LINE_RE.findall(text))
    rows = []
    for sym in symbols:
        if sym not in got:
            rows.append(_quote_row(sym, source="sina-hq", status="no_data"))
            continue
        payload = got[sym]
        f = payload.split(",")
        if not payload or len(f) < 32:
            # 摘牌/退市：整行空串（127022 样本）或残行
            rows.append(_quote_row(sym, source="sina-hq", status="empty_or_delisted",
                                   name=f[0] if f and f[0] else None))
            continue
        price, prev = _num(f[3]), _num(f[2])
        open_, volume = _num(f[1]), _num(f[8])
        if not f[0] and not price:
            # 空名 + 零价 = 新浪未收录该标的（如中证转债指数 sh000832——B14）：
            # 不出行，service 的逐标的补齐会把它交给下一源，而不是伪装 halted
            continue
        halted = (not price) or (not open_ and not volume)
        rows.append(_quote_row(
            sym, source="sina-hq",
            status="halted_or_delisted" if halted else "ok",
            name=f[0], price=price, prev=prev, open=open_,
            high=_num(f[4]), low=_num(f[5]), volume=volume, amount=_num(f[9]),
            date_=f[30] if len(f) > 30 else None, time_=f[31] if len(f) > 31 else None,
        ))
    return rows


def sina_hq_quote(symbols: list[str]) -> list[dict]:
    """新浪 hq 直连：一次请求多标的。"""
    r = requests.get(SINA_HQ_URL.format(codes=",".join(symbols)),
                     headers=SINA_HEADERS, timeout=SINA_TIMEOUT)
    r.encoding = "gbk"
    return _parse_sina_hq(r.text, symbols)


def _parse_tencent_qt(text: str, symbols: list[str]) -> list[dict]:
    """腾讯 qt 响应解析（纯函数）。`~` 分隔；盘口段长度随标的类型变化，
    绝对下标不可靠——价/昨/开/量取头部固定位（1–6），其余以 **14 位时间戳
    字段为锚**相对取位：+1 涨跌 +2 涨跌% +3 高 +4 低 +5「最新/量(手)/额(元)」。
    （布局按 2026-10-29 指数/股票/ETF/转债四类实测样本校准，B14。）"""
    got = dict(_T_LINE_RE.findall(text))
    rows = []
    for sym in symbols:
        payload = got.get(sym)
        if not payload:
            rows.append(_quote_row(sym, source="tencent-qt", status="no_data"))
            continue
        f = payload.split("~")
        if len(f) < 7:
            rows.append(_quote_row(sym, source="tencent-qt", status="no_data"))
            continue
        name = f[1] or None
        price, prev, open_ = _num(f[3]), _num(f[4]), _num(f[5])
        vol_lots = _num(f[6])
        ts_i = next((i for i, x in enumerate(f) if _T_TS_RE.match(x)), None)
        date_ = time_ = high = low = amount = None
        if ts_i is not None:
            ts = f[ts_i]
            date_ = f"{ts[:4]}-{ts[4:6]}-{ts[6:8]}"
            time_ = f"{ts[8:10]}:{ts[10:12]}:{ts[12:14]}"
            if len(f) > ts_i + 4:
                high, low = _num(f[ts_i + 3]), _num(f[ts_i + 4])
            if len(f) > ts_i + 5 and "/" in f[ts_i + 5]:
                amount = _num(f[ts_i + 5].split("/")[2])  # 元，与新浪 f[9] 同单位
        if not name and not price:
            continue  # 源未收录：不出行（service 逐标的补齐）
        kind = S.infer_kind(sym)
        # 量单位与东财同款约定：股票/ETF/转债 手→股/份/张 ×100；指数原样透传（B14 实测：
        # sh000001 腾讯量 443564144 手与新浪指数字段逐位一致）
        volume = vol_lots * 100 if (vol_lots is not None and kind != "index") else vol_lots
        halted = not price
        rows.append(_quote_row(
            sym, source="tencent-qt",
            status="halted_or_delisted" if halted else "ok",
            name=name, price=price, prev=prev, open=open_,
            high=high, low=low, volume=volume, amount=amount,
            date_=date_, time_=time_,
        ))
    return rows


def tencent_qt_quote(symbols: list[str]) -> list[dict]:
    """腾讯 qt 直连（B14 指数 spot 补位；股票/ETF/转债同样覆盖，作通用二通道）。"""
    r = requests.get(TENCENT_QT_URL.format(codes=",".join(symbols)),
                     headers=UA, timeout=EM_TIMEOUT)
    r.encoding = "gbk"
    return _parse_tencent_qt(r.text, symbols)


def em_push2_quote(symbols: list[str]) -> list[dict]:
    """东财 push2 直连备份通道。fltt=2 让价格/百分比直接给小数（免手工 ÷100）。"""
    secids = ",".join(("1." if s.startswith("sh") else "0.") + S.code_of(s) for s in symbols)
    r = requests.get(EM_ULIST, headers=UA, timeout=EM_TIMEOUT,
                     params={"secids": secids, "fields": EM_FIELDS, "fltt": 2, "invt": 2})
    diff = (r.json().get("data") or {}).get("diff") or []
    by_code = {str(it.get("f12")): it for it in diff}
    rows = []
    for sym in symbols:
        it = by_code.get(S.code_of(sym))
        if it is None:
            rows.append(_quote_row(sym, source="em-push2", status="no_data"))
            continue
        price, prev, open_ = _num(it.get("f2")), _num(it.get("f18")), _num(it.get("f17"))
        kind = S.infer_kind(sym)
        vol = _num(it.get("f5"))
        # 东财成交量单位：股票/ETF/转债为手（10 股/份/张），指数另有口径且契约不消费——
        # 非指数统一 ×100 对齐新浪系的「股/份」约定；指数原样透传并留注释（P4-T4 校准）。
        volume = vol * 100 if (vol is not None and kind != "index") else vol
        halted = not price
        rows.append(_quote_row(
            sym, source="em-push2", status="halted_or_delisted" if halted else "ok",
            name=it.get("f14"), price=price, prev=prev, open=open_,
            high=_num(it.get("f15")), low=_num(it.get("f16")),
            volume=volume, amount=_num(it.get("f6")),
        ))
    return rows


# ---------- 日线 ----------

def _filter_df(df: pd.DataFrame, start: str, end: str) -> list[dict]:
    """新浪系日线公共段：date 列过滤区间，输出统一 OHLCV(+amount 若有)。"""
    d = pd.to_datetime(df["date"]).dt.strftime("%Y-%m-%d")
    out = []
    for (_, r), ds in zip(df.iterrows(), d):
        if not (start <= ds <= end):
            continue
        row = {"date": ds, "open": _num(r["open"]), "high": _num(r["high"]),
               "low": _num(r["low"]), "close": _num(r["close"]), "volume": _num(r["volume"])}
        if "amount" in df.columns:
            row["amount"] = _num(r["amount"])
        out.append(row)
    return out


def sina_stock_daily(symbol: str, start: str, end: str) -> list[dict]:
    df = ak.stock_zh_a_daily(symbol=symbol, adjust="qfq")
    return _filter_df(df, start, end)


def sina_etf_daily(symbol: str, start: str, end: str) -> list[dict]:
    return _filter_df(ak.fund_etf_hist_sina(symbol=symbol), start, end)


def sina_index_daily(symbol: str, start: str, end: str) -> list[dict]:
    return _filter_df(ak.stock_zh_index_daily(symbol=symbol), start, end)


def sina_cb_daily(symbol: str, start: str, end: str) -> list[dict]:
    return _filter_df(ak.bond_zh_hs_cov_daily(symbol=symbol), start, end)


def em_stock_hist(symbol: str, start: str, end: str) -> list[dict]:
    """akshare 东财日线（链尾垫底；间歇被指纹拦，故不排前面）。成交量手→股。"""
    df = ak.stock_zh_a_hist(symbol=S.code_of(symbol), period="daily",
                            start_date=start.replace("-", ""), end_date=end.replace("-", ""),
                            adjust="qfq")
    out = []
    for _, r in df.iterrows():
        vol = _num(r["成交量"])
        out.append({"date": str(r["日期"])[:10], "open": _num(r["开盘"]),
                    "high": _num(r["最高"]), "low": _num(r["最低"]),
                    "close": _num(r["收盘"]),
                    "volume": vol * 100 if vol is not None else None,
                    "amount": _num(r["成交额"])})
    return out


def _baostock_k(symbol: str, start: str, end: str, *, index: bool) -> list[dict]:
    """baostock 日线（股票 qfq / 指数不复权）。全局 socket + 锁，登录-查询-登出串行化。"""
    import baostock as bs

    code = f"{symbol[:2]}.{S.code_of(symbol)}"
    with _BS_LOCK:
        lg = bs.login()
        if lg.error_code != "0":
            raise RuntimeError(f"baostock login {lg.error_code}: {lg.error_msg}")
        try:
            kwargs = dict(code=code, fields="date,open,high,low,close,volume,amount",
                          start_date=start, end_date=end, frequency="d")
            if not index:
                kwargs["adjustflag"] = "2"  # 前复权，与新浪/东财 qfq 链对齐
            rs = bs.query_history_k_data_plus(**kwargs)
            if rs.error_code != "0":
                raise RuntimeError(f"baostock query {rs.error_code}: {rs.error_msg}")
            data = []
            while rs.next():
                data.append(rs.get_row_data())
        finally:
            bs.logout()
    out = []
    for ds, o, h, l, c, v, a in data:
        out.append({"date": ds, "open": _num(o), "high": _num(h), "low": _num(l),
                    "close": _num(c), "volume": _num(v), "amount": _num(a)})
    return out


def baostock_stock_daily(symbol: str, start: str, end: str) -> list[dict]:
    return _baostock_k(symbol, start, end, index=False)


def baostock_index_daily(symbol: str, start: str, end: str) -> list[dict]:
    return _baostock_k(symbol, start, end, index=True)


# ---------- 转债列表 ----------

def cb_spot_sina() -> list[dict]:
    """转债现券实时表（akshare 新浪，~327 行）。北交所定转超出覆盖范围，过滤。"""
    df = ak.bond_zh_hs_cov_spot()
    out = []
    for _, r in df.iterrows():
        sym = str(r["symbol"]).lower()
        if sym[:2] not in ("sh", "sz"):
            continue
        price = _num(r["trade"])
        out.append({
            "symbol": sym, "code": str(r["code"]), "mkt": sym[:2].upper(),
            "kind": "cb", "name": str(r["name"]),
            "price": price, "prev": _num(r["settlement"]),
            "open": _num(r["open"]), "high": _num(r["high"]), "low": _num(r["low"]),
            "volume": _num(r["volume"]), "amount": _num(r["amount"]),
            "chg_pct": _num(r["changepercent"]),
            "ticktime": str(r.get("ticktime") or "") or None,
            "status": "ok" if price else "halted_or_delisted",
            "spot_source": "sina-cb-spot",
        })
    return out


def _strip_html(v) -> str | None:
    s = _TAG_RE.sub(" ", str(v or ""))
    s = re.sub(r"\s+", " ", s).replace(" ! ", " ").strip()
    return s or None


def cb_redeem_jsl() -> dict[str, dict]:
    """集思录强赎表 → 按 6 位代码索引。列名按 akshare 1.19.3 实测（P08 全字段）。"""
    df = ak.bond_cb_redeem_jsl()
    out: dict[str, dict] = {}
    for _, r in df.iterrows():
        code = str(r["代码"]).zfill(6)
        out[code] = {
            "stock_code": str(r["正股代码"]).zfill(6),
            "stock_name": str(r["正股名称"]),
            "convert_price": _num(r["转股价"]),
            "trigger_pct": _num(r["强赎触发比"]),
            "trigger_price": _num(r["强赎触发价"]),
            "stock_price": _num(r["正股价"]),
            "redeem_price": _num(r["强赎价"]),
            "last_trade_date": _date_str(r["最后交易日"]),
            "maturity_date": _date_str(r["到期日"]),
            "scale": _num(r["规模"]),
            "scale_left": _num(r["剩余规模"]),
            "redeem_status": str(r["强赎状态"] or "") or None,
            "redeem_hint": _strip_html(r["强赎天计数"]),
        }
    return out


# ---------- 交易日历 ----------

def calendar_sina(start: str, end: str) -> list[str]:
    df = ak.tool_trade_date_hist_sina()
    ds = [str(d)[:10] for d in df["trade_date"]]
    return [d for d in ds if start <= d <= end]


def calendar_baostock(start: str, end: str) -> list[str]:
    import baostock as bs

    with _BS_LOCK:
        lg = bs.login()
        if lg.error_code != "0":
            raise RuntimeError(f"baostock login {lg.error_code}: {lg.error_msg}")
        try:
            rs = bs.query_trade_dates(start_date=start, end_date=end)
            if rs.error_code != "0":
                raise RuntimeError(f"baostock calendar {rs.error_code}: {rs.error_msg}")
            days = []
            while rs.next():
                d, is_open = rs.get_row_data()
                if is_open == "1":
                    days.append(d)
        finally:
            bs.logout()
    return days
