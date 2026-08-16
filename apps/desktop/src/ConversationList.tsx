// 会话列表组件（筛选栏 + 收藏星标 + 归档/删除视图 + 子任务展开 + 日期范围筛选 + 批量操作 + 排序 + Pin 置顶）
import { useMemo, useState } from "react";
import { Conversation, sourceLabel, formatTime } from "./types";
import { showToast } from "./toast";

/** 列表视图维度：全部 / 收藏 / 已归档 / 已删除。 */
export type ListScope = "all" | "favorite" | "archived" | "deleted";

/** 日期快筛：今日 / 近 7 天 / 近 30 天 / 全部（默认全部）。 */
export type DateFilter = "all" | "today" | "week" | "month";

/** 排序方式：最新活动（默认）/ 创建时间 / 标题字母序。 */
export type SortBy = "updated" | "created" | "title";

const DATE_FILTERS: { key: DateFilter; label: string; days: number | null }[] = [
  { key: "all", label: "全部", days: null },
  { key: "today", label: "今日", days: 1 },
  { key: "week", label: "近 7 天", days: 7 },
  { key: "month", label: "近 30 天", days: 30 },
];

const SORT_OPTIONS: { key: SortBy; label: string }[] = [
  { key: "updated", label: "最新活动" },
  { key: "created", label: "创建时间" },
  { key: "title", label: "标题字母序" },
];

const PIN_KEY = "ch-conv-pins";

/** 读取置顶 ID 集合（localStorage 持久化）。 */
export function loadPinnedIds(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(PIN_KEY) ?? "[]") as string[]); }
  catch { return new Set(); }
}
function savePinnedIds(s: Set<string>) {
  try { localStorage.setItem(PIN_KEY, JSON.stringify([...s])); } catch { /* 静默 */ }
}

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
  /** 批量收藏（mode=true 收藏 / false 取消收藏） */
  onBulkFavorite?: (ids: string[], favorite: boolean) => Promise<void> | void;
  /** 批量归档 / 取消归档 */
  onBulkArchive?: (ids: string[], archived: boolean) => Promise<void> | void;
  /** 批量删除（软删 / 回收站） */
  onBulkDelete?: (ids: string[]) => Promise<void> | void;
  /** 批量加标签（每个 id 都会调一次 add_tag，未来可加 batch 接口） */
  onBulkAddTag?: (ids: string[], tag: string) => Promise<void> | void;
}

const SCOPES: [ListScope, string][] = [
  ["all", "全部"],
  ["favorite", "★ 收藏"],
  ["archived", "已归档"],
];

export default function ConversationList({
  conversations, selectedConv, loading, providerFilter, selectedWs,
  expandedParents, childConvs, scope, onScopeChange, availableProviders, onFilter, onSelect,
  onToggleExpand, onClearWs, onToggleFavorite, onRestore,
  onBulkFavorite, onBulkArchive, onBulkDelete, onBulkAddTag,
}: Props) {
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");
  const [sortBy, setSortBy] = useState<SortBy>(() => {
    try { const v = localStorage.getItem("ch-sort-by"); return (["updated","created","title"] as const).includes(v as SortBy) ? v as SortBy : "updated"; }
    catch { return "updated"; }
  });
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [pinned, setPinned] = useState<Set<string>>(loadPinnedIds);
  // 列表内文本搜索：仅在标题/标签上匹配（不查内容避免 FTS 调用）
  const [listSearch, setListSearch] = useState("");
  // 批量加标签：input + Enter 触发
  const [bulkTagInput, setBulkTagInput] = useState("");
  const togglePin = (id: string) => {
    setPinned((p) => {
      const n = new Set(p);
      if (n.has(id)) n.delete(id); else n.add(id);
      savePinnedIds(n);
      return n;
    });
  };
  // 日期过滤：基于 started_at_ms（前端内存过滤，不增加后端请求）
  const dateFiltered = useMemo(() => {
    const cfg = DATE_FILTERS.find((d) => d.key === dateFilter);
    if (!cfg?.days) return conversations;
    const cutoff = Date.now() - cfg.days * 86_400_000;
    return conversations.filter((c) => (c.started_at_ms ?? 0) >= cutoff);
  }, [conversations, dateFilter]);

  // 列表内搜索：在 user_title / title 上做大小写不敏感子串匹配
  const searchFiltered = useMemo(() => {
    const kw = listSearch.trim().toLowerCase();
    if (!kw) return dateFiltered;
    return dateFiltered.filter((c) => {
      const t1 = (c.user_title ?? "").toLowerCase();
      const t2 = (c.title ?? "").toLowerCase();
      return t1.includes(kw) || t2.includes(kw);
    });
  }, [dateFiltered, listSearch]);

  // 排序 + Pin 置顶优先（持久排序：state 变化时缓存到 localStorage）
  const sorted = useMemo(() => {
    const arr = [...searchFiltered];
    arr.sort((a, b) => {
      // Pin 优先：置顶的永远在最前
      const pa = pinned.has(a.id) ? 0 : 1;
      const pb = pinned.has(b.id) ? 0 : 1;
      if (pa !== pb) return pa - pb;
      if (sortBy === "title") {
        return (a.user_title ?? a.title ?? "").localeCompare(b.user_title ?? b.title ?? "");
      }
      const ka = sortBy === "updated" ? (a.updated_at_ms ?? 0) : (a.started_at_ms ?? 0);
      const kb = sortBy === "updated" ? (b.updated_at_ms ?? 0) : (b.started_at_ms ?? 0);
      return kb - ka;
    });
    return arr;
  }, [searchFiltered, sortBy, pinned]);
  const changeSortBy = (s: SortBy) => { setSortBy(s); try { localStorage.setItem("ch-sort-by", s); } catch { /* 静默 */ } };

  const toggleSelected = (id: string) => {
    setSelectedIds((cur) => {
      const n = new Set(cur);
      if (n.has(id)) n.delete(id); else n.add(id);
      return n;
    });
  };
  const clearSelection = () => setSelectedIds(new Set());
  const selectAllVisible = () => setSelectedIds(new Set(dateFiltered.map((c) => c.id)));

  const runBulk = async (label: string, ids: string[], fn: ((ids: string[]) => Promise<void> | void) | undefined) => {
    if (!fn) { showToast("批量操作不可用", "warn"); return; }
    if (ids.length === 0) return;
    try {
      await fn(ids);
      showToast(`✓ 已${label} ${ids.length} 条会话`, "info");
      clearSelection();
    } catch (e) { showToast(`失败：${String(e)}`, "error"); }
  };
  const renderItem = (c: Conversation, isChild = false) => (
    <div key={c.id}>
      <div
        className={`list-item ${isChild ? "child-item" : ""} ${selectedConv?.id === c.id ? "active" : ""} ${selectedIds.has(c.id) ? "selected-multi" : ""}`}
        onClick={() => onSelect(c)}
      >
        {!isChild && (
          <input
            type="checkbox"
            className="list-item-check"
            checked={selectedIds.has(c.id)}
            onClick={(e) => e.stopPropagation()}
            onChange={() => toggleSelected(c.id)}
            title="多选（可批量操作）"
          />
        )}
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
          {pinned.has(c.id) && !isChild && <span className="pin-star" title="已置顶">📌</span>}
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
            <>
              <span
                className={`pin-toggle ${pinned.has(c.id) ? "on" : ""}`}
                title={pinned.has(c.id) ? "取消置顶" : "置顶（永远排在最前）"}
                onClick={(e) => { e.stopPropagation(); togglePin(c.id); }}
              >
                {pinned.has(c.id) ? "📌" : "📍"}
              </span>
              <span
                className={`fav-toggle ${c.favorite ? "on" : ""}`}
                title={c.favorite ? "取消收藏" : "收藏"}
                onClick={(e) => { e.stopPropagation(); onToggleFavorite(c); }}
              >
                {c.favorite ? "★" : "☆"}
              </span>
            </>
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
        会话 ({listSearch ? `${searchFiltered.length}/${dateFiltered.length}` : dateFilter === "all" ? conversations.length : `${dateFiltered.length}/${conversations.length}`})
        {selectedWs && <span className="clear-ws" onClick={onClearWs}>✕</span>}
      </div>
      {/* 列表内文本搜索：标题/user_title 子串匹配（不查内容） */}
      <div className="list-search-row">
        <input
          className="list-search-input"
          type="search"
          placeholder="🔍 搜索会话标题…"
          value={listSearch}
          onChange={(e) => setListSearch(e.target.value)}
        />
        {listSearch && (
          <button className="list-search-clear" onClick={() => setListSearch("")} title="清空搜索">✕</button>
        )}
        {listSearch && (
          <span className="list-search-count">{searchFiltered.length} 匹配</span>
        )}
      </div>
      {selectedIds.size > 0 && (
        <div className="bulk-bar">
          <span>已选 <b>{selectedIds.size}</b> 条</span>
          <button className="bulk-btn" onClick={selectAllVisible} title="全选当前可见">全选</button>
          <button className="bulk-btn" onClick={clearSelection} title="清空选择">清空</button>
          {scope === "favorite" ? (
            <button className="bulk-btn" onClick={() => runBulk("取消收藏", [...selectedIds], onBulkFavorite ? (ids) => onBulkFavorite(ids, false) : undefined)}>☆ 取消收藏</button>
          ) : (
            <button className="bulk-btn" onClick={() => runBulk("收藏", [...selectedIds], onBulkFavorite ? (ids) => onBulkFavorite(ids, true) : undefined)}>★ 收藏</button>
          )}
          {scope === "archived" ? (
            <button className="bulk-btn" onClick={() => runBulk("取消归档", [...selectedIds], onBulkArchive ? (ids) => onBulkArchive(ids, false) : undefined)}>📤 取消归档</button>
          ) : (
            <button className="bulk-btn" onClick={() => runBulk("归档", [...selectedIds], onBulkArchive ? (ids) => onBulkArchive(ids, true) : undefined)}>🗄 归档</button>
          )}
          <input
            className="bulk-tag-input"
            placeholder="# 批量加标签…"
            value={bulkTagInput}
            onChange={(e) => setBulkTagInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && bulkTagInput.trim()) {
                const tag = bulkTagInput.trim().replace(/^#+/, "").trim();
                if (tag) runBulk(`加标签 #${tag}`, [...selectedIds], onBulkAddTag ? (ids) => onBulkAddTag(ids, tag) : undefined);
                setBulkTagInput("");
              }
            }}
            title="输入标签名后按 Enter（自动去 # 前缀）"
          />
          <button className="bulk-btn danger" onClick={() => runBulk("删除（入回收站）", [...selectedIds], onBulkDelete)}>🗑 删除</button>
        </div>
      )}
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
      <div className="filter-bar" style={{ paddingTop: 0 }}>
        <span style={{ fontSize: 10.5, opacity: 0.55, marginRight: 4 }}>日期</span>
        {DATE_FILTERS.map((d) => (
          <button
            key={d.key}
            className={`filter-chip ${dateFilter === d.key ? "active" : ""}`}
            onClick={() => setDateFilter(d.key)}
            title={d.days ? `仅显示最近 ${d.days} 天` : "全部时间"}
          >{d.label}</button>
        ))}
      </div>
      <div className="filter-bar" style={{ paddingTop: 0 }}>
        <span style={{ fontSize: 10.5, opacity: 0.55, marginRight: 4 }}>排序</span>
        {SORT_OPTIONS.map((s) => (
          <button
            key={s.key}
            className={`filter-chip ${sortBy === s.key ? "active" : ""}`}
            onClick={() => changeSortBy(s.key)}
          >{s.label}</button>
        ))}
        {pinned.size > 0 && (
          <span style={{ fontSize: 10.5, opacity: 0.55, marginLeft: "auto" }}>📌 {pinned.size} 置顶</span>
        )}
      </div>
      {loading && (
        <div className="panel-loading"><div className="spinner spinner-sm" /><span>加载会话…</span></div>
      )}
      {!loading && sorted.map((c) => renderItem(c))}
      {!loading && conversations.length === 0 && (
        <div className="empty">
          {scope === "deleted" ? "回收站为空" : selectedWs ? "该项目下暂无会话" : "选择左侧项目"}
        </div>
      )}
      {!loading && conversations.length > 0 && dateFiltered.length === 0 && (
        <div className="empty">当前日期范围无会话（试试「全部」）</div>
      )}
      {!loading && conversations.length > 0 && dateFiltered.length > 0 && searchFiltered.length === 0 && (
        <div className="empty">无匹配「{listSearch}」的会话（清空搜索试试）</div>
      )}
    </>
  );
}
