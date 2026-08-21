// 卡片标题组件：图标徽章 + 标题 + 副标题/操作区
// 用法：<CardTitle icon="chart">Agent 用量分布</CardTitle>
//       <CardTitle icon="flame" sub="高消耗检测">Token 浪费检测</CardTitle>
import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export interface CardTitleProps {
  icon?: IconName;
  children: ReactNode;
  sub?: ReactNode;
  trailing?: ReactNode;
  className?: string;
}

export function CardTitle({ icon, children, sub, trailing, className = "" }: CardTitleProps) {
  return (
    <div className={`ops-card-title ${className}`.trim()}>
      {icon && (
        <span className="card-icon">
          <Icon name={icon} size={13} />
        </span>
      )}
      <span className="card-title-text">{children}</span>
      {sub && <span className="card-title-sub">{sub}</span>}
      {trailing && <span className="card-title-trailing">{trailing}</span>}
    </div>
  );
}

export default CardTitle;
