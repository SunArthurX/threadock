// 导入来源下拉菜单组件
type SourceType = "zcode" | "claude-code" | "cursor" | "minimax" | "codex";

interface Props {
  open: boolean;
  onToggle: () => void;
  onSelect: (source: SourceType | "file") => void;
}

const SOURCES: [SourceType, string, string][] = [
  ["zcode", "zcode", "ZCode"],
  ["claude-code", "claude-code", "Claude Code"],
  ["cursor", "cursor", "Cursor"],
  ["minimax", "minimax-code", "MiniMax"],
  ["codex", "codex", "Codex"],
];

export default function ImportMenu({ open, onToggle, onSelect }: Props) {
  return (
    <div className="import-dropdown">
      <button className="import-trigger" onClick={onToggle}>📥 导入 ▾</button>
      {open && (
        <>
          <div className="import-backdrop" onClick={onToggle} />
          <div className="import-menu">
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
