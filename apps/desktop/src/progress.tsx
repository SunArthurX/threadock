// 共享迷你进度 shim（监听 sync_progress 事件的轻量组件，多页复用）
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

interface P { current: number; total: number; detail: string; finished: boolean }

/** 监听 sync_progress 显示一行迷你进度（label 匹配 detail 前缀时才显示）。 */
export function MiniProgressShim({ show, label }: { show: boolean; label: string }) {
  const [p, setP] = useState<P | null>(null);
  useEffect(() => {
    const un = listen<P>("sync_progress", (e) => {
      setP(e.payload);
      if (e.payload.finished) window.setTimeout(() => setP(null), 1500);
    });
    return () => { un.then((f) => f()); };
  }, []);
  if (!show || !p || p.total === 0 || p.detail === "done" || !p.detail.startsWith(label)) return null;
  return (
    <div className="mini-progress" style={{ margin: "6px 0" }}>
      <span className="mini-progress-fill" style={{ width: `${Math.min(100, (p.current / p.total) * 100)}%` }} />
      <span className="mini-progress-label">{p.detail} {p.current}/{p.total}</span>
    </div>
  );
}
