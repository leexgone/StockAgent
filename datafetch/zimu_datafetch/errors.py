"""错误契约（P4-T3）。

所有可预期错误统一抛 `DfError`，由 `app.py` 的 exception handler 渲染成：

```json
{"error": {"code": "…", "message": "…", "detail": {…}}}
```

| HTTP | code | 场景 |
|---|---|---|
| 400 | `BAD_SYMBOL` | 代码缺少显式市场前缀（B7：禁止号段猜测）或格式非法 |
| 400 | `BAD_PARAMS` | 参数问题：kind 非法、日期格式错、codes 为空/超限、日历区间越界 |
| 502 | `UPSTREAM_ALL_FAILED` | 兜底链全部失败；`detail.attempts` 逐源列出原因（disabled / circuit_open / 异常） |
| 500 | `INTERNAL` | 未预期异常（细节只进日志，不回显堆栈） |

spike 版把上游异常文本直接拼进 `HTTPException.detail`——正式契约不再透传，
源侧细节进 `detail.attempts`（受控结构）或日志。
"""

from __future__ import annotations

from typing import Any

MAX_ATTEMPT_MSG = 200


def clamp_msg(text: str) -> str:
    return text[:MAX_ATTEMPT_MSG]


class DfError(Exception):
    """带 HTTP 状态与机器可读 code 的契约错误。"""

    def __init__(self, status: int, code: str, message: str, detail: Any = None):
        super().__init__(f"{status} {code}: {message}")
        self.status = status
        self.code = code
        self.message = message
        self.detail = detail

    def to_body(self) -> dict:
        return {"error": {"code": self.code, "message": self.message, "detail": self.detail}}


def bad_symbol(raw: Any, why: str) -> DfError:
    return DfError(400, "BAD_SYMBOL", f"行情代码非法：{raw!r}（{why}）", {"input": str(raw)})


def bad_params(message: str, detail: Any = None) -> DfError:
    return DfError(400, "BAD_PARAMS", message, detail)


def all_failed(attempts: list[dict]) -> DfError:
    return DfError(
        502,
        "UPSTREAM_ALL_FAILED",
        "该数据类目的行情源全部不可用（服务未崩；稍后自动半开重试，可先看 detail.attempts）",
        {"attempts": attempts},
    )


def attempt(source: str, reason: str) -> dict:
    return {"source": source, "error": clamp_msg(reason)}
