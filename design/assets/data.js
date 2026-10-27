/* ============================================================
 * 子木 · 股票智能助理 — 共享数据层
 * 所有汇总指标（总资产 / 盈亏 / 权重 / 集中度 / 回撤）均由持仓
 * 反算得出，页面里不出现与数据自相矛盾的硬编码数字。
 * 行情与持仓为「演示样本」，用于验证界面与信息架构。
 * ============================================================ */
(function (W) {
  'use strict';

  /* ---------- 工具 ---------- */
  function seed(str) {
    let h = 2166136261 >>> 0;
    for (let i = 0; i < str.length; i++) { h ^= str.charCodeAt(i); h = Math.imul(h, 16777619); }
    return h >>> 0;
  }
  // 确定性随机（同一标的每次渲染曲线一致）
  function rng(s) {
    let a = seed(s);
    return function () {
      a |= 0; a = (a + 0x6D2B79F5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  // 带漂移的随机游走，末值锚定到当前价
  function walk(key, n, end, vol, drift) {
    const r = rng(key);
    const out = [];
    let v = 1;
    for (let i = 0; i < n; i++) { v *= 1 + (r() - 0.5) * vol + drift; out.push(v); }
    const k = end / out[n - 1];
    return out.map(x => x * k);
  }
  const fmt = (v, d) => Number(v).toLocaleString('zh-CN', { minimumFractionDigits: d == null ? 2 : d, maximumFractionDigits: d == null ? 2 : d });
  const money = v => '¥' + fmt(v, 2);
  const pct = v => (v > 0 ? '+' : v < 0 ? '' : '') + fmt(v, 2) + '%';
  const wan = v => (v / 10000).toFixed(2) + ' 万';
  const dirClass = v => v > 0 ? 'up' : v < 0 ? 'down' : 'flat';
  // 报价小数位按 kind（03 §1 定稿，B4/P1-T3）：股票 2 位、ETF/转债 3 位；页面不得手搓精度
  const priceDec = kind => (kind === 'stock' ? 2 : 3);
  // 涨停/跌停价（03 §2 定稿，B2/P1-T2）：昨收 × (1 ± pct/100) 按报价小数位四舍五入，
  // 整数单元运算与交易所算法一致（裸浮点 9.85×1.1 = 10.834999… 会错得 10.83）。
  // prev ≤ 0（停牌/无行情）或 pct 未配置 → null，页面显示 — 并跳过区间拦截。
  function limitPrices(kind, prev, pct) {
    if (!(prev > 0) || !(pct > 0)) return null;
    const scale = Math.pow(10, priceDec(kind));
    const units = Math.round(prev * scale);
    const cut = up => Math.floor((units * (up ? 100 + pct : 100 - pct) + 50) / 100) / scale;
    return { lo: cut(false), hi: cut(true) };
  }

  /* ---------- 个人账户 ---------- */
  const ws = { name: '我的账户', init: '我', style: '均衡成长', cash: 46800.00, broker: '华泰证券 · 沪深 A 股账户（*8842）', wechat: '已绑定 · 微信' };

  /* ---------- 指数 ---------- */
  const indices = [
    { name: '上证指数', code: '000001.SH', v: 3902.45, chg: 0.62 },
    { name: '深证成指', code: '399001.SZ', v: 12648.30, chg: 0.38 },
    { name: '创业板指', code: '399006.SZ', v: 2556.12, chg: -0.44 },
    { name: '沪深300', code: '000300.SH', v: 4528.66, chg: 0.61 },
    { name: '中证转债', code: '000832.SH', v: 428.55, chg: 0.29 },
  ];

  /* ---------- 持仓（股票 / ETF / 可转债） ----------
   * limitPct = 涨跌幅比例（03 §2 定稿：标板 ±10、创业板/科创板 ±20、转债 ±20；
   * 比例是标的属性，ETF 无法由代码推断——演示样本手工配置，P4 由行情源自动填充）。 */
  const holdings = [
    { code: '600519', name: '贵州茅台', mkt: 'SH', kind: 'stock', sector: '食品饮料', shares: 100,  cost: 1512.00, price: 1618.00, prev: 1601.50, limitPct: 10, tscore: 71 },
    { code: '300750', name: '宁德时代', mkt: 'SZ', kind: 'stock', sector: '电力设备', shares: 400,  cost: 196.40,  price: 212.35,  prev: 207.80,  limitPct: 20, tscore: 78 },
    { code: '002594', name: '比亚迪',   mkt: 'SZ', kind: 'stock', sector: '汽车',     shares: 500,  cost: 264.80,  price: 254.10,  prev: 259.40,  limitPct: 10, tscore: 44 },
    { code: '600036', name: '招商银行', mkt: 'SH', kind: 'stock', sector: '银行',     shares: 1800, cost: 33.85,   price: 37.05,   prev: 36.62,   limitPct: 10, tscore: 66 },
    { code: '600276', name: '恒瑞医药', mkt: 'SH', kind: 'stock', sector: '医药生物', shares: 700,  cost: 52.30,   price: 58.42,   prev: 57.10,   limitPct: 10, tscore: 74 },
    { code: '510300', name: '沪深300ETF', mkt: 'SH', kind: 'etf', sector: '宽基指数', shares: 12000, cost: 4.015, price: 4.291,   prev: 4.262,   limitPct: 10, tscore: 62 },
    { code: '588000', name: '科创50ETF',  mkt: 'SH', kind: 'etf', sector: '宽基指数', shares: 8000, cost: 0.968,   price: 1.086,   prev: 1.104,   limitPct: 20, tscore: 51 },
    { code: '110074', name: '精达转债',   mkt: 'SH', kind: 'cb',  sector: '有色金属', shares: 120,  cost: 118.500, price: 126.720, prev: 124.380, limitPct: 20, tscore: 69 },
    { code: '113650', name: '博威转债',   mkt: 'SH', kind: 'cb',  sector: '有色金属', shares: 80,   cost: 132.400, price: 127.150, prev: 128.600, limitPct: 20, tscore: 48 },
    { code: '127022', name: '恒逸转债',   mkt: 'SZ', kind: 'cb',  sector: '基础化工', shares: 60,   cost: 106.800, price: 105.350, prev: 104.200, limitPct: 20, tscore: 55 },
  ];
  const kindLabel = { stock: '股票', etf: 'ETF / 基金', cb: '可转债' };

  /* ---------- 派生与汇总 ----------
   * 所有派生值统一由 recalc() 重算：任何会改变持仓或价格的入口
   * （自动跳动、手动刷新、记入交易、将来的真实数据推送）都必须调用它，
   * 保证页面数字与行情条永远来自同一份快照（backlog B1 的修复）。
   * agg 是唯一的汇总快照，recalc 后同步到导出对象，页面读 D.totalAssets 等即最新值。 */
  let ZM = null;                       // 导出对象，文件末尾组装
  let bySector = {};                   // 行业市值，wsec() 用
  const agg = {
    cash: 0, mvStocks: 0, mvEtf: 0, mvCb: 0, totalMv: 0, totalAssets: 0,
    dayPnl: 0, dayPnlPct: 0, totalPnl: 0, totalPnlPct: 0, costTotal: 0,
    exposure: 0, sectors: [], hhi: 0, top3Sector: 0,
  };
  function publish() { if (ZM) Object.assign(ZM, agg); }

  function recalc() {
    // 逐笔派生：涨跌、市值、当日盈亏、累计盈亏；spark/bars 是演示曲线，只生成一次
    holdings.forEach(h => {
      h.dayChg = (h.price - h.prev) / h.prev * 100;
      h.dayVal = (h.price - h.prev) * h.shares;
      h.mv = h.price * h.shares;
      h.pnl = (h.price - h.cost) * h.shares;
      h.pnlPct = (h.price - h.cost) / h.cost * 100;
      if (!h.spark) h.spark = walk(h.code + h.name, 30, h.price, h.kind === 'cb' ? 0.012 : 0.03, 0.0012);
      if (!h.bars) h.bars = walk(h.code + 'v', 30, 1, 1.4, 0).map(v => 0.25 + v * 0.75);
    });

    agg.cash = ws.cash;
    agg.mvStocks = holdings.filter(h => h.kind === 'stock').reduce((s, h) => s + h.mv, 0);
    agg.mvEtf = holdings.filter(h => h.kind === 'etf').reduce((s, h) => s + h.mv, 0);
    agg.mvCb = holdings.filter(h => h.kind === 'cb').reduce((s, h) => s + h.mv, 0);
    agg.totalMv = agg.mvStocks + agg.mvEtf + agg.mvCb;
    agg.totalAssets = agg.totalMv + agg.cash;
    agg.dayPnl = holdings.reduce((s, h) => s + h.dayVal, 0);
    agg.totalPnl = holdings.reduce((s, h) => s + h.pnl, 0);
    agg.costTotal = holdings.reduce((s, h) => s + h.cost * h.shares, 0);
    agg.totalPnlPct = agg.costTotal ? agg.totalPnl / agg.costTotal * 100 : 0;
    // 当日盈亏百分比分母 = 昨日总资产（= 当前总资产减去当日变动）
    agg.dayPnlPct = (agg.totalAssets - agg.dayPnl) ? agg.dayPnl / (agg.totalAssets - agg.dayPnl) * 100 : 0;
    agg.exposure = agg.totalAssets ? agg.totalMv / agg.totalAssets * 100 : 0;

    // 权重：市值 / 总资产，与 wp() 同分母，保留数值供排序、纪律校验与跨页使用
    holdings.forEach(h => { h.weight = agg.totalAssets ? h.mv / agg.totalAssets * 100 : 0; });

    // 行业集中度
    bySector = {};
    holdings.forEach(h => { bySector[h.sector] = (bySector[h.sector] || 0) + h.mv; });
    agg.sectors = Object.entries(bySector)
      .map(([name, v]) => ({ name, v, w: agg.totalMv ? v / agg.totalMv * 100 : 0 }))
      .sort((a, b) => b.v - a.v);
    agg.hhi = agg.sectors.reduce((s, x) => s + Math.pow(x.w, 2), 0) / 10000; // 归一化 0–1
    agg.top3Sector = agg.sectors.slice(0, 3).reduce((s, x) => s + x.w, 0);

    publish();
  }
  recalc(); // 加载即首算；页面不要再自行补算派生值

  // 实时总市值（不经快照，任何时刻调用都一致）
  const totalMvAll = () => holdings.reduce((s, h) => s + h.price * h.shares, 0);

  // 文案里出现的仓位数字，一律取自 recalc() 的快照，避免「文字与图表打架」
  const H = c => holdings.find(h => h.code === c) || {};
  const wp = c => (H(c).mv ? (H(c).mv / agg.totalAssets * 100).toFixed(1) : '0.0');
  const wsec = n => (bySector[n] ? (bySector[n] / agg.totalMv * 100).toFixed(1) : '0.0');
  const BYD = H('002594');
  // 降到目标仓位需要卖的股数（按 100 股一手向上取整）
  const sellTo = (c, target) => {
    const h = H(c);
    return Math.max(100, Math.ceil((h.mv - agg.totalAssets * target / 100) / h.price / 100) * 100);
  };
  const weightAfter = (c, qty) => ((H(c).mv - qty * H(c).price) / agg.totalAssets * 100).toFixed(1);

  // 权益曲线（近 90 个交易日）
  const equity = {
    dates: [], nav: [], bench: [],
  };
  (function () {
    const n = 90;
    const port = walk('PORT90', n, agg.totalAssets, 0.018, 0.0009);
    const bench = walk('HS300_90', n, 4528.66 / (1 + 0.086), 0.011, 0.0008);
    const base = port[0];
    const d = new Date('2026-10-24T00:00:00');
    for (let i = 0; i < n; i++) { equity.nav.push(port[i]); equity.bench.push(bench[i] / bench[0] * base); }
    // 交易日回溯
    let cur = new Date(d), got = 0, labels = [];
    while (got < n) {
      const dow = cur.getDay();
      if (dow !== 0 && dow !== 6) { labels.unshift((cur.getMonth() + 1) + '/' + cur.getDate()); got++; }
      cur.setDate(cur.getDate() - 1);
    }
    equity.dates = labels;
  })();

  // 组合净值指标（由 equity 计算）
  function perf(series, benchSeries) {
    const ret = [], vol = [];
    for (let i = 1; i < series.length; i++) {
      ret.push(series[i] / series[i - 1] - 1);
      vol.push(benchSeries[i] / benchSeries[i - 1] - 1);
    }
    const mean = ret.reduce((a, b) => a + b, 0) / ret.length;
    const sd = Math.sqrt(ret.reduce((a, b) => a + (b - mean) ** 2, 0) / ret.length);
    let peak = series[0], mdd = 0;
    series.forEach(v => { peak = Math.max(peak, v); mdd = Math.min(mdd, (v - peak) / peak); });
    let bpeak = benchSeries[0], bmdd = 0;
    benchSeries.forEach(v => { bpeak = Math.max(bpeak, v); bmdd = Math.min(bmdd, (v - bpeak) / bpeak); });
    const cum = series[series.length - 1] / series[0] - 1;
    const bcum = benchSeries[benchSeries.length - 1] / benchSeries[0] - 1;
    return {
      cum: cum * 100, benchCum: bcum * 100,
      annVol: sd * Math.sqrt(244) * 100, benchVol: Math.sqrt(vol.reduce((a, b) => a + b * b, 0) / vol.length) * Math.sqrt(244) * 100,
      mdd: mdd * 100, benchMdd: bmdd * 100,
      sharpe: (mean * 244) / (sd * Math.sqrt(244) + 1e-9),
      winRate: ret.filter((r, i) => r > vol[i]).length / ret.length * 100,
    };
  }
  const P = perf(equity.nav, equity.bench);

  /* ---------- 预警 ---------- */
  const alerts = [
    { time: '10:32', code: '300750', name: '宁德时代', type: '放量突破', level: 'up', text: '成交额明显高于 20 日均量，价格站上 60 日均线', done: false },
    { time: '09:58', code: '002594', name: '比亚迪', type: '跌破成本', level: 'down', text: `现价 ${fmt(BYD.price, priceDec(BYD.kind))} 低于成本 ${fmt(BYD.cost, priceDec(BYD.kind))}，浮亏扩大至 ${pct(BYD.pnlPct)}`, done: false },
    { time: '14:47', code: '588000', name: '科创50ETF', type: '止盈提醒', level: 'warn', text: `累计收益 ${pct(H('588000').pnlPct)}，触及你设定的 +12% 止盈线`, done: false },
    { time: '08:55', code: '113650', name: '博威转债', type: '强赎公告', level: 'warn', text: '正股触发有条件赎回条款，最后交易日 11-20，溢价率 12.4%', done: true },
    { time: '昨 15:05', code: '600036', name: '招商银行', type: '盘后复盘', level: 'flat', text: '已连续 3 日跑赢银行指数，PB 0.96 处近 3 年 31 分位', done: true },
  ];

  /* ---------- AI 分析建议库 ---------- */
  const advice = {
    '300750': {
      score: 78, action: '加仓', horizon: '中线 1–3 月', conf: 0.72, position: '当前 14.4% → 建议 18%',
      target: [226, 241], stop: 191.4, reason: '储能出货增速超市场预期，技术面完成三角收敛向上突破。',
      dims: [
        { k: '技术面', s: 82, t: '站上 60 日线并放量，MACD 零轴上方金叉；20 日乖离 6.1%，短线略偏热。' },
        { k: '基本面', s: 76, t: '动力电池全球份额回升，毛利率环比改善 1.8pct；PE(TTM) 21.4 倍，处近三年 38 分位。' },
        { k: '资金面', s: 71, t: '主力 5 日净流入，北向持股占比由 12.1% 升至 12.9%；融资余额同步走高。' },
        { k: '消息面', s: 64, t: '欧洲碳边界方案落地偏利好，但单一客户车型销量波动构成不确定性。' },
      ],
      risks: ['若周线跌破 199.20 则突破失效，建议止损 191.40。', '行业产能释放快于需求时，毛利率改善可能不可持续。'],
    },
    '002594': {
      score: 44, action: '减仓', horizon: '短中线 2–6 周', conf: 0.66, position: '当前 22.6% → 建议 15%',
      target: [238, 249], stop: 241.0, reason: '价格战挤压单车利润，行业估值下修；但仓位集中度过高是主要问题。',
      dims: [
        { k: '技术面', s: 38, t: '跌破 20 与 60 日线，量能萎缩；周线三连阴，尚未出现止跌形态。' },
        { k: '基本面', s: 55, t: '销量仍创新高但均价下滑，单车毛利环比 -0.4 万元；海外占比提升部分对冲。' },
        { k: '资金面', s: 41, t: '主力 5 日净流出，融资余额连续 4 日下降。' },
        { k: '消息面', s: 46, t: '新一轮官降预期发酵，经销商库存系数 1.7 偏高。' },
      ],
      risks: ['单笔浮亏已达 -4.04%，建议分批而非一次性处理，避免情绪化操作。', `按现仓位测算，正股再跌 10% 大约拖累总资产 ${(H('002594').mv / agg.totalAssets * 10).toFixed(1)}%；把仓位降到 15% 后，同样的跌幅只拖累约 1.5%。`],
    },
    '600519': {
      score: 71, action: '持有', horizon: '长线 6 月+', conf: 0.69, position: '当前 28.0% → 维持（提额需等回落）',
      target: [1720, 1780], stop: 1450, reason: '基本面稳健，但单票占总资产 28%，已触及你设的集中度上限。',
      dims: [
        { k: '技术面', s: 68, t: '年线上方震荡，回踩 1580 获支撑；RSI 56 中性偏强。' },
        { k: '基本面', s: 80, t: '直销占比继续提升，预收款节奏正常；PE(TTM) 22.1 倍处近五年低位区间。' },
        { k: '资金面', s: 63, t: '北向小幅回流，机构持仓稳定，缺少增量买盘。' },
        { k: '消息面', s: 60, t: '批价企稳于 2,260 元/瓶，渠道库存中性。' },
      ],
      risks: ['不建议在 28% 集中度上继续加仓；若回落到 25% 以下再考虑补仓。'],
    },
    '600036': {
      score: 66, action: '持有', horizon: '长线', conf: 0.64, position: '当前 11.5% → 维持',
      target: [40.2, 41.8], stop: 33.8, reason: '股息与估值提供安全垫，弹性有限。',
      dims: [
        { k: '技术面', s: 64, t: '沿 20 日线缓慢上行，波动率处低位。' },
        { k: '基本面', s: 72, t: '不良率环比持平，拨备覆盖率厚；股息率 4.6%。' },
        { k: '资金面', s: 61, t: '保险资金增配红利板块，板块整体受益。' },
        { k: '消息面', s: 58, t: '净息差仍有小幅收窄压力。' },
      ],
      risks: ['若你希望提高组合进攻性，减红利不如减单票集中度。'],
    },
    '600276': {
      score: 74, action: '买入', horizon: '中线 1–3 月', conf: 0.7, position: '当前 7.1% → 建议 10%',
      target: [64.5, 68.0], stop: 53.4, reason: '创新药出海里程碑临近，且医药在你组合中占比偏低，可作分散用途。',
      dims: [
        { k: '技术面', s: 75, t: '周线双底颈线突破，量能温和放大。' },
        { k: '基本面', s: 70, t: '在研管线进入读出期，海外授权首付款确认节奏改善。' },
        { k: '资金面', s: 78, t: '医药生物板块 10 日主力净流入居行业前列。' },
        { k: '消息面', s: 72, t: '医保谈判规则温和化，创新药支付端预期改善。' },
      ],
      risks: ['临床数据读出存在二元风险，建议以不超过 10% 仓位参与。'],
    },
    '510300': { score: 62, action: '定投', horizon: '长期', conf: 0.68, position: '当前 8.9% → 每月 +2,000', target: null, stop: null, reason: '宽基作为组合压舱石，当前估值中位偏低。', dims: [ { k: '技术面', s: 60, t: '沪深300 站上 250 日线，趋势中性偏多。' }, { k: '基本面', s: 64, t: '指数 PE 12.6 倍，处近十年 46 分位。' }, { k: '资金面', s: 62, t: 'ETF 份额近一月净增，配置型资金流入。' }, { k: '消息面', s: 58, t: '政策面偏暖，缺乏强催化。' } ], risks: ['定投价值在于摊平成本，不适合一次性重仓。'] },
    '588000': { score: 51, action: '持有', horizon: '波段', conf: 0.58, position: '当前 1.5% → 维持', target: [1.15, 1.20], stop: 1.02, reason: '高波动小仓位，作为科技敞口保留。', dims: [ { k: '技术面', s: 48, t: '跌破 10 日线，短线进入整理。' }, { k: '基本面', s: 55, t: '成分股半导体营收回暖，但估值不便宜。' }, { k: '资金面', s: 49, t: '份额略有赎回。' }, { k: '消息面', s: 52, t: '出口管制变化仍是主要变量。' } ], risks: ['单日波动常超 2%，请确认自己承受得起。'] },
    '110074': { score: 69, action: '持有', horizon: '短中线', conf: 0.61, position: '当前 2.9% → 维持', target: null, stop: 121.0, reason: '纯债溢价率 8.2%，债底保护尚可，正股弹性仍在。', dims: [ { k: '技术面', s: 66, t: '突破盘整区间，转股溢价率压缩至 14.6%。' }, { k: '基本面', s: 68, t: '正股受益高端铜材需求，业绩改善。' }, { k: '资金面', s: 70, t: '转债余额小幅下降，筹码集中。' }, { k: '消息面', s: 55, t: '无重大公告。' } ], risks: ['溢价率若回到 20% 以上，跟涨能力会明显减弱。'] },
    '113650': { score: 48, action: '减仓', horizon: '短线', conf: 0.63, position: '当前 1.8% → 建议清仓或换券', target: null, stop: 124.0, reason: '接近强赎时间窗，溢价率与赎回风险叠加。', dims: [ { k: '技术面', s: 45, t: '冲高回落，10 日线上方反复。' }, { k: '基本面', s: 52, t: '正股稳健但估值已兑现。' }, { k: '资金面', s: 44, t: '溢价率走阔伴随缩量。' }, { k: '消息面', s: 40, t: '已发布可能强赎提示公告。' } ], risks: ['强赎后未及时转股或卖出会承担额外损失，注意最后交易日 11-20。'] },
    '127022': { score: 55, action: '持有', horizon: '中线', conf: 0.57, position: '当前 1.1% → 维持', target: null, stop: 101.5, reason: '双低特征明显（价格 105.350 / 溢价率 9.8%），下行有限。', dims: [ { k: '技术面', s: 50, t: '底部横盘，波动收敛。' }, { k: '基本面', s: 54, t: '化纤价差修复带动转债债性抬升。' }, { k: '资金面', s: 56, t: '低价格券近期受配置资金关注。' }, { k: '消息面', s: 58, t: '无风险事件。' } ], risks: ['低价格券弹性弱，靠时间换空间。'] },
  };

  // 文案中的仓位百分比统一用计算值覆盖，保证「建议文字 / 表格 / 图表」口径一致
  Object.entries(advice).forEach(([code, a]) => {
    const fix = s => String(s)
      .replace(/当前 [\d.]+%/g, '当前 ' + wp(code) + '%')
      .replace(/占总资产 [\d.]+%/g, '占总资产 ' + wp(code) + '%');
    a.position = fix(a.position);
    a.reason = fix(a.reason);
    a.risks = a.risks.map(fix);
    a.dims.forEach(d => { d.t = fix(d.t); });
  });

  // 自选观察（非持仓）
  const watchlist = [
    { code: '002415', name: '海康威视', mkt: 'SZ', kind: 'stock', limitPct: 10, sector: '电子', price: 34.86, dayChg: 0.91, note: '观察：AI 视觉商业化' },
    { code: '688111', name: '金山办公', mkt: 'SH', kind: 'stock', limitPct: 20, sector: '计算机', price: 287.40, dayChg: -1.24, note: '观察：订阅 ARPU' },
    { code: '601899', name: '紫金矿业', mkt: 'SH', kind: 'stock', limitPct: 10, sector: '有色金属', price: 19.72, dayChg: 1.86, note: '与现有有色仓位重叠' },
    { code: '512890', name: '红利低波ETF', mkt: 'SH', kind: 'etf', limitPct: 10, sector: '宽基指数', price: 1.284, dayChg: 0.31, note: '防守型候选，波动低' },
  ];

  /* ---------- 对话式问股 样本问答 ---------- */
  const qa = [
    {
      q: '比亚迪今天亏了，要不要止损？',
      a: [
        { t: `先给结论：不该恐慌清仓，但确实要减 —— 把 ${BYD.name} 从 ${wp('002594')}% 降到 15% 附近，一次减 ${sellTo('002594', 15)} 股就够，不用拆成很多笔。`, k: 'conclusion' },
        { t: `三个理由：① 你的浮亏来自仓位而非判断——${wp('002594')}% 是组合里的第二大单票，一旦跌 10% 就拖累总资产 ${(H('002594').mv / agg.totalAssets * 10).toFixed(1)}%；② 技术面刚跌破 60 日线，尚未出现放量止跌；③ 基本面销量仍在创新高，不是逻辑坏掉。`, k: 'body' },
        { t: `具体操作：今天 254 附近一次减 ${sellTo('002594', 15)} 股（整手），仓位落到约 ${weightAfter('002594', sellTo('002594', 15))}%，已经回到上限内；剩下的 ${BYD.shares - sellTo('002594', 15)} 股用 241 这条止损线守着，不必再减。`, k: 'plan' },
      ],
      cites: [
        { src: '持仓明细', txt: `002594 比亚迪 · ${fmt(BYD.shares, 0)} 股 · 成本 ${fmt(BYD.cost, priceDec(BYD.kind))} · 现价 ${fmt(BYD.price, priceDec(BYD.kind))} · 当日 ${pct(BYD.dayChg)}` },
        { src: '组合诊断', txt: `单票集中度 ${wp('002594')}%（上限 25%），前三行业合计占持仓 ${agg.top3Sector.toFixed(1)}%` },
        { src: '行情快照', txt: `10-24 收盘 ${fmt(BYD.price, priceDec(BYD.kind))}，昨收 ${fmt(BYD.prev, priceDec(BYD.kind))}，跌破 20 日线与 60 日线` },
      ],
      chips: ['分两笔的话，第一笔挂什么价？', '降到 15% 后风险会小多少？', '换成什么标的更好？'],
    },
    {
      q: '帮我把组合调均衡一点',
      a: [
        { t: '结论：主要问题是「单票 + 单行业」双重集中，不是缺标的。', k: 'conclusion' },
        { t: `前三大行业合计占持仓 ${agg.top3Sector.toFixed(1)}%：${agg.sectors.slice(0, 3).map(s => `${s.name} ${s.w.toFixed(1)}%`).join('、')}；最大单票 ${H('600519').name} 一只就占总资产 ${wp('600519')}%，已经贴着你的 25% 上限。`, k: 'body' },
        { t: `先只做一件事：${BYD.name} 减 ${sellTo('002594', 15)} 股（降到约 ${weightAfter('002594', sellTo('002594', 15))}%），${H('600519').name} 不加仓，回收资金按 4:3:3 投向医药 / 宽基定投 / 现金。调整前后的波动对比在「组合诊断」页可以实测，代价是牛市弹性下降。`, k: 'plan' },
      ],
      cites: [
        { src: '行业权重', txt: agg.sectors.slice(0, 4).map(s => `${s.name} ${s.w.toFixed(1)}%`).join(' · ') },
        { src: '回测', txt: `近 ${equity.dates.length} 交易日 组合 ${pct(P.cum)} / 沪深300 ${pct(P.benchCum)}，最大回撤 ${P.mdd.toFixed(1)}%` },
      ],
      chips: ['这个调整要花多少钱的手续费？', '能不能只减不卖？', '减掉的钱先留现金还是补别的？'],
    },
    {
      q: '博威转债是不是要赎回了？',
      a: [
        { t: '结论：已发布「可能强赎」提示公告，按条款在 11-20 前需要处理，别拖到最后一天。', k: 'conclusion' },
        { t: '你的 80 张成本 132.400、现价 127.150，浮亏 -3.96%；转股价值 113.2、溢价率 12.4%，靠正股再涨修复溢价的时间窗已经很窄。', k: 'body' },
        { t: '两条路：① 11-12 前二级卖出，成交均价 127 上下；② 转股后卖出正股，需承担次日价格风险。若拿不定，先卖一半锁定价差。', k: 'plan' },
      ],
      cites: [ { src: '公告', txt: '10-14 关于赎回公司债券的提示性公告 · 最后交易日 11-20' }, { src: '持仓明细', txt: '113650 博威转债 · 80 张 · 现价 127.150' } ],
      chips: ['转股和卖出哪个到手多？', '强赎后不处理会亏多少？', '帮我把这个设成微信提醒'],
    },
  ];

  const sessions = [
    { t: '10-24 09:12', who: '微信 · 语音', text: '「茅台今天能不能补？」', reply: '已答：28% 集中度已到上限，建议回落到 25% 以下再补。', read: true },
    { t: '10-23 20:41', who: '定时任务', text: '盘后复盘日报已推送到我的微信', reply: '', read: true },
    { t: '10-23 14:07', who: '微信 · 文字', text: '「科创50 跌了 2 个点，要不要跑？」', reply: '已答：仓位仅 1.5%，属于承受范围内，建议持有观察 10 日线。', read: false },
  ];

  /* ---------- 市场时钟（真实时段逻辑） ---------- */
  function marketClock(now) {
    const d = now || new Date();
    const dow = d.getDay(), mins = d.getHours() * 60 + d.getMinutes();
    const hhmm = d.toTimeString().slice(0, 8);
    const isTrade = dow >= 1 && dow <= 5; // 节假日表可在接入真实日历后扩展
    if (!isTrade) {
      const next = new Date(d); next.setDate(next.getDate() + (dow === 6 ? 2 : 1));
      return { state: '休市', dot: 'closed', next: '下一交易日 ' + (next.getMonth() + 1) + '/' + next.getDate() + ' 09:15 集合竞价', time: hhmm, live: false };
    }
    const at = (h, m) => h * 60 + m;
    if (mins < at(9, 15)) return { state: '盘前', dot: 'closed', next: '距集合竞价 ' + cd(at(9, 15) - mins), time: hhmm, live: false };
    if (mins < at(9, 30)) return { state: '集合竞价', dot: 'live', next: '距开盘 ' + cd(at(9, 30) - mins), time: hhmm, live: true };
    if (mins < at(11, 30)) return { state: '交易中 · 早盘', dot: 'live', next: '距午间休市 ' + cd(at(11, 30) - mins), time: hhmm, live: true };
    if (mins < at(13, 0)) return { state: '午间休市', dot: 'lunch', next: '距下午开盘 ' + cd(at(13, 0) - mins), time: hhmm, live: false };
    if (mins < at(15, 0)) return { state: '交易中 · 午盘', dot: 'live', next: '距收盘 ' + cd(at(15, 0) - mins), time: hhmm, live: true };
    if (mins < at(15, 30)) return { state: '盘后定价交易', dot: 'lunch', next: '复盘时段 · 次日 09:15 集合竞价', time: hhmm, live: false };
    return { state: '已收盘', dot: 'closed', next: '距次日集合竞价 ' + cd(at(9, 15) + 24 * 60 - mins), time: hhmm, live: false };
  }
  function cd(m) {
    m = Math.max(0, Math.round(m));
    const h = Math.floor(m / 60);
    return (h ? h + ' 小时 ' : '') + (m % 60) + ' 分';
  }

  /* ---------- 共享外壳（侧栏 / 顶栏 / 移动底栏 / 提示） ---------- */
  const ICONS = {
    grid: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>',
    wallet: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H18a2 2 0 0 1 2 2v1"/><path d="M3 7.5V18a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-3"/><path d="M21 12.5h-4a1.5 1.5 0 0 0 0 3h4z"/></svg>',
    spark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M3 17l5-6 4 3 5-7 4 4"/></svg>',
    chat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M21 12a8 8 0 0 1-8 8H8l-4 3v-6.5A8 8 0 0 1 12 4h1a8 8 0 0 1 8 8z"/><path d="M9 11h6M9 14h3"/></svg>',
    shield: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z"/><path d="M9 12l2 2 4-4"/></svg>',
    wechat: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M4 11a7 5.5 0 1 1 11.2 4.4L17 18l-3-1.3"/><path d="M9.5 9.5h.01M6.5 9.5h.01"/><path d="M14 13.5a5.5 4.5 0 0 1 6 4.4L19 20l2 1-.5-1.8"/></svg>',
    bell: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M18 9a6 6 0 1 0-12 0c0 5-2 6-2 6h16s-2-1-2-6"/><path d="M10.5 20a2 2 0 0 0 3 0"/></svg>',
    search: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><circle cx="11" cy="11" r="7"/><path d="M16.5 16.5L21 21"/></svg>',
    plus: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M12 5v14M5 12h14"/></svg>',
    chevron: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M6 9l6 6 6-6"/></svg>',
    arrowUp: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M12 19V5M6 11l6-6 6 6"/></svg>',
    arrowDown: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M12 5v14M6 13l6 6 6-6"/></svg>',
    check: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M4 12.5l5 5L20 7"/></svg>',
    info: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/></svg>',
    refresh: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>',
    send: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M4 12l16-7-4 7 4 7-16-7z"/><path d="M4 12h9"/></svg>',
    external: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M14 4h6v6"/><path d="M20 4l-8 8"/><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/></svg>',
    filter: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M4 6h16M7 12h10M10 18h4"/></svg>',
    download: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><path d="M12 4v11M7 11l5 5 5-5"/><path d="M4 20h16"/></svg>',
  };

  const NAV = [
    { href: 'dashboard.html', icon: 'grid', label: '总览仪表盘', short: '总览', kbd: '1' },
    { href: 'holdings.html', icon: 'wallet', label: '持仓管理', short: '持仓', kbd: '2' },
    { href: 'analysis.html', icon: 'spark', label: 'AI 分析建议', short: '建议', kbd: '3' },
    { href: 'chat.html', icon: 'chat', label: '智能问答', short: '问答', kbd: '4' },
    { href: 'diagnosis.html', icon: 'shield', label: '组合诊断', short: '诊断', kbd: '5' },
  ];

  function chrome(opts) {
    const here = opts.active;
    const root = document.getElementById('sidebar');
    if (root) {
      root.innerHTML = `
        <a class="brand" href="index.html">
          <span class="brand-mark">${ICONS.spark}</span>
          <span><span class="brand-name">子木 · 股票智能助理</span><span class="brand-sub">个人投研工作台</span></span>
        </a>
        <div class="nav-group">工作台</div>
        ${NAV.map(n => `
          <a class="nav-item" href="${n.href}" ${n.href === here ? 'aria-current="page"' : ''}>
            ${n.icon && ICONS[n.icon]}<span>${n.label}</span><span class="kbd">${n.kbd}</span>
          </a>`).join('')}
        <a class="nav-item" href="wechat.html" ${here === 'wechat.html' ? 'aria-current="page"' : ''}>
          ${ICONS.wechat}<span>微信助理接入</span>
        </a>
        <div class="nav-group">快捷</div>
        <a class="nav-item" href="chat.html?q=${encodeURIComponent('今天该做什么？')}">${ICONS.chat}<span>问一句</span><span class="kbd">⌘K</span></a>
        <div class="side-foot">
          <div class="side-acct">
            <span class="side-acct-av">${ws.init}</span>
            <span class="side-acct-text"><span class="side-acct-name">${ws.name}</span><span class="side-acct-sub">${ws.broker}</span></span>
          </div>
          <div class="side-note">数据为演示样本，建议由模型生成，不构成投资意见。</div>
        </div>`;
    }
    const top = document.getElementById('topbar');
    if (top) {
      top.innerHTML = `
        <button class="icon-btn menu-toggle" id="menu-btn" aria-label="打开导航">${ICONS.filter}</button>
        <span class="crumb">${opts.crumb}</span>
        <label class="search">${ICONS.search}<input id="global-search" placeholder="搜代码 / 名称 / 问一句" autocomplete="off"><span class="kbd">⌘K</span></label>
        <div class="mkt" id="mkt-clock"></div>
        <button class="icon-btn" id="alert-btn" aria-label="预警">${ICONS.bell}</button>
        <button class="icon-btn" id="refresh-btn" aria-label="刷新行情">${ICONS.refresh}</button>`;
    }
    let strip = document.getElementById('strip');
    if (!strip && top) { top.insertAdjacentHTML('afterend', '<div class="strip" id="strip"></div>'); strip = document.getElementById('strip'); }
    if (strip) { strip.className = 'strip'; strip.innerHTML = stripHTML(); }

    const mt = document.getElementById('mtabs');
    if (mt) {
      const items = NAV.concat([{ href: 'wechat.html', icon: 'wechat', label: '微信助理接入', short: '微信' }]);
      mt.innerHTML = items.map(n => `
        <a class="mtab" href="${n.href}" ${n.href === here ? 'aria-current="page"' : ''}>${ICONS[n.icon]}<span>${n.short}</span></a>`).join('');
    }
    if (!document.querySelector('.toasts')) document.body.insertAdjacentHTML('beforeend', '<div class="toasts" id="toasts"></div>');
    bindChrome(opts);
  }

  function stripHTML() {
    return indices.map(i => `
      <span class="strip-item">
        <span class="strip-name">${i.name}</span>
        <span class="strip-v ${dirClass(i.chg)}">${fmt(i.v, 2)}</span>
        <span class="delta ${dirClass(i.chg)}">${i.chg > 0 ? '+' : ''}${i.chg.toFixed(2)}%</span>
      </span>`).join('')
      + `<span class="strip-item" style="border-right:0">
        <span class="strip-name">我的总资产</span>
        <span class="strip-v">${money(agg.totalAssets)}</span>
        <span class="delta ${dirClass(agg.dayPnl)}">今日 ${agg.dayPnl > 0 ? '+' : ''}${fmt(agg.dayPnl, 0)}</span>
      </span>`;
  }

  let clockTimer;
  function bindChrome(opts) {
    const clock = document.getElementById('mkt-clock');
    function paintClock() {
      if (!clock) return;
      const c = marketClock();
      clock.innerHTML = `<span class="dot ${c.dot}"></span><span>${c.state}</span>
        <span class="mkt-time">${c.time}</span><span class="mkt-next">${c.next}</span>`;
    }
    paintClock(); clearInterval(clockTimer); clockTimer = setInterval(paintClock, 1000);

    const menu = document.getElementById('menu-btn');
    const side = document.getElementById('sidebar');
    if (menu && side) menu.addEventListener('click', () => side.classList.toggle('on'));
    if (side) side.addEventListener('click', e => { if (e.target.closest('a')) side.classList.remove('on'); });

    const rb = document.getElementById('refresh-btn');
    // 手动刷新：先跳动价格并重算快照，再让页面通过 onRefresh 重绘自己的区块
    if (rb) rb.addEventListener('click', () => {
      tickPrices(true);
      if (typeof opts.onRefresh === 'function') opts.onRefresh();
    });

    const ab = document.getElementById('alert-btn');
    if (ab) ab.addEventListener('click', () => {
      toast(`待处理预警 ${alerts.filter(a => !a.done).length} 条 · 最近一条 ${alerts[0].time} ${alerts[0].name} ${alerts[0].type}`, 'info');
    });

    const gs = document.getElementById('global-search');
    if (gs) gs.addEventListener('keydown', e => {
      if (e.key === 'Enter' && gs.value.trim()) {
        const q = gs.value.trim();
        const hit = holdings.find(h => h.code === q || h.name.includes(q));
        location.href = hit ? 'analysis.html?code=' + hit.code : 'chat.html?q=' + encodeURIComponent(q);
      }
    });
    document.addEventListener('keydown', e => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); gs && gs.focus(); }
      if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
      const k = parseInt(e.key, 10);
      if (k >= 1 && k <= 6) location.href = k <= 5 ? NAV[k - 1].href : 'wechat.html';
    });
  }

  // 模拟行情跳动：小幅 tick，数字闪色；返回本次价格是否真的变动，
  // 页面可据此决定是否重绘（非交易时段自动跳动会跳过，避免无意义重渲染）
  let tickN = 0;
  function tickPrices(manual) {
    tickN++;
    if (!manual && !marketClock().live) return false;
    holdings.forEach(h => {
      const step = h.price * (Math.random() - 0.5) * (h.kind === 'cb' ? 0.0006 : 0.0009);
      const np = Math.max(0.001, h.price + step);
      const el = document.querySelector(`[data-tick="${h.code}"]`);
      if (el) {
        el.textContent = fmt(np, priceDec(h.kind));
        el.classList.remove('flash-up', 'flash-down');
        void el.offsetWidth;
        el.classList.add(np >= h.price ? 'flash-up' : 'flash-down');
      }
      h.price = np;
    });
    // 价格变了就必须整体重算并同步行情条，汇总不再是加载时常量
    recalc();
    const strip = document.getElementById('strip');
    if (strip) strip.innerHTML = stripHTML();
    if (manual) toast('行情已刷新 · ' + new Date().toTimeString().slice(0, 8), 'check');
    return true;
  }

  function toast(msg, kind) {
    const box = document.getElementById('toasts');
    if (!box) return;
    const el = document.createElement('div');
    el.className = 'toast';
    el.innerHTML = (ICONS[kind === 'check' ? 'check' : 'info']) + `<span>${msg}</span>`;
    box.appendChild(el);
    setTimeout(() => { el.classList.add('out'); setTimeout(() => el.remove(), 220); }, 3200);
  }

  ZM = {
    fmt, money, pct, wan, dirClass, priceDec, limitPrices, ICONS,
    ws, indices, holdings, kindLabel, watchlist, advice, alerts, qa, sessions,
    equity, P, wp, wsec, sellTo, weightAfter,
    marketClock, chrome, toast, tickPrices, stripHTML, recalc, totalMvAll,
  };
  W.ZM = ZM;
  publish(); // 把汇总快照（cash/totalAssets/sectors/hhi…）写入导出对象，此后每次 recalc() 自动同步
})(window);
