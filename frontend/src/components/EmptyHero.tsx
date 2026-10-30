import { Link } from "react-router-dom";
import { IconWallet } from "./Icons";

/**
 * 空态引导块（P1-T1 / A2）—— 镜像原型 ZM.emptyBlock()：图标 + 标题 + 说明 + CTA。
 * 默认 CTA「＋ 录入第一笔」→ /holdings?add=1（ADR-0006 手工逐笔入口）；
 * actions={null} 渲染无按钮轻量版（原型 cta:false 同位）。
 */

export interface EmptyHeroAction {
  label: string;
  to: string;
  primary?: boolean;
}

export interface EmptyHeroProps {
  /** 图标组件（缺省钱包；铃铛/走势等按区块语义传） */
  Icon?: () => React.JSX.Element;
  od?: string;
  title: string;
  desc?: string;
  actions?: EmptyHeroAction[] | null;
}

export default function EmptyHero({ Icon = IconWallet, od, title, desc, actions }: EmptyHeroProps) {
  const acts =
    actions === null ? [] : (actions ?? [{ label: "＋ 录入第一笔", to: "/holdings?add=1", primary: true }]);
  return (
    <div className="empty-hero" data-od-id={od}>
      <span className="eh-ic">
        <Icon />
      </span>
      <div className="eh-title">{title}</div>
      {desc && <div className="eh-desc">{desc}</div>}
      {acts.length > 0 && (
        <div className="eh-actions">
          {acts.map((a) => (
            <Link key={a.label} className={`btn ${a.primary ? "btn-primary" : ""}`} to={a.to}>
              {a.label}
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
