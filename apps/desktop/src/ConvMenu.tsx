// 会话右键菜单共享模块：普通左栏（ConversationList）与搜索结果左栏
// （SearchResultsPanel）共用同一套菜单项构建 / 「加标签」内联输入 /
// 置顶持久化，保证两个列表的右键行为完全一致（v1.4.1）。
import { t } from "./i18n";
import { showToast } from "./toast";
import type { MenuItem } from "./ContextMenu";
import type { Conversation } from "./types";

/** 列表视图维度：全部 / 收藏 / 已归档 / 已删除。 */
export type ListScope = "all" | "favorite" | "archived" | "deleted";

const PIN_KEY = "ch-conv-pins";

/** 读取置顶 ID 集合（localStorage 持久化）。 */
export function loadPinnedIds(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(PIN_KEY) ?? "[]") as string[]); }
  catch { return new Set(); }
}

function savePinnedIds(s: Set<string>) {
  try { localStorage.setItem(PIN_KEY, JSON.stringify([...s])); } catch { /* 静默 */ }
}

/** 切换置顶并持久化，返回新集合（无本地状态的调用方——如搜索面板——直接用返回值 setState）。 */
export function togglePinnedId(id: string): Set<string> {
  const n = loadPinnedIds();
  if (n.has(id)) n.delete(id); else n.add(id);
  savePinnedIds(n);
  return n;
}

/** 菜单构建参数：与 ConversationList 的右键菜单完全同源。 */
export interface ConvMenuArgs {
  conv: Conversation;
  /** 菜单弹出坐标（「加标签」内联输入贴其下方）。 */
  x: number;
  y: number;
  /** 视图维度：deleted →「恢复」；其余 → 常规治理项。 */
  scope: ListScope;
  /** 置顶集合（含 conv.id 决定「置顶/取消置顶」文案）。 */
  pinned: Set<string>;
  onTogglePin: (id: string) => void;
  /** 多选集合（含 conv.id 时菜单动作作用于整批；空集/不含 → 单条）。 */
  selectedIds: Set<string>;
  /** 全量会话（deleted 视图「恢复」按 id 找对象用；可为当前列表数据）。 */
  conversations: Conversation[];
  onToggleFavorite?: (c: Conversation) => void;
  onArchiveOne?: (c: Conversation) => void;
  onDeleteOne?: (c: Conversation) => void;
  onCopyTitle?: (c: Conversation) => void;
  onRestore?: (c: Conversation) => void;
  onBulkFavorite?: (ids: string[], favorite: boolean) => Promise<void> | void;
  onBulkArchive?: (ids: string[], archived: boolean) => Promise<void> | void;
  onBulkDelete?: (ids: string[]) => Promise<void> | void;
  onBulkAddTag?: (ids: string[], tag: string) => Promise<void> | void;
  /** 「加标签…」点击：打开内联输入（由宿主列表持有状态）。 */
  openTagInput: (ids: string[], count: number, x: number, y: number) => void;
}

/** 构建会话右键菜单项（收藏 / 归档 / 置顶 / 加标签 / 复制标题 / 删除 或 恢复）。 */
export function buildConvMenuItems(a: ConvMenuArgs): MenuItem[] {
  const { conv: c, conversations, selectedIds } = a;
  const isMulti = selectedIds.size > 1 && selectedIds.has(c.id);
  const targetCount = isMulti ? selectedIds.size : 1;
  const targetIds = isMulti ? [...selectedIds] : [c.id];
  const items: MenuItem[] = [];
  if (a.scope !== "deleted") {
    items.push({
      icon: c.favorite ? "☆" : "★",
      label: isMulti ? `${c.favorite ? t("取消收藏") : t("收藏")} ${targetCount} 条` : (c.favorite ? t("取消收藏") : t("收藏")),
      onClick: async () => {
        const fn = a.onBulkFavorite ? (ids: string[]) => a.onBulkFavorite!(ids, !c.favorite) : undefined;
        if (fn) { await fn(targetIds); showToast(`✓ ${!c.favorite ? t("已收藏") : t("已取消收藏")} ${targetCount} 条`, "info"); }
        else a.onToggleFavorite?.(c);
      },
      group: 1,
    });
    items.push({
      icon: c.archived ? "📤" : "🗄",
      label: isMulti ? `${c.archived ? t("取消归档") : t("归档")} ${targetCount} 条` : (c.archived ? t("取消归档") : t("归档")),
      onClick: async () => {
        if (isMulti) {
          const fn = a.onBulkArchive ? (ids: string[]) => a.onBulkArchive!(ids, !c.archived) : undefined;
          if (fn) { await fn(targetIds); showToast(`✓ ${!c.archived ? t("已归档") : t("已取消归档")} ${targetCount} 条`, "info"); }
        } else if (a.onArchiveOne) a.onArchiveOne(c);
      },
      group: 1,
    });
    items.push({
      icon: a.pinned.has(c.id) ? "📍" : "📌",
      label: a.pinned.has(c.id) ? t("取消置顶") : t("置顶（排在最前）"),
      onClick: () => a.onTogglePin(c.id),
      group: 1,
    });
    items.push({
      icon: "🏷",
      label: isMulti ? t("给 {__0__} 条加标签…", { __0__: targetCount }) : t("加标签…"),
      onClick: () => {
        // 打开内联输入（位置贴 context menu 下方），不在此处用 window.prompt 阻断流程
        a.openTagInput(targetIds, targetCount, a.x, a.y);
      },
      group: 1,
    });
    items.push({
      icon: "📋",
      label: t("复制标题"),
      onClick: () => {
        if (a.onCopyTitle) a.onCopyTitle(c);
        else {
          navigator.clipboard?.writeText(c.user_title ?? c.title ?? "")
            .then(() => showToast("✓ 标题已复制", "info", 1500))
            .catch(() => showToast("剪贴板不可用", "error"));
        }
      },
      group: 2,
    });
    items.push({
      icon: "🗑",
      label: isMulti ? t("删除 {__0__} 条（带撤销）", { __0__: targetCount }) : t("删除（带撤销）"),
      danger: true,
      onClick: () => {
        const fn = a.onBulkDelete ? (ids: string[]) => a.onBulkDelete!(ids) : undefined;
        if (fn) fn(targetIds);
        else if (a.onDeleteOne) a.onDeleteOne(c);
      },
      group: 3,
    });
  } else {
    items.push({
      icon: "↩",
      label: isMulti ? t("恢复这 {__0__} 条会话", { __0__: targetCount }) : t("恢复此会话"),
      onClick: () => {
        for (const id of targetIds) {
          const cc = conversations.find((x) => x.id === id);
          if (cc) a.onRestore?.(cc);
        }
      },
      group: 1,
    });
  }
  return items;
}

/** 右键「加标签」的内联输入弹层（替代 window.prompt）。 */
export function TagInputPopup({
  x, y, value, onChange, onSubmit, onClose,
}: {
  x: number; y: number; value: string;
  onChange: (v: string) => void;
  onSubmit: () => void;
  onClose: () => void;
}) {
  return (
    <>
      <div className="contextmenu-backdrop" onClick={onClose} />
      <div
        className="contextmenu"
        style={{ left: x, top: y + 32, padding: 6 }}
        role="menu"
        onClick={(e) => e.stopPropagation()}
      >
        <input
          className="bulk-tag-input"
          autoFocus
          placeholder={t("# 标签名（自动去 # 前缀）")}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") { e.preventDefault(); onSubmit(); }
            else if (e.key === "Escape") { e.preventDefault(); onClose(); }
          }}
          title={t("Enter 提交 · Esc 取消")}
        />
      </div>
    </>
  );
}
