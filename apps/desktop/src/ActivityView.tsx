// 活动节律页：按天热力图 + 24 小时分布 + 工具月度趋势
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BarChart } from "./charts";

interface Stats {
  heatmap: { day: string; calls: number; sessions: number }[];
  hourly: { hour: number; calls: number }[];
  tools_trend: { month: string; tool: string; calls: number }[];
}

/** 热力图单元格颜色（5 档）。 */
export function heatColor(calls: number, max: number): string {
  if (calls === 0) return "var(--border, #2a2e3a)";
  const r = max > 0 ? calls / max : 0;
  if (r > 0.75) return "#1d4ed8";
  if (r > 0.5) return "#3b82f6";
  if (r > 0.25) return "#60a5fa";
  return "#93c5fd";
}

/** 生成 GitHub 风格热力图的列（周）×行（周内天）布局数据。 */
export function buildHeatGrid(cells: { day: string; calls: number }[]): { cols: ({ day: string; calls: number } | null)[][]; max: number } {
  if (cells.length === 0) return { cols: [], max: 0 };
  const byDay = new Map(cells.map((c) => [c.day, c]));
  const first = new Date(cells[0].day + "T00:00:00");
  const last = new Date(cells[cells.length - 1].day + "T00:00:00");
  const max = Math.max(...cells.map((c) => c.calls), 1);
  const cols: ({ day: string; calls: number } | null)[][] = [];
  let cur: ({ day: string; calls: number } | null)[] = new Array(first.getDay()).fill(null);
  const d = new Date(first);
  while (d <= last) {
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    const c = byDay.get(key);
    cur.push(c ? { day: key, calls: c.calls } : { day: key, calls: 0 });
    if (cur.length === 7) {
      cols.push(cur);
      cur = [];
    }
    d.setDate(d.getDate() + 1);
  }
  if (cur.length > 0) {
    while (cur.length < 7) cur.push(null);
    cols.push(cur);
  }
  return { cols, max };
}

export default function ActivityView() {
  const [stats, setStats] = useState<Stats | null>(null);

  useEffect(() => {
    (async () => {
      try { setStats(await invoke<Stats>("activity_stats", { days: 365 })); }
      catch { /* 空库静默 */ }
    })();
  }, []);

  const totalCalls = (stats?.heatmap ?? []).reduce((a, b) => a + b.calls, 0);
  const totalDays = (stats?.heatmap ?? []).filter((c) => c.calls > 0).length;
  const grid = buildHeatGrid(stats?.heatmap ?? []);

  // 工具趋势：每月取 Top 5 工具
  const trend = (() => {
    if (!stats) return [];
    const byMonth = new Map<string, { tool: string; calls: number }[]>();
    for (const t of stats.tools_trend) {
      const arr = byMonth.get(t.month) ?? [];
      arr.push({ tool: t.tool, calls: t.calls });
      byMonth.set(t.month, arr);
    }
    const out: { label: string; value: number; tool?: string }[] = [];
    for (const [month, tools] of [...byMonth.entries()].sort()) {
      const top = tools.sort((a, b) => b.calls - a.calls).slice(0, 5);
      for (const t of top) out.push({ label: month.slice(2) + " " + t.tool, value: t.calls, tool: t.tool });
    }
    return out.slice(-40);
  })();

  return (
    <div className="activity-page">
      <div className="ops-card">
        <div className="ops-card-title">
          📆 活动节律
          <span className="ops-card-sub">
            {stats ? `近一年 ${totalCalls.toLocaleString()} 次工具调用 · ${totalDays} 个活跃日` : "加载中…"}
          </span>
        </div>
        {grid.cols.length === 0 ? (
          <div className="ops-table-empty">暂无工具调用数据</div>
        ) : (
          <div className="heatmap-wrap">
            <div className="heatmap">
              {grid.cols.map((col, ci) => (
                <div key={ci} className="heatmap-col">
                  {col.map((cell, ri) => (
                    <div
                      key={ri}
                      className="heat-cell"
                      style={{ background: cell ? heatColor(cell.calls, grid.max) : "transparent" }}
                      title={cell ? `${cell.day} · ${cell.calls} 次调用` : ""}
                    />
                  ))}
                </div>
              ))}
            </div>
            <div className="heat-legend">
              少
              {[0, 0.25, 0.5, 0.75, 1].map((r) => (
                <span key={r} className="heat-cell" style={{ background: heatColor(r * grid.max, grid.max) }} />
              ))}
              多
            </div>
          </div>
        )}
      </div>

      <div className="ops-card">
        <div className="ops-card-title">⏰ 24 小时分布（什么时段和 AI 协作最多）</div>
        {(stats?.hourly ?? []).length === 0 ? <div className="ops-table-empty">暂无数据</div> : (
          <BarChart
            data={(stats?.hourly ?? []).map((h) => ({ label: `${h.hour}`, value: h.calls }))}
            height={120}
          />
        )}
      </div>

      <div className="ops-card">
        <div className="ops-card-title">🔧 工具月度趋势（每月 Top 5）</div>
        {trend.length === 0 ? <div className="ops-table-empty">暂无数据</div> : (
          <BarChart data={trend.map((t) => ({ label: t.label, value: t.value }))} height={140} />
        )}
      </div>
    </div>
  );
}
