"""真实源解析器纯函数测试（B14）。

样本文本 = 2026-10-29 盘后抓取的公开行情原始响应（截取到解析所需字段），
回归锚点：新浪对未收录指数回「空名 + 全零」行（sh000832 中证转债），
腾讯对该指数有完整数据——两源布局差异见 sources.py 解析函数注释。
"""

from zimu_datafetch.sources import _parse_sina_hq, _parse_tencent_qt

# 新浪 hq：sh000832 空名零价（未收录）；sh000001 正常
SINA_SAMPLE = (
    'var hq_str_sh000832=",0.0000,0.0000,0.0000,0.0000,0.0000,0,0,0,0,0,0,0,0,0,0,0,0,'
    '0,0,0,0,0,0,0,0,0,0,0,0,2026-10-29,09:00:00,00,";\n'
    'var hq_str_sh000001="上证指数,3676.7731,3681.4691,3651.4337,3678.4584,3645.2603,'
    '0,0,443564144,715620694906,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'
    '2026-10-29,15:35:30,00,";\n'
)

# 腾讯 qt：中证转债（指数，量原样手）+ 贵州茅台（股票，量手→股 ×100）
TENCENT_SAMPLE = (
    'v_sh000832="1~中证转债~000832~452.63~457.79~457.41~2670693~0~0~0.00~0~0.00~0~'
    '0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~~'
    '20261029161447~-5.16~-1.13~457.44~452.32~452.63/2670693/50568813180~2670693~5056881";\n'
    'v_sh600519="1~贵州茅台~600519~1354.00~1330.75~1349.99~42992~24647~18345~1353.77~1~'
    '1352.00~2~1351.06~1~1351.00~2~1350.99~1~1354.00~27~1354.01~1~1354.02~1~1354.18~1~'
    '1354.40~1~~20261029161457~23.25~1.75~1356.00~1330.01~1354.00/42992/5797980334~42992~579798";\n'
)


def test_sina_drops_uncovered_index_row():
    """空名 + 零价 = 源未收录 → 不出行（service 逐标的补齐交下一源），不伪装 halted。"""
    rows = _parse_sina_hq(SINA_SAMPLE, ["sh000832", "sh000001"])
    by = {r["symbol"]: r for r in rows}
    assert "sh000832" not in by
    ok = by["sh000001"]
    assert ok["status"] == "ok" and ok["kind"] == "index" and ok["name"] == "上证指数"
    assert ok["price"] == 3651.4337 and ok["prev"] == 3681.4691 and ok["open"] == 3676.7731
    assert ok["high"] == 3678.4584 and ok["low"] == 3645.2603
    assert ok["volume"] == 443564144 and ok["amount"] == 715620694906
    assert ok["date"] == "2026-10-29" and ok["time"] == "15:35:30"


def test_tencent_index_row():
    rows = _parse_tencent_qt(TENCENT_SAMPLE, ["sh000832"])
    ix = rows[0]
    assert ix["status"] == "ok" and ix["kind"] == "index" and ix["name"] == "中证转债"
    assert ix["price"] == 452.63 and ix["prev"] == 457.79 and ix["open"] == 457.41
    assert ix["high"] == 457.44 and ix["low"] == 452.32
    assert ix["volume"] == 2670693            # 指数原样透传（手），与新浪指数字段同口径
    assert ix["amount"] == 50568813180        # 合成字段第三段（元），与新浪 f[9] 同单位
    assert ix["chg_pct"] == -1.1272           # (452.63−457.79)/457.79×100，round 4
    assert ix["date"] == "2026-10-29" and ix["time"] == "16:14:47"


def test_tencent_stock_volume_scaled():
    rows = _parse_tencent_qt(TENCENT_SAMPLE, ["sh600519"])
    r = rows[0]
    assert r["status"] == "ok" and r["kind"] == "stock"
    assert r["price"] == 1354.0 and r["prev"] == 1330.75
    assert r["volume"] == 4299200             # 手 → 股 ×100（与新浪/东财约定对齐）
    assert r["amount"] == 5797980334
    assert r["date"] == "2026-10-29" and r["time"] == "16:14:57"


def test_tencent_missing_symbol_is_no_data():
    rows = _parse_tencent_qt(TENCENT_SAMPLE, ["sz999999"])
    assert rows[0]["status"] == "no_data" and rows[0]["price"] is None
