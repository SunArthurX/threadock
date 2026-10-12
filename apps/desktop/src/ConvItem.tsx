// 单条会话列表项（React.memo：避免未变化的 item 随父级 state 一起重渲）。
// 父级负责传 per-item props（已经 useMemo 过）；此组件不持有任何 state。
import { memo } from "react";
import { t } from "./i18n";
import type { Conversation } from "./types";
import { sourceLabel, formatTime } from "./types";

export interface ConvItemProps {
  conv: Conversation;
  isChild: boolean;
  isActive: boolean;
  isSelected: boolean;
  isPinned: boolean;
  isExpanded: boolean;
  scope: "all" | "favorite" | "archived" | "deleted";
  childCount: number;
  /** 同名折叠组的成员数（>1 = 本行是折叠组头，展开显示其余同名会话；0 = 非组）。 */
  groupCount?: number;
  onItemClick: (e: React.MouseEvent) => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onToggleExpand: (e: React.MouseEvent) => void;
  onRestore: (e: React.MouseEvent) => void;
}

function ConvItemImpl({
  conv, isChild, isActive, isSelected, isPinned, isExpanded, scope, childCount, groupCount = 0,
  onItemClick, onContextMenu, onToggleExpand, onRestore,
}: ConvItemProps) {
  const isGroupHead = !isChild && groupCount > 1;
  return (
    <div
      data-conv-row={conv.id}
      className={`list-item ${isChild ? "child-item" : ""} ${isActive ? "active" : ""} ${isSelected ? "selected-multi" : ""} ${isPinned ? "pinned" : ""}`}
      onClick={onItemClick}
      onContextMenu={onContextMenu}
    >
      <div className="title">
        {(!isChild && childCount > 0) || isGroupHead ? (
          <span
            className="expand-toggle"
            onClick={onToggleExpand}
          >
            {isExpanded ? "▼" : "▶"}
          </span>
        ) : null}
        {isChild && <span className="child-arrow">↳</span>}
        {isPinned && <span className="pin-star" title={t("已置顶（永远排最前）")}>📌</span>}
        {conv.user_title ?? conv.title ?? t("(无标题)")}
      </div>
      <div className="meta">
        <span className={`badge source ${conv.provider}`}>{sourceLabel(conv.provider)}</span>
        <span className="meta-time">{formatTime(conv.updated_at_ms)}</span>
        {isGroupHead && <span className="meta-child dup-group-count" title={t("同名会话 {__0__} 条，点 ▶ 展开", { __0__: groupCount })}>×{groupCount}</span>}
        {!isChild && childCount > 0 && <span className="meta-child">{childCount} 子</span>}
        {conv.model && <span className="meta-model">{conv.model}</span>}
        {scope === "deleted" && (
          <span
            className="restore-btn"
            title={t("恢复此会话")}
            onClick={onRestore}
          >{t("↩ 恢复")}</span>
        )}
      </div>
    </div>
  );
}

export default memo(ConvItemImpl);
