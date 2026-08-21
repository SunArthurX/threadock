// 导入菜单：单 IDE 导入已下线（统一走「增量同步」一次拉全部），
// 保留两条入口：增量同步 + 从文件导入。新内容红点由 ImportMenu 顶钮 total 触发。
import { Icon } from "./Icon";
type SourceType = "file";

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
  onSelect: (source: SourceType) => void;
  /** 增量同步全部来源（与自动同步同一命令，手动触发）。 */
  onSync: () => void;
  syncing?: boolean;
  /** 未导入新内容计数：> 0 时触发按钮显示红点。 */
  newCount?: NewCount | null;
}

export default function ImportMenu({ open, onToggle, onSelect, onSync, syncing, newCount }: Props) {
  const total = newCount?.total ?? 0;
  return (
    <div className="import-dropdown">
      <button className="import-trigger" onClick={onToggle}>
        <Icon name="sync" size={12} /> 同步
        {total > 0 && <span className="new-dot" title={`${total} 条待同步`} />}
        <Icon name="chevron-down" size={11} className="import-caret" />
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
              <span className="import-item-icon"><Icon name="sync" size={14} /></span>
              <span className="import-item-main">
                {syncing ? "同步中…" : "立即同步全部"}
                <span className="import-item-sub">全部来源 · 已导入且无更新的自动跳过</span>
              </span>
              {total > 0 && <span className="import-item-count">{total}</span>}
            </button>
            <div className="import-menu-sep" />
            <button onClick={() => onSelect("file")}>
              <span className="import-item-icon"><Icon name="file" size={14} /></span>
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
