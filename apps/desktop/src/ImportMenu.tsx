// 导入来源下拉菜单组件（含增量同步入口）
type SourceType = "zcode" | "claude-code" | "cursor" | "minimax" | "codex";

interface Props {
  open: boolean;
  onToggle: () => void;
  onSelect: (source: SourceType | "file") => void;
  /** 增量同步全部来源（与自动同步同一命令，手动触发）。 */
  onSync: () => void;
  syncing?: boolean;
}

const SOURCES: [SourceType, string, string][] = [
  ["zcode", "zcode", "ZCode"],
  ["claude-code", "claude-code", "Claude Code"],
  ["cursor", "cursor", "Cursor"],
  ["minimax", "minimax-code", "MiniMax"],
  ["codex", "codex", "Codex"],
];

export default function ImportMenu({ open, onToggle, onSelect, onSync, syncing }: Props) {
  return (
    <div className="import-dropdown">
      <button className="import-trigger" onClick={onToggle}>📥 导入 ▾</button>
      {open && (
        <>
          <div className="import-backdrop" onClick={onToggle} />
          <div className="import-menu">
            <button
              onClick={() => { onToggle(); onSync(); }}
              disabled={syncing}
              title="拉取 5 个来源的最新会话（已导入且无更新的自动跳过）"
            >
              {syncing ? "⟳ 同步中…" : "⇩ 增量同步（全部来源）"}
            </button>
            <button onClick={() => onSelect("file")}>📄 从文件导入（Markdown/JSONL）</button>
            <div className="import-menu-sep" />
            {SOURCES.map(([key, badge, label]) => (
              <button key={key} onClick={() => onSelect(key)}>
                <span className={`badge source ${badge}`}>{label}</span> 从 {label} 导入
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
