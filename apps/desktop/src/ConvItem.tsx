// 单条会话列表项（React.memo：避免未变化的 item 随父级 state 一起重渲）。
// 父级负责传 per-item props（已经 useMemo 过）；此组件不持有任何 state。
import { memo } from "react";
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
  onItemClick: (e: React.MouseEvent) => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onToggleExpand: (e: React.MouseEvent) => void;
  onRestore: (e: React.MouseEvent) => void;
}

function ConvItemImpl({
  conv, isChild, isActive, isSelected, isPinned, isExpanded, scope, childCount,
  onItemClick, onContextMenu, onToggleExpand, onRestore,
}: ConvItemProps) {
  return (
    <div
      data-conv-row={conv.id}
      className={`list-item ${isChild ? "child-item" : ""} ${isActive ? "active" : ""} ${isSelected ? "selected-multi" : ""} ${isPinned ? "pinned" : ""}`}
      onClick={onItemClick}
      onContextMenu={onContextMenu}
    >
      <div className="title">
        {!isChild && childCount > 0 && (
          <span
            className="expand-toggle"
            onClick={onToggleExpand}
          >
            {isExpanded ? "▼" : "▶"}
          </span>
        )}
        {isChild && <span className="child-arrow">↳</span>}
        {isPinned && <span className="pin-star" title="已置顶（永远排最前）">📌</span>}
        {conv.user_title ?? conv.title ?? "(无标题)"}
      </div>
      <div className="meta">
        <span className={`badge source ${conv.provider}`}>{sourceLabel(conv.provider)}</span>
        <span className="meta-time">{formatTime(conv.updated_at_ms)}</span>
        {!isChild && childCount > 0 && <span className="meta-child">{childCount} 子</span>}
        {conv.model && <span className="meta-model">{conv.model}</span>}
        {scope === "deleted" && (
          <span
            className="restore-btn"
            title="恢复此会话"
            onClick={onRestore}
          >↩ 恢复</span>
        )}
      </div>
    </div>
  );
}

export default memo(ConvItemImpl);
