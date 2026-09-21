// 搜索结果面板（按主对话分组）：左栏搜索模式专用。
// 命中聚合到「主对话」层级，子对话命中折叠在所属主对话之下（缩进行），
// 保持与普通会话列表一致的父子树心智模型；点击任一行进入右栏命中步进。
// 右键菜单与普通列表（ConversationList）共用同一套构建逻辑（ConvMenu）：
// 收藏 / 归档 / 置顶 / 加标签 / 复制标题 / 删除 完全一致。
import { useMemo, useState } from "react";
import { t } from "./i18n";
import { Conversation, SearchHitGroup, sourceLabel } from "./types";
import { showToast } from "./toast";
import ContextMenu from "./ContextMenu";
import { buildConvMenuItems, TagInputPopup, loadPinnedIds, togglePinnedId } from "./ConvMenu";

interface Props {
  groups: SearchHitGroup[];
  query: string;
  /** 角色筛选（"" 全部 / "user" / "assistant"），变更触发后端重查。 */
  role: string;
  onRoleChange: (role: string) => void;
  /** 点击某个会话行（主对话或子对话）：打开对应会话并进入命中步进。 */
  onOpen: (g: SearchHitGroup) => void;
  /** 当前打开的会话（高亮对应行）。 */
  activeConversationId?: string | null;
  /** 当前已加载的会话全集（右键按 conversation_id 查对象与收藏/归档状态）。 */
  conversations?: Conversation[];
  /** 菜单动作完成后回调（宿主刷新搜索结果与会话列表）。 */
  onAfterAction?: () => void;
  // ── 与 ConversationList 同名同义的治理 handler（右键菜单共用）──
  onToggleFavorite?: (c: Conversation) => void;
  onArchiveOne?: (c: Conversation) => void;
  onDeleteOne?: (c: Conversation) => void;
  onCopyTitle?: (c: Conversation) => void;
  onBulkFavorite?: (ids: string[], favorite: boolean) => Promise<void> | void;
  onBulkArchive?: (ids: string[], archived: boolean) => Promise<void> | void;
  onBulkDelete?: (ids: string[]) => Promise<void> | void;
  onBulkAddTag?: (ids: string[], tag: string) => Promise<void> | void;
}

/** 一个主对话分组：root 信息 + 其下命中的会话行（主对话自身在前，子对话在后）。 */
interface RootSection {
  rootId: string;
  rootTitle: string;
  provider: string;
  rows: SearchHitGroup[];
  totalHits: number;
}

export default function SearchResultsPanel({
  groups, query, role, onRoleChange, onOpen, activeConversationId,
  conversations = [], onAfterAction,
  onToggleFavorite, onArchiveOne, onDeleteOne, onCopyTitle,
  onBulkFavorite, onBulkArchive, onBulkDelete, onBulkAddTag,
}: Props) {
  // 右键菜单：命中行对应的会话对象 + 屏幕坐标（与 ConversationList 同款交互）
  const [ctxMenu, setCtxMenu] = useState<{ conv: Conversation; x: number; y: number } | null>(null);
  // 右键菜单触发的「加标签」内联输入：避免原生 window.prompt 阻断流程
  const [tagInput, setTagInput] = useState<{ ids: string[]; count: number; value: string; x: number; y: number } | null>(null);
  // 置顶：与普通列表共用同一 localStorage 键（两个左栏互斥渲染，重挂载时重新读取）
  const [pinned, setPinned] = useState<Set<string>>(loadPinnedIds);

  // 按 root 聚合；组间顺序 = 首个命中出现顺序（引擎相关序），组内主对话自身在前
  const sections = useMemo<RootSection[]>(() => {
    const out: RootSection[] = [];
    const index = new Map<string, RootSection>();
    for (const g of groups) {
      let sec = index.get(g.root_conversation_id);
      if (!sec) {
        sec = {
          rootId: g.root_conversation_id,
          rootTitle: g.root_title ?? t("(无标题)"),
          provider: g.provider,
          rows: [],
          totalHits: 0,
        };
        index.set(g.root_conversation_id, sec);
        out.push(sec);
      }
      sec.rows.push(g);
      sec.totalHits += g.hit_count;
    }
    for (const sec of out) {
      sec.rows.sort((a, b) => Number(a.is_child) - Number(b.is_child));
    }
    return out;
  }, [groups]);

  const totalConvs = groups.length;

  // 右键命中行（主/子对话行、分组 root 头）：按 id 找会话对象后弹同一套菜单
  const handleContextMenu = (conversationId: string, e: React.MouseEvent) => {
    const conv = conversations.find((c) => c.id === conversationId);
    if (!conv) return; // 会话不在当前列表数据里（状态未刷新等）：无对象可依赖，不弹菜单
    e.preventDefault();
    setCtxMenu({ conv, x: e.clientX, y: e.clientY });
  };

  // 提交右键菜单触发的「加标签」内联输入（与普通列表同款流程）
  const submitTagInput = async () => {
    if (!tagInput) return;
    const tag = tagInput.value.trim().replace(/^#+/, "").trim();
    setTagInput(null);
    setCtxMenu(null);
    if (!tag) return;
    if (onBulkAddTag) {
      await onBulkAddTag(tagInput.ids, tag);
      showToast(t("✓ 已加标签 #{__0__} 到 {__1__} 条", { __0__: tag, __1__: tagInput.count }), "info");
    }
    onAfterAction?.();
  };

  return (
    <>
      <div className="panel-header">
        <div className="search-panel-head">
          <span>命中 {totalConvs} 个会话 · 关键词「{query}」</span>
          <div className="search-panel-actions">
            <select
              className="search-panel-select"
              value={role}
              onChange={(e) => onRoleChange(e.target.value)}
              title={t("按角色筛选（重新查询）")}
            >
              <option value="">{t("全部角色")}</option>
              <option value="user">{t("仅用户")}</option>
              <option value="assistant">{t("仅助手")}</option>
            </select>
          </div>
        </div>
      </div>
      {sections.length === 0 && <div className="empty">{t("无匹配")}</div>}
      {sections.map((sec) => (
        <div key={sec.rootId} className="search-group">
          <div
            className="search-group-root"
            onClick={() => onOpen(sec.rows[0])}
            onContextMenu={(e) => handleContextMenu(sec.rootId, e)}
            title={t("打开该主对话（含子对话命中步进）")}
          >
            <span className="search-group-caret">▾</span>
            <div className="title">
              {sec.rootTitle}
              <span className={`badge source ${sec.provider}`}>{sourceLabel(sec.provider)}</span>
              <span className="search-hit-total" title={t("该主对话（含子对话）总命中数")}>
                🎯 {sec.totalHits} 处
              </span>
            </div>
          </div>
          {sec.rows.map((r) => (
            <div
              key={r.conversation_id}
              className={`search-result search-group-row ${activeConversationId === r.conversation_id ? "active" : ""}`}
              onClick={() => onOpen(r)}
              onContextMenu={(e) => handleContextMenu(r.conversation_id, e)}
              title={t("打开此会话并跳到命中")}
            >
              <div className="title">
                <span className={`search-row-kind ${r.is_child ? "child" : "parent"}`}>
                  {r.is_child ? t("子对话") : t("主对话")}
                </span>
                {r.title ?? sec.rootTitle}
                <span className="search-role">{r.best_role}</span>
                <span className="search-hit-count">{r.hit_count} 处</span>
              </div>
              <div className="snippet" dangerouslySetInnerHTML={{ __html: r.snippet }} />
            </div>
          ))}
        </div>
      ))}

      {ctxMenu && (
        <ContextMenu
          x={ctxMenu.x}
          y={ctxMenu.y}
          items={buildConvMenuItems({
            conv: ctxMenu.conv,
            x: ctxMenu.x,
            y: ctxMenu.y,
            // 搜索命中均为活跃会话：与普通列表「全部会话」视图同款菜单
            scope: "all",
            pinned,
            onTogglePin: (id) => setPinned(togglePinnedId(id)),
            selectedIds: new Set(),
            conversations,
            onToggleFavorite,
            onArchiveOne,
            onDeleteOne,
            onCopyTitle,
            onBulkFavorite: async (ids, favorite) => {
              await onBulkFavorite?.(ids, favorite);
              onAfterAction?.();
            },
            onBulkArchive: async (ids, archived) => {
              await onBulkArchive?.(ids, archived);
              onAfterAction?.();
            },
            onBulkDelete: async (ids) => {
              await onBulkDelete?.(ids);
              onAfterAction?.();
            },
            onBulkAddTag,
            openTagInput: (ids, count, x, y) => setTagInput({ ids, count, value: "", x, y }),
          })}
          // 只关菜单：菜单项触发的「加标签」内联输入由 TagInputPopup 自身生命周期管理
          onClose={() => setCtxMenu(null)}
        />
      )}
      {/* 右键「加标签」触发的内联输入（替代 window.prompt） */}
      {tagInput && (
        <TagInputPopup
          x={tagInput.x}
          y={tagInput.y}
          value={tagInput.value}
          onChange={(v) => setTagInput({ ...tagInput, value: v })}
          onSubmit={() => void submitTagInput()}
          onClose={() => { setTagInput(null); setCtxMenu(null); }}
        />
      )}
    </>
  );
}
