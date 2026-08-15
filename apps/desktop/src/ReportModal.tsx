// 报告中心弹窗：应用内渲染周报（当前周期实时生成）+ 历史报告列表
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTime } from "./types";

interface ReportFile { name: string; size: number; mtime_ms: number }

export default function ReportModal({ onClose }: { onClose: () => void }) {
  const [html, setHtml] = useState<string | null>(null);
  const [history, setHistory] = useState<ReportFile[]>([]);
  const [current, setCurrent] = useState<string | null>(null); // 当前查看的报告名（null=实时周报）
  const [loading, setLoading] = useState(false);

  const loadHistory = async () => {
    try { setHistory(await invoke<ReportFile[]>("list_reports", {})); } catch { /* 静默 */ }
  };
  const renderCurrent = async () => {
    setLoading(true);
    try {
      setHtml(await invoke<string>("ops_weekly_report", {}));
      setCurrent(null);
    } catch { /* 静默 */ }
    setLoading(false);
  };
  const openHistory = async (name: string) => {
    setLoading(true);
    try {
      setHtml(await invoke<string>("read_report", { name }));
      setCurrent(name);
    } catch { /* 静默 */ }
    setLoading(false);
  };

  useEffect(() => {
    renderCurrent();
    loadHistory();
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings-modal report-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>📊 报告中心</h2>
          <div className="knowledge-modal-actions">
            <button className="action-btn" onClick={renderCurrent} disabled={loading}>↻ 当前周报</button>
            <button className="settings-close" onClick={onClose}>✕</button>
          </div>
        </div>
        <div className="settings-body">
          {history.length > 0 && (
            <div className="report-history">
              <span className="automation-sub">历史报告（{history.length}）</span>
              {history.map((h) => (
                <button key={h.name} className={`filter-chip ${current === h.name ? "active" : ""}`}
                  onClick={() => openHistory(h.name)} title={`${h.name} · ${(h.size / 1024).toFixed(0)} KB`}>
                  {h.name.replace("weekly-", "").replace(".html", "")} · {formatTime(h.mtime_ms)}
                </button>
              ))}
            </div>
          )}
          {loading && <div className="sk-line" style={{ margin: 12 }} />}
          {html && (
            <iframe
              className="report-frame"
              sandbox="allow-same-origin"
              srcDoc={html}
              title="周报"
            />
          )}
        </div>
      </div>
    </div>
  );
}
