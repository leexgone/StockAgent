# 05 · 设计系统

方向：现代极简（Linear / Vercel 感），浅色为基线。所有令牌集中在 `design/assets/ui.css:7` 的 `:root`，色值统一用 `oklch()`。

## 色彩令牌

| 组 | 令牌 | 值（oklch） | 用途 |
|---|---|---|---|
| 中性层 | `--bg` | `99% .002 240` | 页面底色 |
| | `--surface` | `100% 0 0` | 卡片面 |
| | `--surface-2` | `97.5% .004 240` | 次级面、轨道、图标底 |
| | `--fg` / `--fg-2` / `--muted` | `18% / 35% / 54%` | 主文、次文、辅助文 |
| | `--border` / `--border-2` | `92% / 88%` | 发丝边框、强调边框 |
| 交互 | `--accent` | `58% .18 255` | 主按钮、选中项、焦点环 |
| | `--accent-hover / -soft / -line` | | 悬停、浅底、描边态 |
| 涨跌 | `--up` | `55% .19 22`（**红**） | 上涨、正收益 |
| | `--down` | `50% .13 152`（**绿**） | 下跌、亏损 |
| | `--flat` | `60% .01 240` | 持平 |
| 状态（好坏轴） | `--warn` | `66% .14 78`（黄） | 提示、接近限制 |
| | `--danger` | `52% .2 22`（红） | 危险、违规、优先、资金不足 |
| | `--ok` | `55% .14 152`（绿） | 通过、安全、质量良好 |
| | `--warn-soft / --danger-soft / --ok-soft` | `96%` 浅底 | pill 与浅底态 |
| 交易方向 | `--buy` / `--sell`（含 `-soft`） | `--up` / `--down` 的别名 | 买卖、加减仓、结论标签——A 股红买绿卖，类名不借 `up/down` |

### 三条不可违背的语义规则

1. **红涨绿跌**（A 股惯例）。不要按西方习惯反过来，也不要给「好/坏」另设一套颜色还复用 `--up/--down`。
2. `--accent` **每屏不超过两处**，只用于交互态。它不参与涨跌与状态表达。
3. `--up/--down` 只作用于涨跌方向的数字与标记，**不做装饰**。好坏轴（合规/质量/反馈）用 `--ok/--warn/--danger`（`.ok/.warn/.danger` 类与同名 pill），交易方向（买卖、加减仓、结论标签）用 `.buy/.sell`（红买绿卖，`--up/--down` 别名令牌）。语义清理已随 P1-T4 完成（2026-10-28），逐类映射见 `03` 第 11 节。

## 排版

| 令牌 | 栈 |
|---|---|
| `--font-ui` | -apple-system / Segoe UI / PingFang SC / Microsoft YaHei |
| `--font-display` | SF Pro Display 优先，用于标题，`font-weight: 590`、`letter-spacing: -.02em` |
| `--font-mono` | SF Mono / JetBrains Mono / Cascadia Mono / ui-monospace |

- 正文 `14px / 1.55`，页面 `max-width: 1560px`、`padding: 24px`（`ui.css:208`）。
- **所有数字 `tabular-nums`**（`.num` / `.mono`，`ui.css:84`）；价格、代码、时间戳走 `--font-mono`。
- 中文不加斜体；英文技术词保持原词，不翻译字段名。

## 度量与形状

- 圆角三档：`--r-sm 6px` / `--r-md 10px` / `--r-lg 14px`。
- 间距基准 `--sp: 4px`，侧栏宽 `--side-w: 232px`。
- 卡片**无投影、只用 1px 发丝边框**；`--shadow-sm/-md/-lg` 仅用于浮层（抽屉、弹窗、hover 卡片）。
- 焦点环：`outline: 2px solid var(--accent); outline-offset: 2px`。

## 组件清单（`ui.css` 的分区）

`外壳（.shell/.sidebar/.topbar/.strip/.mtabs/.mtab）` · `页面骨架（.page/.page-head/.page-title/.eyebrow/.card/.grid/.g-2/.g-3/.g-4/.g-even/.g-main）` · `指标（.metric-label/.metric-value(-sm/-lg)/.metric-meta/.delta/.pill(.accent/.up/.down/.warn/.ok/.danger/.buy/.sell/.plain)）` · `表格（.tbl-wrap/.tbl）` · `控件（.btn/.btn-primary/.btn-sm/.seg/.input/.textarea/.select/.switch/.kbd/.hint/.ic）` · `图表（.chart/.spark/.crosshair-tip/.bar-line/.progress）` · `浮层（.scrim/.drawer/.modal/.toasts/.toast）` · `列表与时间线（.list/.list-item/.tl/.tl-item/.empty）` · `空态引导（.empty-hero，P1-T1，经 ZM.emptyBlock() 渲染）` · `页脚免责（.footnote）`

新增组件请放进 `ui.css` 对应分区（分区以 `/* === */` 注释标出），不要新建顶层分区。

## 响应式

| 断点 | 实际变化 |
|---|---|
| `≤1360` | `.g-main` 转单列；`.g-4` 转两列 |
| `≤1180` | `--side-w: 68px` 侧栏收成图标栏（隐藏品牌文字、导航文字、分组标题、账户信息与快捷键角标）；`.g-3` 转两列；页面内边距收紧 |
| `≤900` | 外壳转单列，侧栏变抽屉（默认 `translateX(-100%)`，`.on` 展开，带阴影与模糊底栏）；显示 `.mtabs`；`.g-2/-3/-4` 全部转单列；`.drawer` 满屏宽；指标字号下调；顶栏隐藏时段倒计时 |
| `≤520` | 页头操作区满宽且按钮等分；搜索框伸缩；卡片与列表内边距收紧 |
| `≤380` | 底栏标签与超大指标字号再降 |

另两条媒体查询：`prefers-reduced-motion: reduce` 把动画与过渡压到 0.01ms；`@media print` 隐藏外壳与浮层、卡片 `break-inside: avoid`（复盘页可直接打印/导出 PDF）。

宽内容（表格、图）必须包在 `overflow-x: auto` 容器里（`.tbl-wrap`），页面本体不允许横向滚动。

## 走查锚点

每个可评审的区块带 `data-od-id`：kebab-case，语义命名（`equity-card`、`trade-drawer`），重复项带数字下标（`alert-item-0`）。它们同时是：设计走查的定位坐标、自动化截图的选择器、以及文档里的引用锚点。新页面必须补全，改名要同步 `02-screens.md`。

## 无障碍底线

- 图标按钮必须有 `aria-label`（外壳已给 `menu-btn` / `alert-btn` / `refresh-btn`）。
- 当前页导航项用 `aria-current="page"`；分段控件用 `aria-pressed`。
- 正文对比度不低于 4.5:1 —— `--muted`（`oklch(54% …)`）在白底上已接近下限，**不要再往更浅调**，也不要把它用在正文长句上。
- 颜色不是唯一信息载体：触及纪律上限时除颜色外还有「已触及」标签与进度条。
- 抽屉 `Esc` 可关闭，遮罩点击可关闭，打开后自动聚焦数量输入框。
