// 统一的 Skeleton 加载占位组件。
// 之前散落在 styles.css 里的 .sk-line / .sk-bar / .chart-skeleton 工具类现在
// 全部走这个组件，调用方用语义化的 variant 而不是直接写 className。
//
// 用法：
//   <Skeleton variant="text" width="60%" />
//   <Skeleton variant="block" height={120} />
//   <Skeleton variant="card" />            // 一整张卡片骨架
//   <Skeleton variant="kpi" />             // KPI 数字 + 标签
//   <Skeleton variant="chart-donut" />     // 圆环 + 右侧文字
//   <Skeleton variant="chart-bars" count={8} />  // 柱状图
//   <Skeleton variant="list" count={5} />  // 列表骨架（每行两条 + 头像圆）
//   <Skeleton variant="heatmap" />         // 热力图骨架（53 列 × 7 行）
//   <Skeleton.Group count={4} variant="kpi" />  // 一组（自动 flex 排）
import type { CSSProperties, ReactNode } from "react";

export type SkeletonVariant =
  | "text"
  | "block"
  | "circle"
  | "card"
  | "kpi"
  | "chart-donut"
  | "chart-bars"
  | "list"
  | "heatmap";

export interface SkeletonProps {
  variant?: SkeletonVariant;
  /** 自定义宽度（"60%" / "200px"） */
  width?: number | string;
  height?: number | string;
  /** 重复行数（kpi / list / chart-bars 用；text 也支持） */
  count?: number;
  className?: string;
  style?: CSSProperties;
  /** 是否显示脉动动画（默认 true） */
  animate?: boolean;
  /** 子节点（用于复合 variant 自定义内容） */
  children?: ReactNode;
}

export function Skeleton({
  variant = "text",
  width,
  height,
  count = 1,
  className = "",
  style,
  animate = true,
  children,
}: SkeletonProps) {
  const cls = ["sk-line", animate && "sk-anim"].filter(Boolean).join(" ");
  const sz: CSSProperties = {
    width: typeof width === "number" ? `${width}px` : width,
    height: typeof height === "number" ? `${height}px` : height,
    ...style,
  };

  if (variant === "text" || variant === "block") {
    const isBlock = variant === "block";
    return (
      <>
        {Array.from({ length: count }, (_, i) => (
          <div
            key={i}
            className={`${cls} ${isBlock ? "sk-block" : ""} ${className}`.trim()}
            style={{ ...sz, ...(i > 0 ? { marginTop: 6 } : {}) }}
          />
        ))}
      </>
    );
  }

  if (variant === "circle") {
    const sizePx = typeof width === "number" ? width : 36;
    return (
      <>
        {Array.from({ length: count }, (_, i) => (
          <div
            key={i}
            className={`sk-circle ${animate ? "sk-anim" : ""} ${className}`.trim()}
            style={{ width: sizePx, height: sizePx, ...sz, ...(i > 0 ? { marginLeft: 8 } : {}) }}
          />
        ))}
      </>
    );
  }

  if (variant === "card") {
    return (
      <div className={`sk-card ${className}`.trim()} style={style}>
        <div className={cls} style={{ width: "35%", height: 14, marginBottom: 12 }} />
        <div className={cls} style={{ width: "100%", height: 10, marginBottom: 6 }} />
        <div className={cls} style={{ width: "85%", height: 10, marginBottom: 6 }} />
        <div className={cls} style={{ width: "70%", height: 10 }} />
      </div>
    );
  }

  if (variant === "kpi") {
    return (
      <>
        {Array.from({ length: count }, (_, i) => (
          <div key={i} className={`ops-kpi skeleton sk-kpi ${className}`.trim()}>
            <div className="sk-circle sk-anim" style={{ width: 26, height: 26, marginBottom: 12, borderRadius: 7 }} />
            <div className={cls} style={{ width: "55%", height: 26, marginBottom: 8 }} />
            <div className={cls} style={{ width: "40%", height: 11, marginBottom: 4 }} />
            <div className={cls} style={{ width: "65%", height: 11 }} />
          </div>
        ))}
      </>
    );
  }

  if (variant === "chart-donut") {
    return (
      <div className={`chart-skeleton donut ${className}`.trim()} style={style}>
        <div className="sk-circle sk-anim" />
        <div className="sk-lines">
          <div className={cls} style={{ width: "85%" }} />
          <div className={cls} style={{ width: "70%" }} />
          <div className={cls} style={{ width: "60%" }} />
          <div className={cls} style={{ width: "75%" }} />
        </div>
      </div>
    );
  }

  if (variant === "chart-bars") {
    const rows = count || 8;
    return (
      <div className={`sk-bars ${className}`.trim()} style={{ height: typeof height === "number" ? height : 140, ...style }}>
        {Array.from({ length: rows }, (_, i) => (
          <div
            key={i}
            className="sk-bar sk-anim"
            style={{ height: `${30 + (i * 37) % 70}%` }}
          />
        ))}
      </div>
    );
  }

  if (variant === "list") {
    const rows = count || 5;
    return (
      <div className={`sk-list ${className}`.trim()} style={style}>
        {Array.from({ length: rows }, (_, i) => (
          <div key={i} className="sk-list-row">
            <div className="sk-circle sk-anim" style={{ width: 28, height: 28 }} />
            <div className="sk-list-body">
              <div className={cls} style={{ width: `${60 + (i * 7) % 25}%`, height: 11 }} />
              <div className={cls} style={{ width: `${30 + (i * 5) % 20}%`, height: 9, marginTop: 6 }} />
            </div>
          </div>
        ))}
      </div>
    );
  }

  if (variant === "heatmap") {
    // GitHub-style: 53 cols × 7 rows
    return (
      <div className={`sk-heatmap ${className}`.trim()} style={style}>
        {Array.from({ length: 7 }, (_, row) => (
          <div key={row} className="sk-heatmap-row">
            {Array.from({ length: 53 }, (_, col) => (
              <div
                key={col}
                className="sk-heatmap-cell sk-anim"
                style={{ animationDelay: `${(row * 53 + col) * 8}ms` }}
              />
            ))}
          </div>
        ))}
      </div>
    );
  }

  return <>{children}</>;
}

/** 一组骨架，自动 flex 排（适合 KPI/列表组） */
export function SkeletonGroup({
  count,
  variant = "kpi",
  gap = 12,
  className = "",
  style,
}: {
  count: number;
  variant?: SkeletonVariant;
  gap?: number;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <div
      className={`sk-group ${className}`.trim()}
      style={{ display: "flex", gap, ...style }}
    >
      <Skeleton variant={variant} count={count} />
    </div>
  );
}

export default Skeleton;
