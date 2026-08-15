// 来源导入面板（从 App.tsx 拆出）

export interface SourceSession {
  session_id: string;
  title: string;
  detail: string;
  message_count: number | null;
  imported: boolean;
}

interface Props {
  panel: string;
  sessions: SourceSession[];
  importing: boolean;
  progress: { done: number; total: number } | null;
  onImport: (id: string) => void;
  onImportAll: () => void;
  onClose: () => void;
  sourceLabel: (p: string) => string;
}

export default function SourcePanel({
  panel, sessions, importing, progress, onImport, onImportAll, onClose, sourceLabel,
}: Props) {
  return (
    <div className="source-overlay">
      <div className="source-panel">
        <div className="source-header">
          <h3>
            {sourceLabel(panel)} 会话
            <span className="source-count">({sessions.length})</span>
          </h3>
          <div className="source-actions">
            <button
              className="source-import-all"
              disabled={importing || sessions.length === 0}
              onClick={onImportAll}
            >
              全部导入
            </button>
            <button className="source-close" onClick={() => !importing && onClose()}>✕</button>
          </div>
        </div>
        {importing && (
          <div className="source-importing">
            {progress ? `批量导入中… ${progress.done}/${progress.total}` : "导入中…"}
          </div>
        )}
        {progress && (
          <div className="batch-progress">
            <div className="batch-progress-bar" style={{ width: `${(progress.done / progress.total) * 100}%` }} />
          </div>
        )}
        <div className="source-list">
          {sessions.map((s) => (
            <div
              key={s.session_id}
              className={`source-item ${s.imported ? "imported" : ""}`}
              onClick={() => !importing && !s.imported && onImport(s.session_id)}
            >
              <div className="source-title">
                {s.title || "(无标题)"}
                {s.imported && <span className="imported-badge">✓ 已导入</span>}
              </div>
              <div className="source-meta">
                {s.message_count != null && `${s.message_count} 消息 · `}
                {s.detail}
              </div>
            </div>
          ))}
          {sessions.length === 0 && <div className="source-empty">加载中或无数据…</div>}
        </div>
      </div>
    </div>
  );
}
