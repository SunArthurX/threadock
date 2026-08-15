// 报告中心弹窗：应用内渲染周报（当前周期实时生成）+ 历史报告列表
// 增强：搜索过滤 + 收藏（localStorage 持久化）
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTime } from "./types";

interface ReportFile { name: string; size: number; mtime_ms: number }

const FAV_KEY = "ch-report-favs";
/** 读取收藏的报告名集合。 */
export function loadReportFavs(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(FAV_KEY) ?? "[]") as string[]); }
  catch { return new Set(); }
}
function saveReportFavs(s: Set<string>) {
  try { localStorage.setItem(FAV_KEY, JSON.stringify([...s])); } catch { /* 静默 */ }
}

export default function ReportModal({ onClose }: { onClose: () => void }) {
  const [html, setHtml] = useState<string | null>(null);
  const [history, setHistory] = useState<ReportFile[]>([]);
  const [current, setCurrent] = useState<string | null>(null); // 当前查看的报告名（null=实时周报）
  const [loading, setLoading] = useState(false);
  const [keyword, setKeyword] = useState("");
  const [favs, setFavs] = useState<Set<string>>(loadReportFavs);
  const [showFavOnly, setShowFavOnly] = useState(false);

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
  const toggleFav = (name: string) => {
    setFavs((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      saveReportFavs(next);
      return next;
    });
  };

  // 搜索 + 收藏过滤
  const filteredHistory = useMemo(() => {
    let arr = history;
    if (showFavOnly) arr = arr.filter((h) => favs.has(h.name));
    if (keyword.trim()) {
      const kw = keyword.trim().toLowerCase();
      arr = arr.filter((h) => h.name.toLowerCase().includes(kw));
    }
    return arr;
  }, [history, keyword, showFavOnly, favs]);

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
              <div className="report-history-header">
                <span className="automation-sub">
                  历史报告（{filteredHistory.length}{filteredHistory.length !== history.length ? ` / ${history.length}` : ""}）
                </span>
                <input
                  className="report-search"
                  type="search"
                  placeholder="🔍 搜索报告名…"
                  value={keyword}
                  onChange={(e) => setKeyword(e.target.value)}
                />
                <button
                  className={`filter-chip ${showFavOnly ? "active" : ""}`}
                  onClick={() => setShowFavOnly((v) => !v)}
                  title="只看收藏"
                >
                  {showFavOnly ? "★ 仅收藏" : "☆ 收藏"}
                </button>
              </div>
              {filteredHistory.length === 0 ? (
                <div className="ops-table-empty" style={{ padding: 8 }}>
                  {keyword ? `没有匹配「${keyword}」的报告` : "暂无收藏"}
                </div>
              ) : (
                <div className="report-history-list">
                  {filteredHistory.map((h) => (
                    <div key={h.name} className={`report-history-item ${current === h.name ? "active" : ""}`}>
                      <button
                        className="report-fav-btn"
                        onClick={() => toggleFav(h.name)}
                        title={favs.has(h.name) ? "取消收藏" : "收藏此报告"}
                      >
                        {favs.has(h.name) ? "★" : "☆"}
                      </button>
                      <button
                        className="report-open-btn"
                        onClick={() => openHistory(h.name)}
                        title={`${h.name} · ${(h.size / 1024).toFixed(0)} KB`}
                      >
                        <span className="mono">{h.name.replace("weekly-", "").replace(".html", "")}</span>
                        <span className="report-time">{formatTime(h.mtime_ms)}</span>
                      </button>
                    </div>
                  ))}
                </div>
              )}
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
