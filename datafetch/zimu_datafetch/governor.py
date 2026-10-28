"""限频与缓存治理（P4-T3）。

三个小组件，都由 `service.MarketService` 组合使用：

- `TTLCache`：进程内 TTL 缓存。`/quote` 按标的缓存（一次上游批量请求扇出
  多个调用方）、`/kline` 按 symbol+区间、`/cb_list`、`/calendar` 整表缓存。
  这是「分钟级轮询不打爆上游」的第一道闸，也是全市场快照类接口不进
  轮询路径的实际保障（重接口低频调用 + 长缓存）。
- `Breaker`：按源连续失败计数，达阈值 open，冷却期内请求直接跳下一个源
  （spike 结论：东财被拦时快速失败 ~0.4s，适合熔断切换而非重试等待）；
  冷却过后半开放行一次探测，成功即闭合。
- `RateGate`：同一源两次外呼之间的最小间隔（预约制，锁外 sleep），
  FastAPI 同步端点跑在线程池里，等待只占当次请求。

没有分布式状态：sidecar 单进程内存活，重启即清零——行情数据的权威在上游，
这里丢的只是缓存，不丢业务数据。
"""

from __future__ import annotations

import threading
import time
from typing import Any


class TTLCache:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._data: dict[str, tuple[float, Any]] = {}
        self.hits = 0
        self.misses = 0

    def get(self, key: str) -> Any | None:
        with self._lock:
            item = self._data.get(key)
            if item is None:
                self.misses += 1
                return None
            expires_at, value = item
            if time.monotonic() >= expires_at:
                del self._data[key]
                self.misses += 1
                return None
            self.hits += 1
            return value

    def set(self, key: str, value: Any, ttl: float) -> None:
        with self._lock:
            self._data[key] = (time.monotonic() + ttl, value)

    def stats(self) -> dict:
        with self._lock:
            return {"entries": len(self._data), "hits": self.hits, "misses": self.misses}


class Breaker:
    """连续失败熔断，冷却后半开。阈值/冷却由各源共享（配置见 Settings）。"""

    def __init__(self, threshold: int, cooloff: float) -> None:
        self._lock = threading.Lock()
        self.threshold = threshold
        self.cooloff = cooloff
        self._fails: dict[str, int] = {}
        self._opened_at: dict[str, float] = {}

    def check(self, source: str) -> tuple[bool, str]:
        """当前是否允许外呼该源。返回 (允许, 说明)；不允许时调用方应跳下一源。"""
        with self._lock:
            opened = self._opened_at.get(source)
            if opened is None:
                return True, "closed"
            remain = self.cooloff - (time.monotonic() - opened)
            if remain > 0:
                return False, f"circuit_open（冷却剩余 {remain:.1f}s）"
            return True, "half_open"  # 冷却已过：放行一次探测

    def success(self, source: str) -> None:
        with self._lock:
            self._fails.pop(source, None)
            self._opened_at.pop(source, None)

    def failure(self, source: str) -> None:
        with self._lock:
            n = self._fails.get(source, 0) + 1
            self._fails[source] = n
            if n >= self.threshold:
                self._opened_at[source] = time.monotonic()

    def states(self) -> dict:
        with self._lock:
            out: dict[str, dict] = {}
            now = time.monotonic()
            for src, n in self._fails.items():
                opened = self._opened_at.get(src)
                out[src] = {
                    "fails": n,
                    "open": opened is not None and now - opened < self.cooloff,
                }
            return out


class RateGate:
    def __init__(self, gaps: dict[str, float], default: float = 0.5) -> None:
        self._lock = threading.Lock()
        self.gaps = gaps
        self.default = default
        self._next_at: dict[str, float] = {}

    def wait(self, source: str) -> None:
        gap = self.gaps.get(source, self.default)
        if gap <= 0:
            return
        with self._lock:
            now = time.monotonic()
            nxt = self._next_at.get(source, 0.0)
            if now >= nxt:
                self._next_at[source] = now + gap
                delay = 0.0
            else:
                self._next_at[source] = nxt + gap
                delay = nxt - now
        if delay > 0:
            time.sleep(delay)
