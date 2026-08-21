// 底部状态栏：当前页 + 同步状态 + 实时时间 + 快捷键提示。
// 独立组件：自管 1s 时间刷新，避免整个 App 树每秒重渲染（P1-D3）。
import { useEffect, useState } from "react";
import { Icon } from "./Icon";

export interface StatusBarProps {
  syncResult: string | null;
  syncing: boolean;
  /** 视图标签（来自 App 的 VIEW_LABEL）。 */
  viewLabel: string;
}

export default function StatusBar({ syncResult, syncing, viewLabel }: StatusBarProps) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  const mod = isMac ? "⌘" : "Ctrl";
  const time = new Date(nowMs).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
  return (
    <div className="status-bar">
      <span className="status-cell status-view">
        <span className="status-dot" />
        {viewLabel}
      </span>
      <span className={`status-cell status-sync ${syncing ? "syncing" : syncResult ? "done" : ""}`}>
        {syncing ? (
          <><Icon name="sync" size={11} /> 同步中…</>
        ) : syncResult ? (
          <><Icon name="check" size={11} /> {syncResult.replace(/^✓\s*/, "")}</>
        ) : (
          <><Icon name="circle-dot" size={11} /> 待同步</>
        )}
      </span>
      <span className="status-cell status-spacer" />
      <span className="status-cell status-hint">
        <kbd>{mod}K</kbd> 命令 · <kbd>{mod}?</kbd> 速查 · <kbd>{mod}F</kbd> 搜索 · <kbd>{mod}R</kbd> 刷新
      </span>
      <span className="status-cell status-time">{time}</span>
    </div>
  );
}
