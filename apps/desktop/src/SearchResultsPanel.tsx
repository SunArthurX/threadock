// 搜索结果面板（按主对话分组）：左栏搜索模式专用。
// 命中聚合到「主对话」层级，子对话命中折叠在所属主对话之下（缩进行），
// 保持与普通会话列表一致的父子树心智模型；点击任一行进入右栏命中步进。
import { useMemo } from "react";
import { SearchHitGroup, sourceLabel } from "./types";

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
}: Props) {
  // 按 root 聚合；组间顺序 = 首个命中出现顺序（引擎相关序），组内主对话自身在前
  const sections = useMemo<RootSection[]>(() => {
    const out: RootSection[] = [];
    const index = new Map<string, RootSection>();
    for (const g of groups) {
      let sec = index.get(g.root_conversation_id);
      if (!sec) {
        sec = {
          rootId: g.root_conversation_id,
          rootTitle: g.root_title ?? "(无标题)",
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
              title="按角色筛选（重新查询）"
            >
              <option value="">全部角色</option>
              <option value="user">仅用户</option>
              <option value="assistant">仅助手</option>
            </select>
          </div>
        </div>
      </div>
      {sections.length === 0 && <div className="empty">无匹配</div>}
      {sections.map((sec) => (
        <div key={sec.rootId} className="search-group">
          <div
            className="search-group-root"
            onClick={() => onOpen(sec.rows[0])}
            title="打开该主对话（含子对话命中步进）"
          >
            <span className="search-group-caret">▾</span>
            <div className="title">
              {sec.rootTitle}
              <span className={`badge source ${sec.provider}`}>{sourceLabel(sec.provider)}</span>
              <span className="search-hit-total" title="该主对话（含子对话）总命中数">
                🎯 {sec.totalHits} 处
              </span>
            </div>
          </div>
          {sec.rows.map((r) => (
            <div
              key={r.conversation_id}
              className={`search-result search-group-row ${activeConversationId === r.conversation_id ? "active" : ""}`}
              onClick={() => onOpen(r)}
              title="打开此会话并跳到命中"
            >
              <div className="title">
                <span className={`search-row-kind ${r.is_child ? "child" : "parent"}`}>
                  {r.is_child ? "子对话" : "主对话"}
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
    </>
  );
}
