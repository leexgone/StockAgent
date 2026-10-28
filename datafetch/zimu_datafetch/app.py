"""FastAPI 应用（P4-T3）。

端点契约的权威描述见 `datafetch/README.md` 与 `docs/04` 附录「正式契约」小节；
OpenAPI 化留给 P4-T5（届时与本文件的 pydantic 响应模型汇流）。

监听 127.0.0.1——sidecar 只服务本机 Rust 主程序（ADR-0005 内网形态），
不开公网入站。
"""

from __future__ import annotations

import logging
import sys

import akshare as ak
from fastapi import FastAPI, Query
from fastapi.responses import JSONResponse

from . import __version__
from .errors import DfError
from .service import MarketService

log = logging.getLogger("zimu.datafetch")


def create_app(settings=None, service: MarketService | None = None) -> FastAPI:
    svc = service or MarketService(settings)
    app = FastAPI(
        title="zimu-datafetch",
        version=__version__,
        description="子木行情 sidecar：/quote /kline /cb_list /calendar（契约见 README 与 docs/04 附录）",
    )
    app.state.service = svc

    @app.exception_handler(DfError)
    async def df_error_handler(_request, exc: DfError):
        return JSONResponse(status_code=exc.status, content=exc.to_body())

    @app.exception_handler(Exception)
    async def internal_handler(request, exc: Exception):
        log.exception("unhandled error on %s %s", request.method, request.url.path)
        return JSONResponse(status_code=500, content={
            "error": {"code": "INTERNAL", "message": f"内部错误：{type(exc).__name__}",
                      "detail": None}})

    @app.get("/healthz")
    def healthz():
        return {
            "ok": True,
            "version": __version__,
            "akshare": ak.__version__,
            "python": sys.version.split()[0],
            "disabled_sources": sorted(svc.s.disabled),
            "breakers": svc.breaker.states(),
            "cache": svc.cache.stats(),
        }

    @app.get("/quote")
    def quote(codes: str = Query(..., description="逗号分隔，须带显式市场前缀：sh600519,sz159915,sh000001")):
        return svc.quote(codes)

    @app.get("/kline/{symbol}")
    def kline(symbol: str, kind: str | None = Query(None, description="stock/etf/index/cb；缺省按号段路由（仅内部用）"),
              start: str | None = Query(None, description="YYYYMMDD 或 YYYY-MM-DD，默认 400 天前"),
              end: str | None = Query(None, description="同上，默认今天")):
        return svc.kline(symbol, kind, start, end)

    @app.get("/cb_list")
    def cb_list():
        return svc.cb_list()

    @app.get("/calendar")
    def calendar(start: str | None = Query(None, description="默认今天-180 天"),
                 end: str | None = Query(None, description="默认今天+180 天（含未来交易日，marketClock 用）")):
        return svc.calendar(start, end)

    return app
