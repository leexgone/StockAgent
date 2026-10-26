# datafetch · 行情 sidecar（P2-T1 spike）

子木的行情取数层。ADR-0005 定案：Python sidecar（uv + FastAPI），akshare 为主、baostock 兜底，Rust 主程序经 HTTP 消费、把它当唯一行情适配器。当前目录是 **spike 产物**，生产化在 `06-roadmap.md` P4-T3（缓存、限频、兜底切换、错误契约）。

## 运行

```bash
cd datafetch
uv sync                      # 安装依赖（首次）
uv run spike_probe.py        # 第一轮：akshare 全接口探测（15 项 + 限频连打）
uv run spike_probe2.py       # 第二轮：东财直连诊断 + 新浪/同花顺替代源
uv run spike_probe3.py       # 第三轮：行业映射 / 东财批量字段 / baostock / 新浪限频
uv run uvicorn app:app --host 127.0.0.1 --port 8300   # 最小 API
```

API 冒烟：

```bash
curl "http://127.0.0.1:8300/healthz"
curl "http://127.0.0.1:8300/quote?codes=sh600519,sh510300,sh000001"
curl "http://127.0.0.1:8300/kline/600519"
curl "http://127.0.0.1:8300/calendar?days=10"
```

## Spike 结论摘要（2026-10-26 周一盘后实测）

- **新浪 `hq.sinajs.cn` 直连 = 实时主通道**：一次请求多标的（股票/ETF/指数/转债），字段人类可读无需缩放，需带 Referer。
- **东财对 akshare 封装指纹拦截**：akshare 全部东财实时接口 `RemoteDisconnected`（连打均拒），但带浏览器 UA 直连 `push2.eastmoney.com` 通（HTTP 200）→ sidecar 的东财通道必须直连实现，不依赖 akshare 封装。
- 雪球接口要求 `xq_a_token`，弃用；同花顺 ETF 表是场外净值，不适用场内实时价。
- **转债强赎**：集思录 `bond_cb_redeem_jsl` 字段全（强赎触发价/天计数/最后交易日/强赎状态），可直接支撑强赎预警。
- **交易日历**：`tool_trade_date_hist_sina` 通（1990 年起 8797 天，含未来交易日），`marketClock` 节假日扩展的日历源就它。
- **股票日线**：新浪 `stock_zh_a_daily` 冒烟稳定（沪/深/创业板均通，含换手率/流通股本），东财间歇被拦降为备选；**baostock 登录失败**（10002007 网络接收错误），兜底次序改为新浪系→东财直连，P4 复测再定去留。
- **行业映射**：新浪板块列表 + 成分股（`stock_sector_spot` + `stock_sector_detail`）可建 code→sector 映射，附带 PE/PB/市值字段。
- **最小 API 冒烟通过**：`/quote` 7 标的三态状态识别正确（含真实退市样本）、`/kline` 四类均返回当日 bar、`/calendar` 含未来交易日。
- **演示样本踩雷**：`110074` 已退市、`127022` 已摘牌（见 backlog B10）。

字段对照表、风险清单与限频结论的完整版：`docs/04-data-contract.md` 附录。

## 文件

| 文件 | 用途 |
|---|---|
| `app.py` | FastAPI 最小骨架（/quote /kline /calendar /healthz），spike 质量 |
| `spike_probe.py` | 第一轮探测：akshare 15 个接口 + 限频连打 |
| `spike_probe2.py` | 第二轮：东财直连诊断、新浪 hq 直连、替代源 |
| `spike_probe3.py` | 第三轮：行业映射、东财批量字段、baostock、新浪限频 |
| `pyproject.toml` / `uv.lock` | uv 管理的依赖（lock 提交入库） |
