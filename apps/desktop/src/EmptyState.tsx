// 统一空态 / 加载态 / 错误态 组件。
// 之前有 3 档 size × 3 档 tone，现在再加 4 档 state：default / loading / error / empty
// 用法：
//   <EmptyState icon="inbox" title="还没有任何会话" desc="..." action={...} />
//   <EmptyState state="loading" title="正在拉取数据…" />
//   <EmptyState state="error" title="拉取失败" desc="..." action={<button>重试</button>} />
//   <EmptyState state="empty" title="无匹配结果" />
import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";
import { Skeleton, SkeletonGroup } from "./Skeleton";

export type EmptyStateSize = "sm" | "md" | "lg";
export type EmptyStateTone = "default" | "muted" | "info";
export type EmptyStateState = "default" | "loading" | "error" | "empty";

export interface EmptyStateProps {
  icon?: IconName;
  title: string;
  desc?: ReactNode;
  action?: ReactNode;
  size?: EmptyStateSize;
  tone?: EmptyStateTone;
  state?: EmptyStateState;
  /** 状态码 / 错误细分（仅 error 时显示） */
  errorCode?: string | number;
}

export function EmptyState({
  icon = "inbox",
  title,
  desc,
  action,
  size = "md",
  tone = "default",
  state = "default",
  errorCode,
}: EmptyStateProps) {
  // state="loading"：走专门的 skeleton 风格，不显示图标
  if (state === "loading") {
    return (
      <div className={`empty-state empty-state-${size} empty-state-loading`}>
        <Skeleton variant="circle" width={size === "lg" ? 56 : size === "sm" ? 24 : 36} />
        <div className="empty-state-title">{title}</div>
        {desc && <div className="empty-state-desc">{desc}</div>}
        <SkeletonGroup count={size === "sm" ? 1 : 3} variant="text" gap={6} />
      </div>
    );
  }

  // state="error"：红色调，提示重试
  const toneClass = state === "error" ? "empty-state-error" : `empty-state-${tone}`;
  const displayIcon = state === "error" ? "alert" : icon;

  return (
    <div className={`empty-state empty-state-${size} ${toneClass}`}>
      <div className="empty-state-art" aria-hidden>
        <Icon name={displayIcon} size={size === "lg" ? 36 : size === "sm" ? 18 : 24} />
      </div>
      {errorCode !== undefined && (
        <span className="empty-state-code">{String(errorCode)}</span>
      )}
      <div className="empty-state-title">{title}</div>
      {desc && <div className="empty-state-desc">{desc}</div>}
      {action && <div className="empty-state-action">{action}</div>}
    </div>
  );
}

/** 表格内联的轻量"无数据"提示（区别于 EmptyState 的大块占位） */
export function InlineEmpty({
  message = "暂无数据",
  hint,
  action,
}: {
  message?: string;
  hint?: string;
  action?: ReactNode;
}) {
  return (
    <div className="ops-table-empty">
      <div>{message}</div>
      {hint && <div className="ops-table-empty-hint">{hint}</div>}
      {action && <div className="empty-state-action" style={{ marginTop: 4 }}>{action}</div>}
    </div>
  );
}

/** 加载中文本（替代 "加载中..." 这种裸字符串） */
export function LoadingText({ text = "加载中…", className = "" }: { text?: string; className?: string }) {
  return (
    <span className={`inline-loading ${className}`.trim()}>
      <Icon name="sync" size={11} className="inline-loading-spin" />
      {text}
    </span>
  );
}

export default EmptyState;
