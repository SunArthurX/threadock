// 会话列表组件
// 第 11 轮大改版：4 行 filter-bar 合并成 1 行 toolbar（3 个 dropdown + 搜索 + 数量）；
// 列表项去掉复选框 / hover-pin-toggle / hover-fav-toggle —— 全部走右键菜单（⌘点击多选 + ⌘A 全选 + ⋯ / 右键触发）。
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Conversation, sourceLabel } from "./types";
import { showToast } from "./toast";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import ConvItem from "./ConvItem";

/** 列表视图维度：全部 / 收藏 / 已归档 / 已删除。 */
export type ListScope = "all" | "favorite" | "archived" | "deleted";

/** 日期快筛：今日 / 近 7 天 / 近 30 天 / 全部（默认全部）。 */
export type DateFilter = "all" | "today" | "week" | "month";

/** 排序方式：最新活动（默认）/ 创建时间 / 标题字母序。 */
export type SortBy = "updated" | "created" | "title";

const DATE_FILTERS: { key: DateFilter; label: string; days: number | null }[] = [
  { key: "all", label: "全部时间", days: null },
  { key: "today", label: "今日", days: 1 },
  { key: "week", label: "近 7 天", days: 7 },
  { key: "month", label: "近 30 天", days: 30 },
];

const SORT_OPTIONS: { key: SortBy; label: string; icon: string }[] = [
  { key: "updated", label: "最新活动", icon: "🕐" },
  { key: "created", label: "创建时间", icon: "📅" },
  { key: "title", label: "标题字母序", icon: "🔤" },
];

const SCOPE_OPTIONS: { key: ListScope; label: string; icon: string }[] = [
  { key: "all", label: "全部会话", icon: "💬" },
  { key: "favorite", label: "收藏", icon: "★" },
  { key: "archived", label: "已归档", icon: "🗄" },
  { key: "deleted", label: "回收站", icon: "🗑" },
];

const PIN_KEY = "ch-conv-pins";
const SORT_KEY = "ch-sort-by";
const DATE_KEY = "ch-date-filter";

/** 读取置顶 ID 集合（localStorage 持久化）。 */
export function loadPinnedIds(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(PIN_KEY) ?? "[]") as string[]); }
  catch { return new Set(); }
}
function savePinnedIds(s: Set<string>) {
  try { localStorage.setItem(PIN_KEY, JSON.stringify([...s])); } catch { /* 静默 */ }
}

/** 读受控 / 默认值（受控时优先 props）。 */
function loadLS<T extends string>(key: string, valid: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return (valid as readonly string[]).includes(v ?? "") ? (v as T) : fallback;
  } catch { return fallback; }
}

/** 通用 dropdown 控件：按钮 + 点击展开面板（点外自动收起）。 */
function Dropdown<T extends string>({
  label, value, options, onChange, align = "left",
}: {
  /** 在按钮左侧显示的小 label（如「视图：」「排序：」），让用户一眼明白这是干什么的。 */
  label?: string;
  value: T;
  options: { key: T; label: string; icon?: string }[];
  onChange: (v: T) => void;
  align?: "left" | "right";
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const cur = options.find((o) => o.key === value) ?? options[0];
  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onClick);
    return () => window.removeEventListener("mousedown", onClick);
  }, [open]);
  return (
    <div className={`list-dropdown ${open ? "open" : ""}`} ref={ref}>
      {label && <span className="list-dropdown-label-text">{label}</span>}
      <button
        className={`list-dropdown-btn ${open ? "active" : ""}`}
        onClick={() => setOpen((o) => !o)}
        title={label}
      >
        {cur?.icon && <span className="list-dropdown-icon">{cur.icon}</span>}
        <span className="list-dropdown-label">{cur?.label ?? ""}</span>
        <span className="list-dropdown-caret">▾</span>
      </button>
      {open && (
        <div className={`list-dropdown-panel ${align === "right" ? "right" : ""}`}>
          {options.map((o) => (
            <button
              key={o.key}
              className={`list-dropdown-item ${o.key === value ? "active" : ""}`}
              onClick={() => { onChange(o.key); setOpen(false); }}
            >
              {o.icon && <span className="list-dropdown-icon">{o.icon}</span>}
              <span>{o.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
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
  onToggleFavorite?: (c: Conversation) => void;
  /** 单条归档/取消归档（右键菜单）。 */
  onArchiveOne?: (c: Conversation) => void;
  /** 单条删除（带 undo）。 */
  onDeleteOne?: (c: Conversation) => void;
  /** 复制标题。 */
  onCopyTitle?: (c: Conversation) => void;
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
  /** 批量拆分：把选中会话移到新 Workspace（plan §4.3 手动拆分，v1.0.0） */
  onBulkSplit?: (ids: string[], newWorkspaceName: string) => Promise<void> | void;
}

export default function ConversationList({
  conversations, selectedConv, loading, providerFilter, selectedWs,
  expandedParents, childConvs, scope, onScopeChange, availableProviders, onFilter, onSelect,
  onToggleExpand, onClearWs, onToggleFavorite, onArchiveOne, onDeleteOne, onCopyTitle, onRestore,
  onBulkFavorite, onBulkArchive, onBulkDelete, onBulkAddTag, onBulkSplit,
}: Props) {
  const [dateFilter, setDateFilter] = useState<DateFilter>(() => loadLS(DATE_KEY, ["all", "today", "week", "month"] as const, "all"));
  const [sortBy, setSortBy] = useState<SortBy>(() => loadLS(SORT_KEY, ["updated", "created", "title"] as const, "updated"));
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [pinned, setPinned] = useState<Set<string>>(loadPinnedIds);
  // 列表内文本搜索：仅在标题/标签上匹配（不查内容避免 FTS 调用）
  const [listSearch, setListSearch] = useState("");
  // 批量加标签：input + Enter 触发
  const [bulkTagInput, setBulkTagInput] = useState("");
  // 右键菜单：当前点击的会话 + 屏幕坐标
  const [ctxMenu, setCtxMenu] = useState<{ conv: Conversation; x: number; y: number } | null>(null);
  // 右键菜单触发的「加标签」内联输入：避免原生 window.prompt 阻断流程
  const [tagInput, setTagInput] = useState<{ ids: string[]; count: number; value: string; x: number; y: number } | null>(null);
  // 虚拟列表的滚动容器 ref
  const parentRef = useRef<HTMLDivElement>(null);

  const togglePin = (id: string) => {
    setPinned((p) => {
      const n = new Set(p);
      if (n.has(id)) n.delete(id); else n.add(id);
      savePinnedIds(n);
      return n;
    });
  };

  // 持久化偏好
  useEffect(() => { try { localStorage.setItem(SORT_KEY, sortBy); } catch { /* 静默 */ } }, [sortBy]);
  useEffect(() => { try { localStorage.setItem(DATE_KEY, dateFilter); } catch { /* 静默 */ } }, [dateFilter]);

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

  // 排序 + Pin 置顶优先
  const sorted = useMemo(() => {
    const arr = [...searchFiltered];
    arr.sort((a, b) => {
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

  // ⌘点击多选 / ⌘A 全选 / Esc 清空
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && (e.key === "a" || e.key === "A")) {
        // 仅在搜索输入框不 focus 时才拦截
        const tag = (document.activeElement as HTMLElement | null)?.tagName ?? "";
        if (tag === "INPUT" || tag === "TEXTAREA") return;
        e.preventDefault();
        if (selectedIds.size === sorted.length) {
          setSelectedIds(new Set());
        } else {
          setSelectedIds(new Set(sorted.map((c) => c.id)));
        }
      } else if (e.key === "Escape" && selectedIds.size > 0) {
        setSelectedIds(new Set());
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedIds, sorted]);

  const handleItemClick = useCallback((c: Conversation, e: React.MouseEvent) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod) {
      e.preventDefault();
      setSelectedIds((cur) => {
        const n = new Set(cur);
        if (n.has(c.id)) n.delete(c.id); else n.add(c.id);
        return n;
      });
    } else {
      // 单击：清空多选、选中
      if (selectedIds.size > 0) setSelectedIds(new Set());
      onSelect(c);
    }
  }, [selectedIds, onSelect]);

  const handleContextMenu = useCallback((c: Conversation, e: React.MouseEvent) => {
    e.preventDefault();
    // 如果右键的不是已选中的项，先单选它
    if (!selectedIds.has(c.id) && selectedIds.size <= 1) {
      setSelectedIds(new Set());
      onSelect(c);
    } else if (!selectedIds.has(c.id)) {
      setSelectedIds(new Set([c.id]));
    }
    setCtxMenu({ conv: c, x: e.clientX, y: e.clientY });
  }, [selectedIds, onSelect]);

  const buildMenu = (c: Conversation, c_x: number, c_y: number): MenuItem[] => {
    const isMulti = selectedIds.size > 1 && selectedIds.has(c.id);
    const targetCount = isMulti ? selectedIds.size : 1;
    const targetIds = isMulti ? [...selectedIds] : [c.id];
    const items: MenuItem[] = [];
    if (scope !== "deleted") {
      items.push({
        icon: c.favorite ? "☆" : "★",
        label: isMulti ? `${c.favorite ? "取消收藏" : "收藏"} ${targetCount} 条` : (c.favorite ? "取消收藏" : "收藏"),
        onClick: async () => {
          const fn = onBulkFavorite ? (ids: string[]) => onBulkFavorite(ids, !c.favorite) : undefined;
          if (fn) { await fn(targetIds); showToast(`✓ ${!c.favorite ? "已收藏" : "已取消收藏"} ${targetCount} 条`, "info"); }
          else onToggleFavorite?.(c);
        },
        group: 1,
      });
      items.push({
        icon: c.archived ? "📤" : "🗄",
        label: isMulti ? `${c.archived ? "取消归档" : "归档"} ${targetCount} 条` : (c.archived ? "取消归档" : "归档"),
        onClick: async () => {
          if (isMulti) {
            const fn = onBulkArchive ? (ids: string[]) => onBulkArchive(ids, !c.archived) : undefined;
            if (fn) { await fn(targetIds); showToast(`✓ ${!c.archived ? "已归档" : "已取消归档"} ${targetCount} 条`, "info"); }
          } else if (onArchiveOne) onArchiveOne(c);
        },
        group: 1,
      });
      items.push({
        icon: pinned.has(c.id) ? "📍" : "📌",
        label: pinned.has(c.id) ? "取消置顶" : "置顶（排在最前）",
        onClick: () => togglePin(c.id),
        group: 1,
      });
    }
    if (scope === "deleted") {
      items.push({
        icon: "↩",
        label: `恢复此${isMulti ? ` ${targetCount} 条` : ""}会话`,
        onClick: () => {
          for (const id of targetIds) {
            const cc = conversations.find((x) => x.id === id);
            if (cc) onRestore?.(cc);
          }
        },
        group: 1,
      });
    } else {
      items.push({
        icon: "🏷",
        label: `加标签${isMulti ? `到 ${targetCount} 条` : ""}…`,
        onClick: () => {
          // 打开内联输入（位置贴 context menu 下方），不在此处用 window.prompt 阻断流程
          setTagInput({ ids: targetIds, count: targetCount, value: "", x: c_x, y: c_y });
        },
        group: 1,
      });
      items.push({
        icon: "📋",
        label: "复制标题",
        onClick: () => { if (onCopyTitle) onCopyTitle(c); else { navigator.clipboard?.writeText(c.user_title ?? c.title ?? "").then(() => showToast("✓ 标题已复制", "info", 1500)).catch(() => showToast("剪贴板不可用", "error")); } },
        group: 2,
      });
      items.push({
        icon: "🗑",
        label: isMulti ? `删除 ${targetCount} 条（带撤销）` : "删除（带撤销）",
        danger: true,
        onClick: () => {
          const fn = onBulkDelete ? (ids: string[]) => onBulkDelete(ids) : undefined;
          if (fn) fn(targetIds);
          else if (onDeleteOne) onDeleteOne(c);
        },
        group: 3,
      });
    }
    return items;
  };

  // per-item flags（useMemo Map：避免每行重新计算 set.has）
  const itemFlags = useMemo(() => {
    const map = new Map<string, { isActive: boolean; isSelected: boolean; isPinned: boolean; isExpanded: boolean; isChild: boolean }>();
    for (const c of sorted) {
      map.set(c.id, {
        isActive: selectedConv?.id === c.id,
        isSelected: selectedIds.has(c.id),
        isPinned: pinned.has(c.id),
        isExpanded: expandedParents.has(c.id),
        isChild: false,
      });
    }
    return map;
  }, [sorted, selectedConv, selectedIds, pinned, expandedParents]);

  // 虚拟列表：默认关闭（getScrollElement 返 null → virtualizer 用 initialRect）。
  // 仅当父级真有可滚动高度（>0）时才打开：jsdom 下高度为 0，关闭即走 initialRect 兜底。
  const [enableVirtualization, setEnableVirtualization] = useState(false);
  useEffect(() => {
    const el = parentRef.current;
    if (el && el.getBoundingClientRect().height > 0) {
      setEnableVirtualization(true);
    }
  }, [sorted.length]);
  // TanStack Virtual 的 useVirtualizer 返回不可安全 memo 化的函数句柄，
  // 属第三方库 API 设计与 React Compiler 的兼容性限制，非本仓库代码问题。
  // eslint-disable-next-line react-hooks/incompatible-library
  const rowVirtualizer = useVirtualizer({
    count: sorted.length,
    getScrollElement: () => (enableVirtualization ? parentRef.current : null),
    estimateSize: () => 80,
    overscan: 8,
    initialRect: { width: 0, height: 600 },
  });

  // 来源 chip：仅显示有数据的来源
  const providerChips = (["zcode", "claude-code", "cursor", "minimax-code", "codex"] as const)
    .filter((p) => !availableProviders || availableProviders.size === 0 || availableProviders.has(p));

  // 提交右键菜单触发的「加标签」内联输入
  const submitTagInput = async () => {
    if (!tagInput) return;
    const tag = tagInput.value.trim().replace(/^#+/, "").trim();
    setTagInput(null);
    setCtxMenu(null);
    if (!tag) return;
    if (onBulkAddTag) {
      await onBulkAddTag(tagInput.ids, tag);
      showToast(`✓ 已加标签 #${tag} 到 ${tagInput.count} 条`, "info");
    }
  };

  return (
    <>
      <div className="panel-header">
        会话
        <span className="panel-header-count">
          {listSearch
            ? `${searchFiltered.length} / ${dateFiltered.length}`
            : dateFilter === "all"
              ? conversations.length
              : `${dateFiltered.length} / ${conversations.length}`}
        </span>
        {selectedWs && <span className="clear-ws" onClick={onClearWs}>✕</span>}
      </div>

      {/* 工具栏：搜索 + 3 个 dropdown（视图/日期/排序） + 来源 chip 折叠 */}
      <div className="list-toolbar">
        <input
          className="list-search-input"
          type="search"
          placeholder="🔍 搜索标题…"
          value={listSearch}
          onChange={(e) => setListSearch(e.target.value)}
        />
        {listSearch && (
          <button className="list-search-clear" onClick={() => setListSearch("")} title="清空搜索">✕</button>
        )}
        <div className="list-toolbar-row">
          <Dropdown
            label="视图"
            value={scope}
            options={SCOPE_OPTIONS}
            onChange={onScopeChange}
          />
          <Dropdown
            label="日期"
            value={dateFilter}
            options={DATE_FILTERS}
            onChange={setDateFilter}
          />
          <Dropdown
            label="排序"
            value={sortBy}
            options={SORT_OPTIONS}
            onChange={setSortBy}
            align="right"
          />
        </div>
        {providerChips.length > 1 && (
          <div className="list-provider-chips">
            <button
              className={`provider-chip ${providerFilter === null ? "active" : ""}`}
              onClick={() => onFilter(null)}
              title="显示全部来源"
            >全部</button>
            {providerChips.map((p) => (
              <button
                key={p}
                className={`provider-chip ${providerFilter === p ? "active" : ""}`}
                onClick={() => onFilter(providerFilter === p ? null : p)}
                title={sourceLabel(p)}
              >{sourceLabel(p)}</button>
            ))}
          </div>
        )}
      </div>

      {/* 多选操作栏：仅在 selectedIds > 0 时显示 */}
      {selectedIds.size > 0 && (
        <div className="bulk-bar">
          <span>已选 <b>{selectedIds.size}</b> 条</span>
          <button className="bulk-btn" onClick={() => setSelectedIds(new Set(sorted.map((c) => c.id)))} title="全选当前可见（⌘A）">全选</button>
          <button className="bulk-btn" onClick={() => setSelectedIds(new Set())} title="清空选择（Esc）">清空</button>
          <button className="bulk-btn" onClick={() => {
            const ids = [...selectedIds];
            const fav = !conversations.find((c) => c.id === ids[0])?.favorite;
            onBulkFavorite?.(ids, fav);
            showToast(`✓ ${fav ? "已收藏" : "已取消收藏"} ${ids.length} 条`, "info");
          }}>★ 收藏</button>
          <button className="bulk-btn" onClick={() => {
            const ids = [...selectedIds];
            const arch = !conversations.find((c) => c.id === ids[0])?.archived;
            onBulkArchive?.(ids, arch);
            showToast(`✓ ${arch ? "已归档" : "已取消归档"} ${ids.length} 条`, "info");
          }}>🗄 归档</button>
          <input
            className="bulk-tag-input"
            placeholder="# 批量加标签…"
            value={bulkTagInput}
            onChange={(e) => setBulkTagInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && bulkTagInput.trim()) {
                const tag = bulkTagInput.trim().replace(/^#+/, "").trim();
                if (tag) {
                  onBulkAddTag?.([...selectedIds], tag);
                  showToast(`✓ 已加标签 #${tag} 到 ${selectedIds.size} 条`, "info");
                }
                setBulkTagInput("");
              }
            }}
            title="输入标签名后按 Enter（自动去 # 前缀）"
          />
          <button className="bulk-btn" title="把这批会话拆分到一个新 Workspace（plan §4.3 手动拆分）" onClick={() => {
            const name = window.prompt(`把选中的 ${selectedIds.size} 条会话移到新 Workspace，输入名称：`);
            if (!name?.trim()) return;
            onBulkSplit?.([...selectedIds], name.trim());
          }}>📂 拆分到新 Workspace</button>
          <button className="bulk-btn danger" onClick={() => {
            onBulkDelete?.([...selectedIds]);
            setSelectedIds(new Set());
          }}>🗑 删除</button>
        </div>
      )}

      {loading && (
        <div className="panel-loading"><div className="spinner spinner-sm" /><span>加载会话…</span></div>
      )}
      {!loading && sorted.length > 0 && (
        <div
          ref={parentRef}
          className="list-virtual-container"
          style={{ position: "relative", height: rowVirtualizer.getTotalSize() }}
        >
          {rowVirtualizer.getVirtualItems().map((vi) => {
            const c = sorted[vi.index];
            if (!c) return null;
            const flags = itemFlags.get(c.id);
            if (!flags) return null;
            const isExpanded = flags.isExpanded;
            const children = isExpanded ? (childConvs[c.id] ?? []) : [];
            return (
              <div
                key={c.id}
                data-index={vi.index}
                ref={rowVirtualizer.measureElement}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${vi.start}px)`,
                }}
              >
                <ConvItem
                  conv={c}
                  isChild={false}
                  isActive={flags.isActive}
                  isSelected={flags.isSelected}
                  isPinned={flags.isPinned}
                  isExpanded={isExpanded}
                  scope={scope}
                  childCount={c.child_count}
                  onItemClick={(e) => handleItemClick(c, e)}
                  onContextMenu={(e) => handleContextMenu(c, e)}
                  onToggleExpand={(e) => { e.stopPropagation(); onToggleExpand(c); }}
                  onRestore={(e) => { e.stopPropagation(); onRestore?.(c); }}
                />
                {isExpanded && (
                  <div className="child-list">
                    {children.length === 0 && <div className="child-empty">无子任务</div>}
                    {children.map((ch) => (
                      <ConvItem
                        key={ch.id}
                        conv={ch}
                        isChild={true}
                        isActive={selectedConv?.id === ch.id}
                        isSelected={selectedIds.has(ch.id)}
                        isPinned={false}
                        isExpanded={false}
                        scope={scope}
                        childCount={0}
                        onItemClick={(e) => handleItemClick(ch, e)}
                        onContextMenu={() => { /* 子项不响应右键 */ }}
                        onToggleExpand={() => { /* 子项无展开 */ }}
                        onRestore={(e) => { e.stopPropagation(); onRestore?.(ch); }}
                      />
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
      {!loading && conversations.length === 0 && (
        <div className="empty empty-cta">
          <div className="empty-icon">📥</div>
          <div className="empty-title">还没有任何会话</div>
          <div className="empty-hint">点上方「⬇ 导入」按钮把 Cursor / Claude Code / ZCode 里的历史对话同步进来</div>
        </div>
      )}
      {!loading && conversations.length > 0 && dateFiltered.length === 0 && (
        <div className="empty">当前日期范围无会话（试试「全部时间」）</div>
      )}
      {!loading && conversations.length > 0 && dateFiltered.length > 0 && searchFiltered.length === 0 && (
        <div className="empty">无匹配「{listSearch}」的会话（清空搜索试试）</div>
      )}

      {ctxMenu && (
        <ContextMenu
          x={ctxMenu.x}
          y={ctxMenu.y}
          items={buildMenu(ctxMenu.conv, ctxMenu.x, ctxMenu.y)}
          onClose={() => { setCtxMenu(null); setTagInput(null); }}
        />
      )}
      {/* 右键「加标签」触发的内联输入（替代 window.prompt） */}
      {tagInput && (
        <>
          <div className="contextmenu-backdrop" onClick={() => { setTagInput(null); setCtxMenu(null); }} />
          <div
            className="contextmenu"
            style={{ left: tagInput.x, top: tagInput.y + 32, padding: 6 }}
            role="menu"
            onClick={(e) => e.stopPropagation()}
          >
            <input
              className="bulk-tag-input"
              autoFocus
              placeholder="# 标签名（自动去 # 前缀）"
              value={tagInput.value}
              onChange={(e) => setTagInput({ ...tagInput, value: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") { e.preventDefault(); void submitTagInput(); }
                else if (e.key === "Escape") { e.preventDefault(); setTagInput(null); setCtxMenu(null); }
              }}
              title="Enter 提交 · Esc 取消"
            />
          </div>
        </>
      )}
    </>
  );
}
