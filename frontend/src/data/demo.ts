/* ============================================================
 * 演示数据层 —— P2-T4 index 静态复刻（移植自 design/assets/data.js）
 *
 * 边界说明：
 * - 数字唯一来源仍是原型 data.js；本文件是它的 TS 只读移植，
 *   公式与样本逐一对应，保证复刻页与原型显示同一批数字。
 * - 派生值（市值/盈亏/权重/HHI/净值指标）在模块加载时按原型
 *   recalc()/perf() 同口径算出，页面不出现写死的派生数字。
 * - 静态复刻不含价格跳动（tickPrices）；P4 接真实行情、P5 正式
 *   移植时本文件被后端 API + P3-T4 领域库取代。
 * ============================================================ */

/* ---------- 格式化工具（与 data.js 逐一相同） ---------- */
export const fmt = (v: number, d = 2): string =>
  Number(v).toLocaleString("zh-CN", { minimumFractionDigits: d, maximumFractionDigits: d });
export const money = (v: number): string => "¥" + fmt(v, 2);
export const pct = (v: number): string => (v > 0 ? "+" : "") + fmt(v, 2) + "%";
export const wan = (v: number): string => (v / 10000).toFixed(2) + " 万";
export const dirClass = (v: number): "up" | "down" | "flat" => (v > 0 ? "up" : v < 0 ? "down" : "flat");

/* ---------- 确定性随机（同一标的每次渲染曲线一致） ---------- */
function seed(str: string): number {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
function rng(s: string): () => number {
  let a = seed(s);
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
/** 带漂移的随机游走，末值锚定到 end */
function walk(key: string, n: number, end: number, vol: number, drift: number): number[] {
  const r = rng(key);
  const out: number[] = [];
  let v = 1;
  for (let i = 0; i < n; i++) {
    v *= 1 + (r() - 0.5) * vol + drift;
    out.push(v);
  }
  const k = end / out[n - 1];
  return out.map((x) => x * k);
}

/* ---------- 个人账户 ---------- */
export const ws = {
  name: "我的账户",
  init: "我",
  style: "均衡成长",
  cash: 46800.0,
  broker: "华泰证券 · 沪深 A 股账户（*8842）",
  wechat: "已绑定 · 微信",
};

/* ---------- 指数 ---------- */
export const indices = [
  { name: "上证指数", code: "000001.SH", v: 3902.45, chg: 0.62 },
  { name: "深证成指", code: "399001.SZ", v: 12648.3, chg: 0.38 },
  { name: "创业板指", code: "399006.SZ", v: 2556.12, chg: -0.44 },
  { name: "沪深300", code: "000300.SH", v: 4528.66, chg: 0.61 },
  { name: "中证转债", code: "000832.SH", v: 428.55, chg: 0.29 },
];

/* ---------- 持仓（股票 / ETF / 可转债） ---------- */
export type Kind = "stock" | "etf" | "cb";
export interface Holding {
  code: string;
  name: string;
  mkt: string;
  kind: Kind;
  sector: string;
  shares: number;
  cost: number;
  price: number;
  prev: number;
  tscore: number;
  /* 派生（加载时按 recalc 口径算出） */
  dayChg: number;
  dayVal: number;
  mv: number;
  pnl: number;
  pnlPct: number;
  weight: number;
}

const RAW_HOLDINGS: Omit<Holding, "dayChg" | "dayVal" | "mv" | "pnl" | "pnlPct" | "weight">[] = [
  { code: "600519", name: "贵州茅台", mkt: "SH", kind: "stock", sector: "食品饮料", shares: 100,  cost: 1512.0, price: 1618.0,  prev: 1601.5, tscore: 71 },
  { code: "300750", name: "宁德时代", mkt: "SZ", kind: "stock", sector: "电力设备", shares: 400,  cost: 196.4,  price: 212.35,  prev: 207.8,  tscore: 78 },
  { code: "002594", name: "比亚迪",   mkt: "SZ", kind: "stock", sector: "汽车",     shares: 500,  cost: 264.8,  price: 254.1,   prev: 259.4,  tscore: 44 },
  { code: "600036", name: "招商银行", mkt: "SH", kind: "stock", sector: "银行",     shares: 1800, cost: 33.85,  price: 37.05,   prev: 36.62,  tscore: 66 },
  { code: "600276", name: "恒瑞医药", mkt: "SH", kind: "stock", sector: "医药生物", shares: 700,  cost: 52.3,   price: 58.42,   prev: 57.1,   tscore: 74 },
  { code: "510300", name: "沪深300ETF", mkt: "SH", kind: "etf", sector: "宽基指数", shares: 12000, cost: 4.015, price: 4.291,   prev: 4.262,  tscore: 62 },
  { code: "588000", name: "科创50ETF",  mkt: "SH", kind: "etf", sector: "宽基指数", shares: 8000,  cost: 0.968, price: 1.086,   prev: 1.104,  tscore: 51 },
  { code: "110074", name: "精达转债",   mkt: "SH", kind: "cb",  sector: "有色金属", shares: 120,  cost: 118.5,  price: 126.72,  prev: 124.38, tscore: 69 },
  { code: "113650", name: "博威转债",   mkt: "SH", kind: "cb",  sector: "有色金属", shares: 80,   cost: 132.4,  price: 127.15,  prev: 128.6,  tscore: 48 },
  { code: "127022", name: "恒逸转债",   mkt: "SZ", kind: "cb",  sector: "基础化工", shares: 60,   cost: 106.8,  price: 105.35,  prev: 104.2,  tscore: 55 },
];
export const kindLabel: Record<Kind, string> = { stock: "股票", etf: "ETF / 基金", cb: "可转债" };

/* ---------- 派生与汇总（原型 recalc() 的加载期等价实现） ---------- */
const withDerived = RAW_HOLDINGS.map((h) => ({
  ...h,
  dayChg: ((h.price - h.prev) / h.prev) * 100,
  dayVal: (h.price - h.prev) * h.shares,
  mv: h.price * h.shares,
  pnl: (h.price - h.cost) * h.shares,
  pnlPct: ((h.price - h.cost) / h.cost) * 100,
  weight: 0,
}));
const totalMv0 = withDerived.reduce((s, h) => s + h.mv, 0);
const totalAssets0 = totalMv0 + ws.cash;
export const holdings: Holding[] = withDerived.map((h) => ({
  ...h,
  weight: totalAssets0 ? (h.mv / totalAssets0) * 100 : 0,
}));

export interface Sector {
  name: string;
  v: number;
  w: number;
}
export interface Agg {
  cash: number;
  mvStocks: number;
  mvEtf: number;
  mvCb: number;
  totalMv: number;
  totalAssets: number;
  dayPnl: number;
  dayPnlPct: number;
  totalPnl: number;
  totalPnlPct: number;
  costTotal: number;
  exposure: number;
  sectors: Sector[];
  hhi: number;
  top3Sector: number;
}

const sumBy = (k: Kind) => holdings.filter((h) => h.kind === k).reduce((s, h) => s + h.mv, 0);
const totalMv = sumBy("stock") + sumBy("etf") + sumBy("cb");
const totalAssets = totalMv + ws.cash;
const dayPnl = holdings.reduce((s, h) => s + h.dayVal, 0);
const totalPnl = holdings.reduce((s, h) => s + h.pnl, 0);
const costTotal = holdings.reduce((s, h) => s + h.cost * h.shares, 0);

const bySector: Record<string, number> = {};
holdings.forEach((h) => {
  bySector[h.sector] = (bySector[h.sector] || 0) + h.mv;
});
const sectors: Sector[] = Object.entries(bySector)
  .map(([name, v]) => ({ name, v, w: totalMv ? (v / totalMv) * 100 : 0 }))
  .sort((a, b) => b.v - a.v);

export const agg: Agg = {
  cash: ws.cash,
  mvStocks: sumBy("stock"),
  mvEtf: sumBy("etf"),
  mvCb: sumBy("cb"),
  totalMv,
  totalAssets,
  dayPnl,
  // 当日盈亏百分比分母 = 昨日总资产（= 当前总资产减去当日变动）
  dayPnlPct: totalAssets - dayPnl ? (dayPnl / (totalAssets - dayPnl)) * 100 : 0,
  totalPnl,
  totalPnlPct: costTotal ? (totalPnl / costTotal) * 100 : 0,
  costTotal,
  exposure: totalAssets ? (totalMv / totalAssets) * 100 : 0,
  sectors,
  hhi: sectors.reduce((s, x) => s + Math.pow(x.w, 2), 0) / 10000, // 归一化 0–1
  top3Sector: sectors.slice(0, 3).reduce((s, x) => s + x.w, 0),
};

/* ---------- 权益曲线（近 90 个交易日）与净值指标 ---------- */
export const equity: { dates: string[]; nav: number[]; bench: number[] } = { dates: [], nav: [], bench: [] };
{
  const n = 90;
  const port = walk("PORT90", n, agg.totalAssets, 0.018, 0.0009);
  const bench = walk("HS300_90", n, 4528.66 / (1 + 0.086), 0.011, 0.0008);
  const base = port[0];
  const d = new Date("2026-10-24T00:00:00");
  for (let i = 0; i < n; i++) {
    equity.nav.push(port[i]);
    equity.bench.push((bench[i] / bench[0]) * base);
  }
  // 交易日回溯
  const cur = new Date(d);
  let got = 0;
  const labels: string[] = [];
  while (got < n) {
    const dow = cur.getDay();
    if (dow !== 0 && dow !== 6) {
      labels.unshift(cur.getMonth() + 1 + "/" + cur.getDate());
      got++;
    }
    cur.setDate(cur.getDate() - 1);
  }
  equity.dates = labels;
}

export interface Perf {
  cum: number;
  benchCum: number;
  annVol: number;
  benchVol: number;
  mdd: number;
  benchMdd: number;
  sharpe: number;
  winRate: number;
}
function perf(series: number[], benchSeries: number[]): Perf {
  const ret: number[] = [];
  const vol: number[] = [];
  for (let i = 1; i < series.length; i++) {
    ret.push(series[i] / series[i - 1] - 1);
    vol.push(benchSeries[i] / benchSeries[i - 1] - 1);
  }
  const mean = ret.reduce((a, b) => a + b, 0) / ret.length;
  const sd = Math.sqrt(ret.reduce((a, b) => a + (b - mean) ** 2, 0) / ret.length);
  let peak = series[0];
  let mdd = 0;
  series.forEach((v) => {
    peak = Math.max(peak, v);
    mdd = Math.min(mdd, (v - peak) / peak);
  });
  let bpeak = benchSeries[0];
  let bmdd = 0;
  benchSeries.forEach((v) => {
    bpeak = Math.max(bpeak, v);
    bmdd = Math.min(bmdd, (v - bpeak) / bpeak);
  });
  const cum = series[series.length - 1] / series[0] - 1;
  const bcum = benchSeries[benchSeries.length - 1] / benchSeries[0] - 1;
  return {
    cum: cum * 100,
    benchCum: bcum * 100,
    annVol: sd * Math.sqrt(244) * 100,
    benchVol: Math.sqrt(vol.reduce((a, b) => a + b * b, 0) / vol.length) * Math.sqrt(244) * 100,
    mdd: mdd * 100,
    benchMdd: bmdd * 100,
    sharpe: (mean * 244) / (sd * Math.sqrt(244) + 1e-9),
    winRate: (ret.filter((r, i) => r > vol[i]).length / ret.length) * 100,
  };
}
export const P: Perf = perf(equity.nav, equity.bench);

/* ---------- 预警 ---------- */
export interface Alert {
  time: string;
  code: string;
  name: string;
  type: string;
  level: "up" | "down" | "warn" | "flat";
  text: string;
  done: boolean;
}
const H = (c: string): Holding => holdings.find((h) => h.code === c)!;
const BYD = H("002594");
export const alerts: Alert[] = [
  { time: "10:32", code: "300750", name: "宁德时代", type: "放量突破", level: "up", text: "成交额明显高于 20 日均量，价格站上 60 日均线", done: false },
  { time: "09:58", code: "002594", name: "比亚迪", type: "跌破成本", level: "down", text: `现价 ${fmt(BYD.price, 2)} 低于成本 ${fmt(BYD.cost, 2)}，浮亏扩大至 ${pct(BYD.pnlPct)}`, done: false },
  { time: "14:47", code: "588000", name: "科创50ETF", type: "止盈提醒", level: "warn", text: `累计收益 ${pct(H("588000").pnlPct)}，触及你设定的 +12% 止盈线`, done: false },
  { time: "08:55", code: "113650", name: "博威转债", type: "强赎公告", level: "warn", text: "正股触发有条件赎回条款，最后交易日 11-20，溢价率 12.4%", done: true },
  { time: "昨 15:05", code: "600036", name: "招商银行", type: "盘后复盘", level: "flat", text: "已连续 3 日跑赢银行指数，PB 0.96 处近 3 年 31 分位", done: true },
];

/* ---------- AI 分析建议库（index 屏只用到覆盖数；完整库 P5 复刻 analysis 屏时移植） ---------- */
export const adviceCodes = ["300750", "002594", "600519", "600036", "600276", "510300", "588000", "110074", "113650", "127022"];

/* ---------- 对话式问股 样本（index 屏只用到问题数；完整问答 P5 移植） ---------- */
export const qa = [
  { q: "比亚迪今天亏了，要不要止损？" },
  { q: "帮我把组合调均衡一点" },
  { q: "博威转债是不是要赎回了？" },
];

/* ---------- 最近会话（时间线数据源） ---------- */
export interface SessionItem {
  t: string;
  who: string;
  text: string;
  reply: string;
  read: boolean;
}
export const sessions: SessionItem[] = [
  { t: "10-24 09:12", who: "微信 · 语音", text: "「茅台今天能不能补？」", reply: "已答：28% 集中度已到上限，建议回落到 25% 以下再补。", read: true },
  { t: "10-23 20:41", who: "定时任务", text: "盘后复盘日报已推送到我的微信", reply: "", read: true },
  { t: "10-23 14:07", who: "微信 · 文字", text: "「科创50 跌了 2 个点，要不要跑？」", reply: "已答：仓位仅 1.5%，属于承受范围内，建议持有观察 10 日线。", read: false },
];

/* ---------- 市场时钟（真实时段逻辑，与 data.js 相同；节假日表接真实日历后扩展） ---------- */
export interface Clock {
  state: string;
  dot: string;
  next: string;
  time: string;
  live: boolean;
}
function cd(m: number): string {
  const mm = Math.max(0, Math.round(m));
  const h = Math.floor(mm / 60);
  return (h ? h + " 小时 " : "") + (mm % 60) + " 分";
}
export function marketClock(now: Date): Clock {
  const dow = now.getDay();
  const mins = now.getHours() * 60 + now.getMinutes();
  const hhmm = now.toTimeString().slice(0, 8);
  const isTrade = dow >= 1 && dow <= 5;
  if (!isTrade) {
    const next = new Date(now);
    next.setDate(next.getDate() + (dow === 6 ? 2 : 1));
    return { state: "休市", dot: "closed", next: "下一交易日 " + (next.getMonth() + 1) + "/" + next.getDate() + " 09:15 集合竞价", time: hhmm, live: false };
  }
  const at = (h: number, m: number) => h * 60 + m;
  if (mins < at(9, 15)) return { state: "盘前", dot: "closed", next: "距集合竞价 " + cd(at(9, 15) - mins), time: hhmm, live: false };
  if (mins < at(9, 30)) return { state: "集合竞价", dot: "live", next: "距开盘 " + cd(at(9, 30) - mins), time: hhmm, live: true };
  if (mins < at(11, 30)) return { state: "交易中 · 早盘", dot: "live", next: "距午间休市 " + cd(at(11, 30) - mins), time: hhmm, live: true };
  if (mins < at(13, 0)) return { state: "午间休市", dot: "lunch", next: "距下午开盘 " + cd(at(13, 0) - mins), time: hhmm, live: false };
  if (mins < at(15, 0)) return { state: "交易中 · 午盘", dot: "live", next: "距收盘 " + cd(at(15, 0) - mins), time: hhmm, live: true };
  if (mins < at(15, 30)) return { state: "盘后定价交易", dot: "lunch", next: "复盘时段 · 次日 09:15 集合竞价", time: hhmm, live: false };
  return { state: "已收盘", dot: "closed", next: "距次日集合竞价 " + cd(at(9, 15) + 24 * 60 - mins), time: hhmm, live: false };
}

/* ---------- 导航（P5-T1：路由已定，path 对应 routes.ts 路由表；未移植屏渲染占位屏） ---------- */
export interface NavItem {
  path: string; // 路由路径（src/routes.ts 单一来源）
  icon: string;
  label: string;
  short: string;
  kbd?: string;
}
export const NAV: NavItem[] = [
  { path: "/dashboard", icon: "grid", label: "总览仪表盘", short: "总览", kbd: "1" },
  { path: "/holdings", icon: "wallet", label: "持仓管理", short: "持仓", kbd: "2" },
  { path: "/analysis", icon: "spark", label: "AI 分析建议", short: "建议", kbd: "3" },
  { path: "/chat", icon: "chat", label: "智能问答", short: "问答", kbd: "4" },
  { path: "/diagnosis", icon: "shield", label: "组合诊断", short: "诊断", kbd: "5" },
];
export const WECHAT_NAV: NavItem = { path: "/wechat", icon: "wechat", label: "微信助理接入", short: "微信" };
