// 活动节律页（第 6-10 轮优化）：peak 高亮/时间范围说明/空状态引导/范围日期
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BarChart } from "./charts";

interface Stats {
  heatmap: { day: string; calls: number; sessions: number }[];
  hourly: { hour: number; calls: number }[];
  tools_trend: { month: string; tool: string; calls: number }[];
}

/** 把 days 转成可读的「YYYY-MM-DD ~ YYYY-MM-DD」范围文案。 */
export function daysToRange(days: number, now: number = Date.now()): string {
  const end = new Date(now);
  const start = new Date(now - days * 86_400_000);
  const fmt = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  return `${fmt(start)} ~ ${fmt(end)}`;
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

/** 生成 GitHub 风格热力图布局 + 每列首月份标签。 */
export function buildHeatGrid(cells: { day: string; calls: number; sessions?: number }[]): {
  cols: ({ day: string; calls: number; sessions: number } | null)[][];
  labels: { col: number; label: string }[];
  max: number;
} {
  // 纯算术日期序（不依赖 Date 解析：WKWebView 对异常日期串解析为 Invalid
  // 后 getDay()=NaN，new Array(NaN) 抛 RangeError 曾致整窗黑屏——2026-08-15）
  const valid = cells.filter((c) => /^\d{4}-\d{2}-\d{2}$/.test(String(c.day)));
  if (valid.length === 0) return { cols: [], labels: [], max: 0 };
  const seq = (day: string) => {
    const [y, m, d] = day.split("-").map(Number);
    // days from civil（Howard Hinnant 算法）
    const yy = m <= 2 ? y - 1 : y;
    const mm = m > 2 ? m - 3 : m + 9;
    const era = Math.floor(yy / 400);
    const yoe = yy - era * 400;
    const doy = Math.floor((153 * mm + 2) / 5) + d - 1;
    const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
    return era * 146097 + doe - 719468; // 对齐 Unix epoch（1970-01-01 = 0）
  };
  const weekday = (day: string) => {
    const n = seq(day);
    // 1970-01-01 是周四：归一到周日起始
    return (n + 4 - 0 + 700000) % 7;
  };
  const byDay = new Map(valid.map((c) => [c.day, c]));
  const sorted = [...valid].sort((a, b) => a.day.localeCompare(b.day));
  const first = sorted[0].day;
  const last = sorted[sorted.length - 1].day;
  const max = Math.max(...valid.map((c) => c.calls), 1);
  const cols: ({ day: string; calls: number; sessions: number } | null)[][] = [];
  const labels: { col: number; label: string }[] = [];
  let cur: ({ day: string; calls: number; sessions: number } | null)[] = new Array(weekday(first)).fill(null);
  let lastMonth = -1;
  let colIdx = 0;
  for (let n = seq(first); n <= seq(last); n += 1) {
    // 序号 → y-m-d（civil_from_days）
    const z = n + 719468;
    const era = Math.floor(z / 146097);
    const doe = z - era * 146097;
    const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365);
    const y0 = yoe + era * 400;
    const doy0 = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100));
    const mp = Math.floor((5 * doy0 + 2) / 153);
    const d0 = doy0 - Math.floor((153 * mp + 2) / 5) + 1;
    const m0 = mp < 10 ? mp + 3 : mp - 9;
    const y1 = y0 + (m0 <= 2 ? 1 : 0);
    const key = `${y1}-${String(m0).padStart(2, "0")}-${String(d0).padStart(2, "0")}`;
    const c = byDay.get(key);
    cur.push(c ? { day: key, calls: c.calls, sessions: c.sessions ?? 0 } : { day: key, calls: 0, sessions: 0 });
    if (m0 !== lastMonth) {
      labels.push({ col: colIdx, label: `${m0}月` });
      lastMonth = m0;
    }
    if (cur.length === 7) {
      cols.push(cur);
      cur = [];
      colIdx += 1;
    }
  }
  if (cur.length > 0) {
    while (cur.length < 7) cur.push(null);
    cols.push(cur);
  }
  return { cols, labels, max };
}

/** 时段分组（凌晨/上午/下午/晚上）。 */
export function dayPart(hour: number): string {
  if (hour < 6) return "凌晨";
  if (hour < 12) return "上午";
  if (hour < 18) return "下午";
  return "晚上";
}

export default function ActivityView() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [days, setDays] = useState(365);

  useEffect(() => {
    (async () => {
      try { setStats(await invoke<Stats>("activity_stats", { days })); }
      catch { /* 空库静默 */ }
    })();
  }, [days]);

  const totalCalls = (stats?.heatmap ?? []).reduce((a, b) => a + b.calls, 0);
  const activeDays = (stats?.heatmap ?? []).filter((c) => c.calls > 0).length;
  const avgPerDay = activeDays > 0 ? Math.round(totalCalls / activeDays) : 0;
  const peak = (stats?.hourly ?? []).reduce((a, b) => (b.calls > (a?.calls ?? -1) ? b : a), { hour: 0, calls: 0 });
  const grid = buildHeatGrid(stats?.heatmap ?? []);
  const labelAt = new Map(grid.labels.map((l) => [l.col, l.label]));
  const rangeText = useMemo(() => daysToRange(days), [days]);

  // 工具趋势：全局 Top3 工具的月度线（BarChart 展示）
  // 防御性：过滤掉 month 为空/非字符串的脏行——后端早期版本以 tuple 序列化
  // 时，前端按对象读 month 全是 undefined，会把 byMonth 写成 [undefined]
  // 然后 `month.slice(2)` 抛 "undefined is not an object"
  const trend = (() => {
    if (!stats) return [];
    const safe = stats.tools_trend.filter((t) => typeof t.month === "string" && /^\d{4}-\d{2}$/.test(t.month));
    const toolTotals = new Map<string, number>();
    for (const t of safe) {
      toolTotals.set(t.tool, (toolTotals.get(t.tool) ?? 0) + t.calls);
    }
    const top3 = [...toolTotals.entries()].sort((a, b) => b[1] - a[1]).slice(0, 3).map(([n]) => n);
    const byMonth = new Map<string, { tool: string; calls: number }[]>();
    for (const t of safe) {
      if (!top3.includes(t.tool)) continue;
      const arr = byMonth.get(t.month) ?? [];
      arr.push({ tool: t.tool, calls: t.calls });
      byMonth.set(t.month, arr);
    }
    const out: { label: string; value: number }[] = [];
    for (const [month, tools] of [...byMonth.entries()].sort()) {
      const yymm = month.slice(2); // 已是防御过滤过的安全字符串
      for (const t of tools) out.push({ label: `${yymm} ${t.tool}`, value: t.calls });
    }
    return out;
  })();

  // 时段汇总
  const parts = (() => {
    const m = new Map<string, number>([["凌晨", 0], ["上午", 0], ["下午", 0], ["晚上", 0]]);
    for (const h of stats?.hourly ?? []) {
      m.set(dayPart(h.hour), (m.get(dayPart(h.hour)) ?? 0) + h.calls);
    }
    return m;
  })();
  const partsMax = Math.max(...parts.values(), 1);

  const isEmpty = !stats || (stats.heatmap.length === 0 && stats.hourly.length === 0 && stats.tools_trend.length === 0);
  // 24h BarChart 数据 + peak 高亮
  const hourlyChart = (stats?.hourly ?? []).map((h) => ({
    label: `${h.hour}`,
    value: h.calls,
    highlight: h.hour === peak.hour && peak.calls > 0,
  }));

  return (
    <div className="activity-page">
      <div className="ops-card">
        <div className="ops-card-title">
          📆 活动节律
          <span className="ops-card-sub">{rangeText}</span>
          <div className="ops-range" style={{ marginLeft: "auto" }}>
            {([90, 180, 365] as const).map((d) => (
              <button key={d} className={`filter-chip ${days === d ? "active" : ""}`} onClick={() => setDays(d)}>
                {d === 365 ? "1 年" : `${d} 天`}
              </button>
            ))}
          </div>
        </div>
        <div className="kb-grid">
          <div className="kb-stat" title="统计范围内全部工具调用次数"><b>{totalCalls.toLocaleString()}</b><span>工具调用</span></div>
          <div className="kb-stat" title="至少有 1 次调用的天数"><b>{activeDays}</b><span>活跃天数</span></div>
          <div className="kb-stat" title="总调用 ÷ 活跃天数"><b>{avgPerDay.toLocaleString()}</b><span>日均调用</span></div>
          <div className="kb-stat" title={`${peak.calls} 次调用集中在 ${peak.hour}:00`}><b>{String(peak.hour).padStart(2, "0")}:00</b><span>最活跃时段</span></div>
        </div>
      </div>

      <div className="ops-card">
        <div className="ops-card-title">每日协作热力图</div>
        {grid.cols.length === 0 ? (
          <div className="ops-table-empty">
            {isEmpty
              ? "📊 暂无活动数据 — 导入并使用 ZCode / Claude Code / Cursor / minimax / Codex 等 Agent 后，本页会按天聚合工具调用与活跃会话"
              : "暂无热力数据（同步指标后生成）"}
          </div>
        ) : (
          <div className="heatmap-wrap">
            <div className="heatmap-scroll">
              <div className="heatmap-months">
                {grid.cols.map((_, ci) => (
                  <span key={ci} className="heat-month-label">{labelAt.get(ci) ?? ""}</span>
                ))}
              </div>
              <div className="heatmap">
                {grid.cols.map((col, ci) => (
                  <div key={ci} className="heatmap-col">
                    {col.map((cell, ri) => (
                      <div
                        key={ri}
                        className={`heat-cell ${!cell || cell.calls === 0 ? "empty" : ""}`}
                        style={{ background: cell ? heatColor(cell.calls, grid.max) : "transparent" }}
                        title={cell ? `${cell.day} · ${cell.calls} 次调用 · ${cell.sessions} 会话` : "无数据"}
                      />
                    ))}
                  </div>
                ))}
              </div>
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
        <div className="ops-card-title">⏰ 24 小时分布{peak.calls > 0 && <span className="ops-card-sub">高峰 {String(peak.hour).padStart(2, "0")}:00 · {peak.calls.toLocaleString()} 次</span>}</div>
        {(stats?.hourly ?? []).length === 0 ? <div className="ops-table-empty">暂无数据</div> : (
          <>
            <div className="day-parts">
              {[...parts.entries()].map(([name, v]) => (
                <div key={name} className="day-part">
                  <span className="day-part-name">{name}</span>
                  <div className="day-part-bar"><div className="day-part-fill" style={{ width: `${(v / partsMax) * 100}%` }} /></div>
                  <span className="day-part-val">{v.toLocaleString()}</span>
                </div>
              ))}
            </div>
            <BarChart
              data={hourlyChart.map((h) => ({ label: h.label, value: h.value, className: h.highlight ? "bar-peak" : undefined }))}
              height={110}
            />
          </>
        )}
      </div>

      <div className="ops-card">
        <div className="ops-card-title">🔧 Top 工具月度趋势（用量前 3）</div>
        {trend.length === 0 ? <div className="ops-table-empty">暂无数据</div> : (
          <BarChart data={trend} height={130} />
        )}
      </div>
    </div>
  );
}
