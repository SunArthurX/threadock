// 搜索结果面板组件
import { SearchResult, sourceLabel } from "./types";

interface Props {
  results: SearchResult[];
  query: string;
  onJump: (r: SearchResult) => void;
}

export default function SearchPanel({ results, query, onJump }: Props) {
  return (
    <>
      <div className="panel-header">搜索结果 ({results.length}) · 关键词「{query}」</div>
      {results.map((r) => (
        <div key={r.message_id} className="search-result" onClick={() => onJump(r)}>
          <div className="title">
            {r.title ?? "(无标题)"}
            <span className={`badge source ${r.provider}`}>{sourceLabel(r.provider)}</span>
            <span className="search-role">{r.role}</span>
          </div>
          <div className="snippet" dangerouslySetInnerHTML={{ __html: r.snippet }} />
        </div>
      ))}
      {results.length === 0 && <div className="empty">无匹配</div>}
    </>
  );
}
