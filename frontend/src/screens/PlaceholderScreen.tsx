import { Link } from "react-router-dom";

/**
 * 未移植路由的占位屏（P5-T1）：T2 按 design/*.html 逐屏替换为真实屏，
 * 届时 routes.ts 对应项 ported 翻真。占位屏是临时脚手架，不要求免责区块。
 */
export default function PlaceholderScreen({ path, crumb }: { path: string; crumb: string }) {
  return (
    <main className="page" id="main" data-od-id="placeholder-page">
      <div className="page-head">
        <div>
          <div className="eyebrow">P5-T2 移植中</div>
          <h1 className="page-title">{crumb}</h1>
          <p className="page-desc">
            路由 <code>{path}</code> 已就位，界面尚未从原型移植。设计参照见 design/ 目录同名页面。
          </p>
        </div>
        <div className="page-actions">
          <Link className="btn" to="/">返回工作台</Link>
        </div>
      </div>
    </main>
  );
}
