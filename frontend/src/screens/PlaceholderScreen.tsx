import { Link } from "react-router-dom";

/**
 * 未移植路由的占位屏（P5-T1）：T2 按 design/*.html 逐屏替换为真实屏，
 * 届时 routes.ts 对应项 ported 翻真。占位屏是临时脚手架，不要求免责区块。
 * P5-T2 收尾后仅剩 /wechat——它不是「移植中」而是卡决策闸门 C4（微信渠道
 * 方案待拍板），eyebrow/note 可由路由侧覆写以如实说明状态。
 */
export default function PlaceholderScreen({
  path,
  crumb,
  eyebrow = "P5-T2 移植中",
  note,
}: {
  path: string;
  crumb: string;
  eyebrow?: string;
  note?: string;
}) {
  return (
    <main className="page" id="main" data-od-id="placeholder-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">{eyebrow}</div>
          <h1 className="page-title">{crumb}</h1>
          <p className="page-desc">
            {note ?? `路由 ${path} 已就位，界面尚未从原型移植。设计参照见 design/ 目录同名页面。`}
          </p>
        </div>
        <div className="page-actions">
          <Link className="btn" to="/">返回工作台</Link>
        </div>
      </div>
    </main>
  );
}
