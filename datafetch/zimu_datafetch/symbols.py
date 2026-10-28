"""代码与日期归一（P4-T3）。

**B7 在 sidecar 侧的落实**：`/quote` `/kline` 的标的必须带显式市场前缀
（`sh600519`/`sz159915`/`bj830799`，大小写不限）；裸代码一律 400 拒绝，
不再像 spike 那样按号段猜前缀（`5` 开头深市 ETF 会猜错）。调用方（Rust 适配器 /
前端）手里都有契约的显式 `mkt`，由它们拼好前缀再来。

号段推断只剩一个用途：**kline 兜底路由**——用户没传 `kind` 时决定走哪条日线链，
属于 sidecar 内部实现，响应里有 `kind_routed` 标注；不参与任何业务口径。
"""

from __future__ import annotations

import re
from datetime import date, datetime, timedelta

from .errors import bad_params, bad_symbol

_PREFIXES = ("sh", "sz", "bj")
_KINDS = ("stock", "etf", "index", "cb")
_DATE_COMPACT = re.compile(r"^\d{8}$")
_DATE_DASH = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def normalize_symbol(raw: str) -> str:
    """`SH600519` → `sh600519`；缺前缀/位数不对 → BAD_SYMBOL。"""
    c = (raw or "").strip().lower()
    if not c:
        raise bad_symbol(raw, "为空")
    if len(c) != 8 or not c.startswith(_PREFIXES) or not c[2:].isdigit():
        raise bad_symbol(raw, "必须带显式市场前缀 sh/sz/bj + 6 位数字（B7：不做号段猜测）")
    return c


def mkt_of(symbol: str) -> str:
    return symbol[:2].upper()


def code_of(symbol: str) -> str:
    return symbol[2:]


def infer_kind(symbol: str) -> str:
    """号段路由表（仅内部用）：index / cb / etf / stock / other。"""
    code = symbol[2:]
    if symbol.startswith("sh000") or symbol.startswith("sz399"):
        return "index"
    if code.startswith(("110", "111", "113", "118", "123", "127", "128")):
        return "cb"
    if symbol.startswith("sh") and code[0] == "5":
        return "etf"
    if symbol.startswith("sz") and code[:2] in ("15", "16", "17", "18"):
        return "etf"
    if symbol.startswith("bj"):
        return "other"
    return "stock"


def kind_for_kline(symbol: str, explicit: str | None) -> tuple[str, str]:
    """→ (kind, kind_routed)。显式 kind 优先；推断兜底。"""
    if explicit is not None:
        k = explicit.strip().lower()
        if k not in _KINDS:
            raise bad_params(f"kind 只支持 {'/'.join(_KINDS)}，收到 {explicit!r}")
        return k, "explicit"
    k = infer_kind(symbol)
    if k == "other":
        raise bad_params(f"无法由代码推断 {symbol} 的类型（北交所超出覆盖范围），请显式传 kind")
    return k, "inferred"


def normalize_date(raw: str, param: str) -> str:
    """接受 `YYYYMMDD` / `YYYY-MM-DD`，返回 ISO `YYYY-MM-DD`；非法 400。"""
    s = (raw or "").strip()
    if _DATE_COMPACT.match(s):
        s = f"{s[:4]}-{s[4:6]}-{s[6:]}"
    elif not _DATE_DASH.match(s):
        raise bad_params(f"{param} 需为 YYYYMMDD 或 YYYY-MM-DD，收到 {raw!r}")
    try:
        return datetime.strptime(s, "%Y-%m-%d").date().isoformat()
    except ValueError:
        raise bad_params(f"{param} 不是有效日期，收到 {raw!r}") from None


def default_range(days_back: int) -> tuple[str, str]:
    today = date.today()
    return (today - timedelta(days=days_back)).isoformat(), today.isoformat()
