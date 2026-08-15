// 导入来源下拉菜单组件（增量同步 + 5 来源 + 文件导入置底，带新内容红点）
type SourceType = "zcode" | "claude-code" | "cursor" | "minimax" | "codex";

/** 各来源未导入新内容计数（sources_new_count 返回；total 为总数）。 */
export interface NewCount {
  zcode?: number;
  claude_code?: number;
  cursor?: number;
  minimax?: number;
  codex?: number;
  total?: number;
}

interface Props {
  open: boolean;
  onToggle: () => void;
  onSelect: (source: SourceType | "file") => void;
  /** 增量同步全部来源（与自动同步同一命令，手动触发）。 */
  onSync: () => void;
  syncing?: boolean;
  /** 未导入新内容计数：> 0 时触发按钮显示红点。 */
  newCount?: NewCount | null;
}

const SOURCES: [SourceType, string, string, keyof NewCount][] = [
  ["zcode", "zcode", "ZCode", "zcode"],
  ["claude-code", "claude-code", "Claude Code", "claude_code"],
  ["cursor", "cursor", "Cursor", "cursor"],
  ["minimax", "minimax-code", "MiniMax", "minimax"],
  ["codex", "codex", "Codex", "codex"],
];

export default function ImportMenu({ open, onToggle, onSelect, onSync, syncing, newCount }: Props) {
  const total = newCount?.total ?? 0;
  return (
    <div className="import-dropdown">
      <button className="import-trigger" onClick={onToggle}>
        📥 导入 {total > 0 && <span className="new-dot" title={`${total} 条未导入`} />}
        <span className="import-caret">▾</span>
      </button>
      {open && (
        <>
          <div className="import-backdrop" onClick={onToggle} />
          <div className="import-menu">
            <button
              className="import-sync-item"
              onClick={() => { onToggle(); onSync(); }}
              disabled={syncing}
              title="拉取全部来源的最新会话（已导入且无更新的自动跳过）"
            >
              <span className="import-item-icon">⇩</span>
              <span className="import-item-main">
                {syncing ? "⟳ 同步中…" : "增量同步"}
                <span className="import-item-sub">全部来源 · 已导入且无更新的自动跳过</span>
              </span>
            </button>
            <div className="import-menu-sep" />
            {SOURCES.map(([key, badge, label, countKey]) => {
              const n = newCount?.[countKey] ?? 0;
              return (
                <button key={key} onClick={() => onSelect(key)}>
                  <span className={`badge source ${badge}`}>{label}</span>
                  <span className="import-item-main">
                    从 {label} 导入
                    <span className="import-item-sub">
                      {n > 0 ? `${n} 条未导入` : "已全部导入"}
                    </span>
                  </span>
                  {n > 0 && <span className="new-dot" />}
                </button>
              );
            })}
            <div className="import-menu-sep" />
            <button onClick={() => onSelect("file")}>
              <span className="import-item-icon">📄</span>
              <span className="import-item-main">
                从文件导入
                <span className="import-item-sub">Markdown / JSONL</span>
              </span>
            </button>
          </div>
        </>
      )}
    </div>
  );
}
