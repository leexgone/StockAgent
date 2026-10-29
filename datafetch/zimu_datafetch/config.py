"""运行时配置（P4-T3）。

默认值全部来自 `docs/04` 附录的 spike 实测结论（限频表现、通道选择），
每一项都可以用环境变量覆盖（前缀 `ZIMU_DF_`）。

源名字符串是**契约**：熔断状态、限频闸、`ZIMU_DF_DISABLE` 故障注入、
`/healthz` 观测都按这些名字对齐，改名等于改契约。
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field

# ---------- 源名（熔断 / 限频 / 禁用注入的标识） ----------
SINA_HQ = "sina_hq"                  # 实时主通道：新浪 hq.sinajs.cn 直连
TENCENT_QT = "tencent_qt"            # 实时二通道：腾讯 qt.gtimg.cn 直连（B14：指数 spot 补位——新浪不 carry 中证转债等指数）
EM_PUSH2 = "em_push2"                # 实时备通道：东财 push2 ulist 直连（UA）
SINA_STOCK_DAILY = "sina_stock_daily"
SINA_ETF_DAILY = "sina_etf_daily"
SINA_INDEX_DAILY = "sina_index_daily"
SINA_CB_DAILY = "sina_cb_daily"
EM_STOCK_HIST = "em_stock_hist"      # akshare 东财封装（间歇被拦，第三备胎）
BAOSTOCK_DAILY = "baostock_daily"    # P4 复测登录成功，进股票日线兜底链
BAOSTOCK_INDEX = "baostock_index"
SINA_CB_SPOT = "sina_cb_spot"        # 转债全量实时表（akshare 新浪系）
JSL_CB_REDEEM = "jsl_cb_redeem"      # 集思录强赎表（低频）
SINA_CALENDAR = "sina_calendar"
BAOSTOCK_CALENDAR = "baostock_calendar"

# 上游最小请求间隔（秒）：同源两次外呼之间强制等待（spike 风险清单第 5 条）。
# 新浪系与集思录按实测/礼貌值给；全市场快照类本就不进分钟级轮询（缓存兜住）。
DEFAULT_GATES: dict[str, float] = {
    SINA_HQ: 0.2,
    TENCENT_QT: 0.5,
    EM_PUSH2: 0.5,
    SINA_STOCK_DAILY: 0.5,
    SINA_ETF_DAILY: 0.5,
    SINA_INDEX_DAILY: 0.5,
    SINA_CB_DAILY: 0.5,
    EM_STOCK_HIST: 1.0,
    BAOSTOCK_DAILY: 1.0,
    BAOSTOCK_INDEX: 1.0,
    SINA_CB_SPOT: 2.0,
    JSL_CB_REDEEM: 2.0,
    SINA_CALENDAR: 5.0,
    BAOSTOCK_CALENDAR: 1.0,
}


def _env_f(name: str, default: float) -> float:
    try:
        return float(os.environ[name])
    except (KeyError, ValueError):
        return default


def _env_i(name: str, default: int) -> int:
    try:
        return int(os.environ[name])
    except (KeyError, ValueError):
        return default


@dataclass
class Settings:
    """sidecar 运行参数。生产从 `from_env()` 构造；测试直接实例化。"""

    # 缓存 TTL（秒）
    quote_ttl: float = 3.0        # 盘中行情：一次上游请求扇出多个调用方
    kline_ttl: float = 300.0      # 日线（含当日 bar）：5 分钟够新
    cb_ttl: float = 300.0         # 转债列表：全表重接口，不进分钟级轮询
    calendar_ttl: float = 21600.0 # 交易日历：半天缓存

    # 熔断：连续失败 N 次 → open 冷却 T 秒，期间直接跳源（东财被拦时快速失败，适合切源而非等待）
    breaker_threshold: int = 3
    breaker_cooloff: float = 60.0

    # 上游限频闸（秒/次）；未登记名字的源用 gate_default
    gates: dict[str, float] = field(default_factory=lambda: dict(DEFAULT_GATES))
    gate_default: float = 0.5

    # 故障注入：逗号分隔源名，这些源直接视为不可用（验收「主源故障时降级不崩」用）
    disabled: frozenset[str] = frozenset()

    # 单次 /quote 请求最多合并多少标的（新浪 hq 一次多标的，50 足够持仓+自选一轮）
    max_quote_symbols: int = 50

    @classmethod
    def from_env(cls) -> "Settings":
        disabled = frozenset(
            x.strip() for x in os.getenv("ZIMU_DF_DISABLE", "").split(",") if x.strip()
        )
        gates = dict(DEFAULT_GATES)
        for name in list(gates):
            gates[name] = _env_f(f"ZIMU_DF_GATE_{name.upper()}", gates[name])
        return cls(
            quote_ttl=_env_f("ZIMU_DF_QUOTE_TTL", 3.0),
            kline_ttl=_env_f("ZIMU_DF_KLINE_TTL", 300.0),
            cb_ttl=_env_f("ZIMU_DF_CB_TTL", 300.0),
            calendar_ttl=_env_f("ZIMU_DF_CALENDAR_TTL", 21600.0),
            breaker_threshold=_env_i("ZIMU_DF_BREAKER_THRESHOLD", 3),
            breaker_cooloff=_env_f("ZIMU_DF_BREAKER_COOLOFF", 60.0),
            gates=gates,
            disabled=disabled,
            max_quote_symbols=_env_i("ZIMU_DF_MAX_QUOTE_SYMBOLS", 50),
        )
