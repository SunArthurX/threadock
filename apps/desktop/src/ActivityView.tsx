// 活动节律页（持续优化）：tool_daily/工作日-周末拆分/查看当日会话/24h 自定义 tooltip
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { BarChart } from "./charts";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";
import type { Conversation } from "./types";
import { formatTime } from "./types";
import HeatmapGitHub from "./HeatmapGitHub";
import { CardTitle } from "./CardTitle";
import { Skeleton } from "./Skeleton";
import { InlineEmpty } from "./EmptyState";
import { ListToolbar } from "./ListToolbar";
import { Icon } from "./Icon";

interface Stats {
  heatmap: { day: string; calls: number; sessions: number }[];
  hourly: { hour: number; calls: number }[];
  hourly_weekday: { hour: number; calls: number }[];
  hourly_weekend: { hour: number; calls: number }[];
  tools_trend: { month: string; tool: string; calls: number }[];
  tool_daily: { day: string; tool: string; calls: number }[];
}

/** 把 days 转成可读的「YYYY-MM-DD ~ YYYY-MM-DD」范围文案。 */
export function daysToRange(days: number, now: number = Date.now()): string {
  const end = new Date(now);
  const start = new Date(now - days * 86_400_000);
  const fmt = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  return `${fmt(start)} ~ ${fmt(end)}`;
}

/** 活动页完整数据 → CSV（UTF-8 BOM，Excel 友好）。多块拼接用空行分隔。 */
export function activityToCsv(s: {
  heatmap: { day: string; calls: number; sessions: number }[];
  hourly: { hour: number; calls: number }[];
  hourly_weekday?: { hour: number; calls: number }[];
  hourly_weekend?: { hour: number; calls: number }[];
  tools_trend: { month: string; tool: string; calls: number }[];
  tool_daily?: { day: string; tool: string; calls: number }[];
}): string {
  const esc = (v: unknown) => {
    const s = String(v ?? "");
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  const out: string[] = [];
  out.push("# 每日热力");
  out.push("日期,工具调用,活跃会话");
  s.heatmap.forEach((c) => out.push([c.day, c.calls, c.sessions].map(esc).join(",")));
  out.push("");
  out.push("# 24h 整体");
  out.push("小时,调用");
  s.hourly.forEach((h) => out.push([h.hour, h.calls].map(esc).join(",")));
  if (s.hourly_weekday?.length) {
    out.push("");
    out.push("# 24h 工作日");
    out.push("小时,调用");
    s.hourly_weekday.forEach((h) => out.push([h.hour, h.calls].map(esc).join(",")));
  }
  if (s.hourly_weekend?.length) {
    out.push("");
    out.push("# 24h 周末");
    out.push("小时,调用");
    s.hourly_weekend.forEach((h) => out.push([h.hour, h.calls].map(esc).join(",")));
  }
  out.push("");
  out.push("# 工具月度趋势");
  out.push("月份,工具,调用");
  s.tools_trend.forEach((t) => out.push([t.month, t.tool, t.calls].map(esc).join(",")));
  if (s.tool_daily?.length) {
    out.push("");
    out.push("# 工具日级（限最近 90 天）");
    out.push("日期,工具,调用");
    s.tool_daily.forEach((t) => out.push([t.day, t.tool, t.calls].map(esc).join(",")));
  }
  return "\uFEFF" + out.join("\n");
}

/** 热力图单元格颜色（5 档 GitHub 风格绿色梯度）。 */
export function heatColor(calls: number, max: number): string {
  if (calls === 0) return "transparent"; // 0 档：透明 + 边框（CSS）
  const r = max > 0 ? calls / max : 0;
  if (r > 0.75) return "#39d353"; // 4 档：亮绿
  if (r > 0.5) return "#26a641"; // 3 档：中绿
  if (r > 0.25) return "#006d32"; // 2 档：深绿
  return "#0e4429"; // 1 档：最深绿（>0 但比例小）
}

/** 0-4 档分档（与 heatColor 一致），用于 legend 显示 5 个独立色块。 */
export function heatLevel(calls: number, max: number): number {
  if (calls === 0) return 0;
  const r = max > 0 ? calls / max : 0;
  if (r > 0.75) return 4;
  if (r > 0.5) return 3;
  if (r > 0.25) return 2;
  return 1;
}

/** 5 档 GitHub 绿色（按 0-4 索引）。legend 渲染用。 */
export const HEAT_LEVELS = ["transparent", "#0e4429", "#006d32", "#26a641", "#39d353"];

/** 工作日判定（0=周日）。 */
export function isWeekend(day: string): boolean {
  const d = new Date(day + "T00:00:00").getDay();
  return d === 0 || d === 6;
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
      const en = ["", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
      labels.push({ col: colIdx, label: en[m0] ?? `${m0}月` });
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

/** 中文星期几。 */
export function weekdayCN(day: string): string {
  const names = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  return names[new Date(day + "T00:00:00").getDay()];
}

/** YYYY-MM-DD 字符串当天的「今日/最近 7/30」快速聚合。 */
function dayWindowSum(
  cells: { day: string; calls: number; sessions: number }[],
  window: number,
): { calls: number; sessions: number } {
  if (cells.length === 0) return { calls: 0, sessions: 0 };
  const now = Date.now();
  const cutoff = now - window * 86_400_000;
  let calls = 0;
  let sessions = 0;
  for (const c of cells) {
    const t = new Date(c.day + "T00:00:00").getTime();
    if (t >= cutoff) {
      calls += c.calls;
      sessions += c.sessions;
    }
  }
  return { calls, sessions };
}

/** 计算连续活跃天数（从今天往前数 ≥1 次调用的连续天数）。 */
export function calcStreak(cells: { day: string; calls: number }[]): number {
  if (cells.length === 0) return 0;
  const set = new Set(cells.filter((c) => c.calls > 0).map((c) => c.day));
  let streak = 0;
  const d = new Date();
  let todayChecked = false;
  while (true) {
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    if (set.has(key)) {
      streak += 1;
      d.setDate(d.getDate() - 1);
      todayChecked = true;
    } else {
      // 如果今天没活动，容许从昨天开始算（避免刚启动时 streak=0）
      if (streak === 0 && !todayChecked) {
        d.setDate(d.getDate() - 1);
        todayChecked = true;
        continue;
      }
      break;
    }
  }
  return streak;
}

export default function ActivityView({ onJumpToConversation }: { onJumpToConversation?: (conversationId: string) => void } = {}) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [days, setDays] = useState(365);
  const [selectedDay, setSelectedDay] = useState<string | null>(null);
  const [dayConvs, setDayConvs] = useState<Conversation[] | null>(null);
  const [dayConvsLoading, setDayConvsLoading] = useState(false);
  // P1-C5: 日历年度选择 — "all"=跟 days 走（90/180/365）；具体年份强制 span Jan 1 ~ Dec 31
  const [year, setYear] = useState<number | "all">("all");
  // P1-C3: todayKey 跨午夜过期 → 用 state + 1 分钟 interval 自动刷新
  //   1) 跨日后「今天」高亮 / 「连续活跃」streak / day-detail badge 都立即切换
  //   2) 组件 unmount 时 clearInterval 避免内存泄漏
  const computeTodayKey = () => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  };
  const [todayKey, setTodayKey] = useState<string>(computeTodayKey);
  // 热力图维度：all=全工具 / 单一工具
  const [toolFilter, setToolFilter] = useState<string | "all">("all");
  // 工具维度下，热力值 = 当天该工具的调用数；非选中工具的热力 = 全工具值（保留对比）
  // 但色阶按当前工具的 max 计算（视觉一致）
  const filteredHeatmap = useMemo(() => {
    if (toolFilter === "all" || !stats) return stats?.heatmap ?? [];
    if (!stats.tool_daily || stats.tool_daily.length === 0) return stats?.heatmap ?? [];
    const byDay = new Map<string, number>();
    for (const t of stats.tool_daily) {
      if (t.tool === toolFilter) byDay.set(t.day, (byDay.get(t.day) ?? 0) + t.calls);
    }
    return (stats.heatmap ?? []).map((c) => ({ ...c, calls: byDay.get(c.day) ?? 0 }));
  }, [toolFilter, stats]);
  const toolList = useMemo(() => {
    if (!stats?.tool_daily) return [] as string[];
    return [...new Set(stats.tool_daily.map((t) => t.tool))].sort();
  }, [stats]);

  useEffect(() => {
    (async () => {
      try { setStats(await invoke<Stats>("activity_stats", { days })); }
      catch { /* 空库静默 */ }
    })();
  }, [days]);

  // P1-C5: 年度选择 — 把 days 强制覆盖为「从今天到目标年 1/1」的天数，覆盖整年
  // （year 与 days 是两个独立状态，选中年份时需要同步派生 days；重构为纯派生值
  //  会牵动 90/180/365 按钮、CSV 导出等多处语义，此处保持 effect 同步并豁免检查）
  useEffect(() => {
    if (year === "all") return;
    const yearStart = new Date(`${year}-01-01T00:00:00`).getTime();
    const span = Math.max(1, Math.ceil((Date.now() - yearStart) / 86_400_000));
    // eslint-disable-next-line react-hooks/set-state-in-effect -- year→days 状态同步（P1-C5）
    setDays(span);
  }, [year]);

  // P1-C5: 年度选项 = 数据中出现的年份 ∪ 当前年（倒序）
  const currentYear = new Date().getFullYear();
  const yearOptions = useMemo<number[]>(() => {
    const ys = new Set<number>([currentYear]);
    for (const c of stats?.heatmap ?? []) {
      const y = Number(c.day.slice(0, 4));
      if (!Number.isNaN(y)) ys.add(y);
    }
    return [...ys].sort((a, b) => b - a);
  }, [stats, currentYear]);

  // P1-C5: 渲染层只展示目标年的 cells（grid 用）— 今日/7/30 窗口不受影响
  const visibleHeatmap = useMemo(() => {
    if (year === "all") return filteredHeatmap;
    const yPrefix = `${year}-`;
    return filteredHeatmap.filter((c) => c.day.startsWith(yPrefix));
  }, [filteredHeatmap, year]);

  const heatmapCells = filteredHeatmap;
  // P1-C5: 年度视图下，统计卡累计值用年份范围内的 cells；今日/7/30 仍走原始 cells
  const cumulativeCells = visibleHeatmap;
  const totalCalls = cumulativeCells.reduce((a, b) => a + b.calls, 0);
  const activeDays = cumulativeCells.filter((c) => c.calls > 0).length;
  const avgPerDay = activeDays > 0 ? Math.round(totalCalls / activeDays) : 0;
  const peak = (stats?.hourly ?? []).reduce((a, b) => (b.calls > (a?.calls ?? -1) ? b : a), { hour: 0, calls: 0 });
  const grid = buildHeatGrid(visibleHeatmap);
  const labelAt = new Map(grid.labels.map((l) => [l.col, l.label]));
  // P1-C5: 年份选择时显示「2024-01-01 ~ 2024-12-31」而不是 days 推算的 today 范围
  const rangeText = useMemo(() => {
    if (year === "all") return daysToRange(days);
    return `${year}-01-01 ~ ${year}-12-31`;
  }, [days, year]);

  // 近期窗口（今日/7 天/30 天）
  const todayStats = useMemo(() => dayWindowSum(heatmapCells, 1), [heatmapCells]);
  const week7Stats = useMemo(() => dayWindowSum(heatmapCells, 7), [heatmapCells]);
  const month30Stats = useMemo(() => dayWindowSum(heatmapCells, 30), [heatmapCells]);
  // P1-C3: todayKey 变化时（跨午夜后）也重算 streak，否则「连续活跃」会卡在昨天的计数
  // （todayKey 是有意的额外依赖：calcStreak 内部取当前日期，跨午夜需借 todayKey 触发重算）
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const streak = useMemo(() => calcStreak(heatmapCells), [heatmapCells, todayKey]);

  // 工具 Top 10 列表（带月份切分）
  // 防御性：过滤掉 month 为空/非字符串的脏行
  const toolRanking = useMemo<{
    curMonth: string;
    prevMonth: string;
    items: { tool: string; calls: number; cur: number; prev: number; delta: number; barWidth: number }[];
  }>(() => {
    if (!stats) return { curMonth: "", prevMonth: "", items: [] };
    const safe = (stats.tools_trend ?? []).filter((t) => typeof t.month === "string" && /^\d{4}-\d{2}$/.test(t.month));
    const months = [...new Set(safe.map((t) => t.month))].sort();
    const curMonth = months[months.length - 1] ?? "";
    const prevMonth = months[months.length - 2] ?? "";
    const cur = new Map<string, number>();
    const prev = new Map<string, number>();
    for (const t of safe) {
      if (t.month === curMonth) cur.set(t.tool, (cur.get(t.tool) ?? 0) + t.calls);
      if (t.month === prevMonth) prev.set(t.tool, (prev.get(t.tool) ?? 0) + t.calls);
    }
    const all = new Map<string, number>();
    for (const [tool, c] of cur) all.set(tool, c);
    for (const [tool, p] of prev) all.set(tool, Math.max(all.get(tool) ?? 0, p));
    const ranked = [...all.entries()].sort((a, b) => b[1] - a[1]).slice(0, 10);
    const max = ranked[0]?.[1] ?? 1;
    return {
      curMonth,
      prevMonth,
      items: ranked.map(([tool, calls]) => {
        const c = cur.get(tool) ?? 0;
        const p = prev.get(tool) ?? 0;
        const delta = p === 0 ? (c > 0 ? 1 : 0) : (c - p) / p;
        return { tool, calls, cur: c, prev: p, delta, barWidth: (calls / max) * 100 };
      }),
    };
  }, [stats]);

  // 24h 工作日 vs 周末 对比 — 后端暂未拆分，先只展示整体曲线 + 留扩展位
  // （如需拆分需在 SQL 加 GROUP BY strftime('%w', ...)，后续可补）

  // 时段汇总
  const parts = (() => {
    const m = new Map<string, number>([["凌晨", 0], ["上午", 0], ["下午", 0], ["晚上", 0]]);
    for (const h of stats?.hourly ?? []) {
      m.set(dayPart(h.hour), (m.get(dayPart(h.hour)) ?? 0) + h.calls);
    }
    return m;
  })();
  const partsMax = Math.max(...parts.values(), 1);

  const isEmpty = !stats || ((stats.heatmap ?? []).length === 0 && (stats.hourly ?? []).length === 0 && (stats.tools_trend ?? []).length === 0);
  // 24h BarChart 数据 + peak 高亮
  const hourlyChart = (stats?.hourly ?? []).map((h) => ({
    label: `${h.hour}`,
    value: h.calls,
    className: h.hour === peak.hour && peak.calls > 0 ? "bar-peak" : undefined,
  }));

  // 选中日详情
  const selectedCell = selectedDay ? heatmapCells.find((c) => c.day === selectedDay) ?? null : null;
  // 优先用 tool_daily（日级精确），fallback 到 tools_trend 按月聚合
  const selectedDayTools = useMemo(() => {
    if (!selectedDay || !stats) return [];
    const safeDaily = (stats.tool_daily ?? []).filter((t) => t.day === selectedDay);
    if (safeDaily.length > 0) {
      const total = safeDaily.reduce((s, t) => s + t.calls, 0);
      return safeDaily
        .sort((a, b) => b.calls - a.calls)
        .slice(0, 5)
        .map((t) => ({ ...t, share: total > 0 ? (t.calls / total) * 100 : 0 }));
    }
    const month = selectedDay.slice(0, 7);
    const safeMonth = (stats.tools_trend ?? []).filter((t) => t.month === month);
    const total = safeMonth.reduce((s, t) => s + t.calls, 0);
    return safeMonth
      .sort((a, b) => b.calls - a.calls)
      .slice(0, 5)
      .map((t) => ({ ...t, share: total > 0 ? (t.calls / total) * 100 : 0 }));
  }, [selectedDay, stats]);

  /** 导出活动页全量数据为 CSV（剪贴板 + 可选保存到文件）。 */
  const exportCsv = async (toFile = false) => {
    if (!stats) return;
    const csv = activityToCsv(stats);
    if (toFile) {
      try {
        const path = await save({
          defaultPath: `threadock-activity-${new Date().toISOString().slice(0, 10)}.csv`,
          filters: [{ name: "CSV", extensions: ["csv"] }],
        });
        if (!path) return;
        // Tauri 2.x 通过 fs plugin 写文件；这里用 clipboard + 提示让用户保存到文件路径
        await navigator.clipboard.writeText(csv);
        showToast(`✓ CSV 已复制到剪贴板，请粘贴到 ${path}`, "info", 8000);
      } catch (e) {
        showToast(`保存失败：${String(e)}`, "error");
      }
    } else {
      try {
        await navigator.clipboard.writeText(csv);
        showToast("✓ 活动数据 CSV 已复制到剪贴板", "info");
      } catch {
        showToast("剪贴板不可用", "error");
      }
    }
  };

  // 选中日点击「查看当日会话」→ 拉会话列表
  const loadDayConvs = async () => {
    if (!selectedDay) return;
    setDayConvsLoading(true);
    try {
      const fromMs = new Date(selectedDay + "T00:00:00").getTime();
      const toMs = fromMs + 86_400_000 - 1; // 当天 23:59:59.999
      const list = await invoke<Conversation[]>("list_conversations_by_date", { fromMs, toMs });
      setDayConvs(list);
    } catch (e) {
      showToast(`查询失败：${String(e)}`, "error");
      setDayConvs(null);
    } finally {
      setDayConvsLoading(false);
    }
  };
  // 选中日点击「查看当日会话」→ 拉会话列表（典型的 effect 数据加载模式：
  // 先清旧数据再异步拉取；selectedDay 驱动的加载无法移入事件处理器，有意保留）
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 清旧数据后异步加载当日会话
    setDayConvs(null);
    if (selectedDay) loadDayConvs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedDay]);

  // P1-C3: todayKey 每分钟自动刷新，跨午夜后立刻更新「今天」标记
  useEffect(() => {
    const id = setInterval(() => setTodayKey(computeTodayKey()), 60_000);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="activity-page">
      <div className="ops-card">
        <CardTitle icon="calendar" sub={rangeText} trailing={stats ? (
          <>
            <button className="action-btn" onClick={() => exportCsv(false)}
              title="把热力+时段+工具明细复制为 CSV（Excel 友好 UTF-8 BOM）">
              <Icon name="copy" size={12} /> 复制 CSV
            </button>
            <button className="action-btn" onClick={() => exportCsv(true)}
              title="弹文件选择对话框，把 CSV 保存到磁盘（实际写入需要 clipboard 兜底）">
              <Icon name="save" size={12} /> 保存 CSV
            </button>
          </>
        ) : null}>活动节律</CardTitle>
        <div className="ops-range-wrap">
          <div className="ops-range">
            {([90, 180, 365] as const).map((d) => (
              <button
                key={d}
                className={`filter-chip ${days === d ? "active" : ""}`}
                onClick={() => setDays(d)}
                disabled={year !== "all"}
                title={year !== "all" ? "切换到「全部」后可选" : "按天数查看"}
              >
                {d === 365 ? "1 年" : `${d} 天`}
              </button>
            ))}
            {/* P1-C5: 日历年度选择 — 数据中出现的年份 ∪ 当前年；"全部" 切回 90/180/365 */}
            <span style={{ width: 1, height: 16, background: "var(--border-default, rgba(148,163,199,0.18))", margin: "0 2px" }} />
            <button
              key="all-years"
              className={`filter-chip ${year === "all" ? "active" : ""}`}
              onClick={() => { setYear("all"); setDays(365); }}
              title="按右上 90/180/365 天查看"
            >
              全部
            </button>
            {yearOptions.map((y) => (
              <button
                key={y}
                className={`filter-chip ${year === y ? "active" : ""}`}
                onClick={() => setYear(y)}
                title={`只看 ${y} 年（Jan 1 ~ Dec 31）`}
              >
                {y}
              </button>
            ))}
          </div>
        </div>
        <div className="kb-grid-grouped">
          <div className="kb-grid-section">
            <div className="kb-grid-section-label">活动度量</div>
            <div className="kb-grid">
              <div className="kb-stat kpi-primary" title="统计范围内全部工具调用次数"><b>{totalCalls.toLocaleString()}</b><span>工具调用</span></div>
              <div className="kb-stat kpi-primary" title="至少有 1 次调用的天数"><b>{activeDays}</b><span>活跃天数</span></div>
              <div className="kb-stat kpi-primary" title="总调用 ÷ 活跃天数"><b>{avgPerDay.toLocaleString()}</b><span>日均调用</span></div>
              <div className="kb-stat kpi-primary" title="从今天或昨天起算的连续活跃天数"><b>{streak} 天</b><span><Icon name="flame" size={11} /> 连续活跃</span></div>
            </div>
          </div>
          <div className="kb-grid-section">
            <div className="kb-grid-section-label">时间分布</div>
            <div className="kb-grid">
              <div className="kb-stat kpi-secondary" title={`${peak.calls} 次调用集中在 ${peak.hour}:00`}><b>{String(peak.hour).padStart(2, "0")}:00</b><span>最活跃时段</span></div>
              <div className="kb-stat kpi-secondary" title="今日（最近 1 天）总调用"><b>{todayStats.calls.toLocaleString()}</b><span>今日 · {todayStats.sessions} 会话</span></div>
              <div className="kb-stat kpi-secondary" title="最近 7 天总调用"><b>{week7Stats.calls.toLocaleString()}</b><span>近 7 天 · {week7Stats.sessions} 会话</span></div>
              <div className="kb-stat kpi-secondary" title="最近 30 天总调用"><b>{month30Stats.calls.toLocaleString()}</b><span>近 30 天 · {month30Stats.sessions} 会话</span></div>
            </div>
          </div>
        </div>
      </div>

      <div className="ops-card">
        <CardTitle icon="calendar" sub={`${totalCalls.toLocaleString()} contributions in the last ${days} days`} trailing={
          <ListToolbar
            dense
            filterLabel="维度"
            filterValue={toolFilter === "all" ? "__all__" : toolFilter}
            onFilterChange={(v) => setToolFilter(v === "__all__" ? "all" : v)}
            filterOptions={[{ value: "__all__", label: "全部工具" }, ...toolList.map((t) => ({ value: t, label: t }))]}
            count={totalCalls}
            countLabel="次调用"
          />
        }>活动热力图</CardTitle>
        {grid.cols.length === 0 ? (
          stats === null
            ? <Skeleton variant="heatmap" />
            : <InlineEmpty
                message="暂无活动热力数据"
                hint={isEmpty
                  ? "同步并使用 ZCode / Claude Code / Cursor / MiniMax / Codex 等 Agent 后，本页会按天聚合"
                  : "同步指标后生成热力"}
              />
        ) : (
          <div className="heatmap-wrap">
            <div className="heatmap-scroll">
              {/* 完全独立的 GitHub 风格热力图组件：inline style 强制 12×12 + aspect-ratio 兜底，
                  避开 styles.css 中所有 .heat-cell 历史冲突。7 行（Mon→Sun）× N 列（周数） */}
              <HeatmapGitHub
                cols={grid.cols.map((col) => ({
                  cells: col.map((c) =>
                    c ? { day: c.day, calls: c.calls, sessions: c.sessions } : null,
                  ),
                }))}
                max={grid.max}
                monthLabels={labelAt}
                selectedDay={selectedDay}
                todayKey={todayKey}
                year={year}
                onClickCell={(day) => setSelectedDay(selectedDay === day ? null : day)}
              />
            </div>
            <div className="heat-legend">
              Less
              {HEAT_LEVELS.map((c, i) => (
                <span key={i} className="heat-legend-cell" style={{ background: c, border: i === 0 ? "1px solid rgba(255,255,255,0.1)" : "1px solid transparent" }} />
              ))}
              More
            </div>

            {selectedCell && (
              <div className="day-detail">
                <div className="day-detail-head">
                  <span className="day-detail-date">{selectedCell.day}</span>
                  <span className="day-detail-weekday">{weekdayCN(selectedCell.day)}</span>
                  {isWeekend(selectedCell.day) && <span className="day-detail-weekday" style={{ background: "rgba(244,114,182,0.18)", color: "#f472b6" }}>周末</span>}
                  {selectedCell.day === todayKey && <span className="day-detail-weekday" style={{ background: "rgba(96,165,250,0.18)", color: "#60a5fa" }}>今天</span>}
                  <button
                    className="action-btn"
                    style={{ marginLeft: "auto", fontSize: 11 }}
                    onClick={loadDayConvs}
                    disabled={dayConvsLoading}
                    title={`查询 ${selectedCell.day} 的主任务会话列表`}
                  >
                    {dayConvsLoading ? "查询中…" : dayConvs ? "↻ 重新查询" : "💬 查看当日会话"}
                  </button>
                  <button className="day-detail-close" onClick={() => { setSelectedDay(null); setDayConvs(null); }}>✕ 关闭</button>
                </div>
                <div className="day-detail-stats">
                  <div className="day-detail-stat"><b>{selectedCell.calls.toLocaleString()}</b><span>工具调用</span></div>
                  <div className="day-detail-stat"><b>{selectedCell.sessions}</b><span>活跃会话</span></div>
                  <div className="day-detail-stat"><b>{selectedCell.sessions > 0 ? (selectedCell.calls / selectedCell.sessions).toFixed(1) : "0"}</b><span>平均每次会话</span></div>
                </div>
                {selectedDayTools.length > 0 ? (
                  <>
                    <div className="day-detail-tools-title">当日工具分布（最常调用 Top 5）</div>
                    <div className="day-detail-tools">
                      {selectedDayTools.map((t) => (
                        <div key={t.tool} className="day-detail-tool-row">
                          <span className="day-detail-tool-name">{t.tool}</span>
                          <div className="day-detail-tool-bar">
                            <div className="day-detail-tool-fill" style={{ width: `${t.share}%` }} />
                          </div>
                          <span className="day-detail-tool-count">{t.calls.toLocaleString()} 次 · {t.share.toFixed(0)}%</span>
                        </div>
                      ))}
                    </div>
                  </>
                ) : (
                  <div className="ops-table-empty" style={{ padding: "10px 0" }}>
                    当日暂无工具分布数据（点击「查看当日会话」看真实会话）
                  </div>
                )}
                {dayConvs && (
                  <div className="day-detail-convs">
                    <div className="day-detail-tools-title">
                      {selectedCell.day} 的主任务会话（{dayConvs.length} 条）
                    </div>
                    {dayConvs.length === 0 ? (
                      <div className="ops-table-empty" style={{ padding: "10px 0" }}>
                        当日没有主任务会话（仅子任务或无 started_at 数据）
                      </div>
                    ) : (
                      <ScrollArea className="day-detail-conv-list">
                        {dayConvs.slice(0, 12).map((c) => (
                          <div
                            key={c.id}
                            className="day-detail-conv-row"
                            onClick={() => onJumpToConversation?.(c.id)}
                            title="点击跳转到该会话"
                          >
                            <span className="day-detail-conv-provider">{c.provider}</span>
                            <span className="day-detail-conv-title">{c.user_title ?? c.title ?? "(无标题)"}</span>
                            <span className="day-detail-conv-time">{formatTime(c.started_at_ms ?? null)}</span>
                          </div>
                        ))}
                        {dayConvs.length > 12 && (
                          <div className="ops-table-empty" style={{ padding: "6px 0", fontSize: 11 }}>
                            还有 {dayConvs.length - 12} 条未展示（去 chat 页查看完整列表）
                          </div>
                        )}
                      </ScrollArea>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </div>

      <div className="ops-card">
        <CardTitle icon="clock" sub={peak.calls > 0 ? `高峰 ${String(peak.hour).padStart(2, "0")}:00 · ${peak.calls.toLocaleString()} 次` : undefined}>24 小时分布</CardTitle>
        {(stats?.hourly ?? []).length === 0
          ? (stats === null ? <Skeleton variant="chart-bars" count={12} height={140} /> : <InlineEmpty message="暂无 24 小时分布数据" />)
          : (
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
            <div className="hourly-split">
              <div className="hourly-split-item"><span className="hourly-split-dot wd" /> 工作日（周一~五）</div>
              <div className="hourly-split-item"><span className="hourly-split-dot we" /> 周末（周六~日）</div>
              <div className="hourly-split-item" style={{ marginLeft: "auto", opacity: 0.6 }}>悬停柱子看明细</div>
            </div>
            <BarChart
              data={hourlyChart.map((h) => ({ label: h.label, value: h.value, className: h.className }))}
              height={120}
              axisLabel={(d) => `${d.label}:00`}
              renderTooltip={(d, max) => {
                const wd = stats?.hourly_weekday?.[Number(d.label)]?.calls ?? 0;
                const we = stats?.hourly_weekend?.[Number(d.label)]?.calls ?? 0;
                const total = Number(d.value);
                const part = dayPart(Number(d.label));
                return (
                  <>
                    <div className="tooltip-title">{d.label}:00 · {part}</div>
                    <div className="tooltip-row">
                      <span className="tooltip-dot" style={{ background: "var(--accent)" }} />
                      <span>总调用 <b style={{ marginLeft: 4 }}>{total.toLocaleString()}</b>（{((total / max) * 100).toFixed(0)}% of peak）</span>
                    </div>
                    <div className="tooltip-row" style={{ marginTop: 2 }}>
                      <span className="tooltip-dot" style={{ background: "#60a5fa" }} />
                      <span>工作日 <b style={{ marginLeft: 4 }}>{wd.toLocaleString()}</b></span>
                    </div>
                    <div className="tooltip-row">
                      <span className="tooltip-dot" style={{ background: "#f472b6" }} />
                      <span>周末 <b style={{ marginLeft: 4 }}>{we.toLocaleString()}</b></span>
                    </div>
                  </>
                );
              }}
            />
          </>
        )}
      </div>

      <div className="ops-card">
        <CardTitle icon="wand" sub={toolRanking.curMonth ? `${toolRanking.curMonth.slice(2)} 月${toolRanking.prevMonth ? ` · 对比 ${toolRanking.prevMonth.slice(2)} 月` : ""}` : undefined}>工具使用 Top 10</CardTitle>
        {toolRanking.items.length === 0 ? (
          stats === null ? <Skeleton variant="list" count={6} /> : <InlineEmpty message="暂无工具使用排行" hint="导入并使用 Agent 后会按月统计" />
        ) : (
          <div className="tool-rank-list">
            {toolRanking.items.map((t, i) => {
              const dCls = !toolRanking.prevMonth ? "flat" : t.delta > 0.05 ? "up" : t.delta < -0.05 ? "down" : "flat";
              const dText = !toolRanking.prevMonth
                ? "—"
                : t.delta > 0.05 ? `↑ ${(t.delta * 100).toFixed(0)}%`
                : t.delta < -0.05 ? `↓ ${Math.abs(t.delta * 100).toFixed(0)}%`
                : "持平";
              return (
                <div key={t.tool} className="tool-rank-row">
                  <span className={`tool-rank-num ${i < 3 ? "top" : ""}`}>{i + 1}</span>
                  <span className="tool-rank-name">{t.tool}</span>
                  <div className="tool-rank-bar"><div className="tool-rank-fill" style={{ width: `${t.barWidth}%` }} /></div>
                  <span className="tool-rank-count">{t.calls.toLocaleString()}</span>
                  <span className={`tool-rank-delta ${dCls}`}>{dText}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
