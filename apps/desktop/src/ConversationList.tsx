// 会话列表组件（筛选栏 + 收藏星标 + 归档/删除视图 + 子任务展开）
import { Conversation, sourceLabel, formatTime } from "./types";

/** 列表视图维度：全部 / 收藏 / 已归档 / 已删除。 */
export type ListScope = "all" | "favorite" | "archived" | "deleted";

interface Props {
  conversations: Conversation[];
  selectedConv: Conversation | null;
  loading: boolean;
  providerFilter: string | null;
  selectedWs: string | null;
  expandedParents: Set<string>;
  childConvs: Record<string, Conversation[]>;
  scope: ListScope;
  onScopeChange: (s: ListScope) => void;
  /** 库中实际存在会话的来源（空集合 = 尚未加载，显示全部）。 */
  availableProviders?: Set<string>;
  onFilter: (p: string | null) => void;
  onSelect: (c: Conversation) => void;
  onToggleExpand: (c: Conversation) => void;
  onClearWs: () => void;
  /** 切换收藏（星标）。 */
  onToggleFavorite: (c: Conversation) => void;
  /** 已删除视图：恢复会话。 */
  onRestore?: (c: Conversation) => void;
}

const SCOPES: [ListScope, string][] = [
  ["all", "全部"],
  ["favorite", "★ 收藏"],
  ["archived", "已归档"],
  ["deleted", "已删除"],
];

export default function ConversationList({
  conversations, selectedConv, loading, providerFilter, selectedWs,
  expandedParents, childConvs, scope, onScopeChange, availableProviders, onFilter, onSelect,
  onToggleExpand, onClearWs, onToggleFavorite, onRestore,
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
          {c.favorite && <span className="fav-star" title="已收藏">★</span>}
          {c.archived && <span className="arch-badge" title="已归档">🗄</span>}
          {c.user_title ?? c.title ?? "(无标题)"}
        </div>
        <div className="meta">
          <span className={`badge source ${c.provider}`}>{sourceLabel(c.provider)}</span>
          <span className="meta-time">{formatTime(c.updated_at_ms)}</span>
          {!isChild && c.child_count > 0 && <span className="meta-child">{c.child_count} 子任务</span>}
          {c.model && <span className="meta-model">{c.model}</span>}
          {scope !== "deleted" ? (
            <span
              className={`fav-toggle ${c.favorite ? "on" : ""}`}
              title={c.favorite ? "取消收藏" : "收藏"}
              onClick={(e) => { e.stopPropagation(); onToggleFavorite(c); }}
            >
              {c.favorite ? "★" : "☆"}
            </span>
          ) : (
            <span
              className="restore-btn"
              title="恢复此会话"
              onClick={(e) => { e.stopPropagation(); onRestore?.(c); }}
            >
              ↩ 恢复
            </span>
          )}
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
      <div className="scope-bar">
        {SCOPES.map(([s, label]) => (
          <button
            key={s}
            className={`scope-chip ${scope === s ? "active" : ""}`}
            onClick={() => onScopeChange(s)}
          >{label}</button>
        ))}
      </div>
      <div className="filter-bar">
        <button
          className={`filter-chip ${providerFilter === null ? "active" : ""}`}
          onClick={() => onFilter(null)}
        >全部</button>
        {["zcode", "claude-code", "cursor", "minimax-code", "codex"]
          .filter((p) => !availableProviders || availableProviders.size === 0 || availableProviders.has(p))
          .map((p) => (
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
        <div className="empty">
          {scope === "deleted" ? "回收站为空" : selectedWs ? "该项目下暂无会话" : "选择左侧项目"}
        </div>
      )}
    </>
  );
}
