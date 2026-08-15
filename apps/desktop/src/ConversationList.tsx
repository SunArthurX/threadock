// 会话列表组件（筛选栏 + 列表项 + 子任务展开）
import { Conversation, sourceLabel, formatTime } from "./types";

interface Props {
  conversations: Conversation[];
  selectedConv: Conversation | null;
  loading: boolean;
  providerFilter: string | null;
  selectedWs: string | null;
  expandedParents: Set<string>;
  childConvs: Record<string, Conversation[]>;
  onFilter: (p: string | null) => void;
  onSelect: (c: Conversation) => void;
  onToggleExpand: (c: Conversation) => void;
  onClearWs: () => void;
}

export default function ConversationList({
  conversations, selectedConv, loading, providerFilter, selectedWs,
  expandedParents, childConvs, onFilter, onSelect, onToggleExpand, onClearWs,
}: Props) {
  const renderItem = (c: Conversation, isChild = false) => (
    <div key={c.id}>
      <div
        className={`list-item ${isChild ? "child-item" : ""} ${selectedConv?.id === c.id ? "active" : ""}`}
        onClick={() => onSelect(c)}
      >
        <div className="title">
          {!isChild && c.child_count > 0 && (
            <span
              className="expand-toggle"
              onClick={(e) => { e.stopPropagation(); onToggleExpand(c); }}
            >
              {expandedParents.has(c.id) ? "▼" : "▶"}
            </span>
          )}
          {isChild && <span className="child-arrow">↳</span>}
          {c.user_title ?? c.title ?? "(无标题)"}
        </div>
        <div className="meta">
          <span className={`badge source ${c.provider}`}>{sourceLabel(c.provider)}</span>
          <span className="meta-time">{formatTime(c.updated_at_ms)}</span>
          {!isChild && c.child_count > 0 && <span className="meta-child">{c.child_count} 子任务</span>}
          {c.model && <span className="meta-model">{c.model}</span>}
        </div>
      </div>
      {!isChild && expandedParents.has(c.id) && childConvs[c.id] && (
        <div className="child-list">
          {childConvs[c.id].length === 0 && <div className="child-empty">无子任务</div>}
          {childConvs[c.id].map((ch) => renderItem(ch, true))}
        </div>
      )}
    </div>
  );

  return (
    <>
      <div className="panel-header">
        会话 ({conversations.length})
        {selectedWs && <span className="clear-ws" onClick={onClearWs}>✕</span>}
      </div>
      <div className="filter-bar">
        <button
          className={`filter-chip ${providerFilter === null ? "active" : ""}`}
          onClick={() => onFilter(null)}
        >全部</button>
        {["zcode", "claude-code", "cursor", "minimax-code", "codex"].map((p) => (
          <button
            key={p}
            className={`filter-chip ${providerFilter === p ? "active" : ""}`}
            onClick={() => onFilter(p)}
          >{sourceLabel(p)}</button>
        ))}
      </div>
      {loading && (
        <div className="panel-loading"><div className="spinner spinner-sm" /><span>加载会话…</span></div>
      )}
      {!loading && conversations.map((c) => renderItem(c))}
      {!loading && conversations.length === 0 && (
        <div className="empty">{selectedWs ? "该项目下暂无会话" : "选择左侧项目"}</div>
      )}
    </>
  );
}
