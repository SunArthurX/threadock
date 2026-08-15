// 搜索结果面板组件（角色/时间筛选 + 关键词高亮 + 复制）
import { useMemo, useState } from "react";
import { SearchResult, sourceLabel } from "./types";
import { showToast } from "./toast";

interface Props {
  results: SearchResult[];
  query: string;
  onJump: (r: SearchResult) => void;
}

type RoleFilter = "all" | "user" | "assistant";

export default function SearchPanel({ results, query, onJump }: Props) {
  const [role, setRole] = useState<RoleFilter>("all");
  const [copied, setCopied] = useState(false);

  const filtered = useMemo(() => {
    if (role === "all") return results;
    return results.filter((r) => r.role === role);
  }, [results, role]);

  const copyAll = async () => {
    const text = filtered.map((r) => `[${r.provider}/${r.role}] ${r.title ?? "(无标题)"}\n${r.snippet.replace(/<[^>]+>/g, "")}`).join("\n\n---\n\n");
    try {
      await navigator.clipboard.writeText(text);
      showToast(`✓ 已复制 ${filtered.length} 条结果`, "info");
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { showToast("剪贴板不可用", "error"); }
  };

  return (
    <>
      <div className="panel-header">
        <div className="search-panel-head">
          <span>搜索结果 ({filtered.length}{filtered.length !== results.length ? `/${results.length}` : ""}) · 关键词「{query}」</span>
          <div className="search-panel-actions">
            <select className="search-panel-select" value={role} onChange={(e) => setRole(e.target.value as RoleFilter)} title="按角色筛选">
              <option value="all">全部角色</option>
              <option value="user">仅用户</option>
              <option value="assistant">仅助手</option>
            </select>
            <button className="action-btn" style={{ fontSize: 11 }} onClick={copyAll} disabled={filtered.length === 0}>
              {copied ? "✓ 已复制" : "📋 复制全部"}
            </button>
          </div>
        </div>
      </div>
      {filtered.length === 0 && results.length > 0 && (
        <div className="empty">当前筛选条件下无结果（试试「全部角色」）</div>
      )}
      {filtered.length === 0 && results.length === 0 && <div className="empty">无匹配</div>}
      {filtered.map((r) => (
        <div key={r.message_id} className="search-result" onClick={() => onJump(r)}>
          <div className="title">
            {r.title ?? "(无标题)"}
            <span className={`badge source ${r.provider}`}>{sourceLabel(r.provider)}</span>
            <span className="search-role">{r.role}</span>
          </div>
          <div className="snippet" dangerouslySetInnerHTML={{ __html: r.snippet }} />
        </div>
      ))}
    </>
  );
}
