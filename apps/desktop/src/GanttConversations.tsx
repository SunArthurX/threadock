// 会话甘特图：每会话一行，条 = started_at → updated_at 跨度，按 Agent 着色。
// 悬停浮动详情（fixed 定位，沿 HeatmapGitHub 模式，不被滚动容器裁剪），
// 点击条跳转会话详情；跨范围边界的会话裁剪到窗口内显示。
import { useEffect, useMemo, useState } from "react";
import type { Conversation } from "./types";
import { meta } from "./ops-types";
import { CardTitle } from "./CardTitle";
import { Skeleton } from "./Skeleton";
import { InlineEmpty } from "./EmptyState";

/** 单行布局（leftPct/widthPct 相对整个时间窗口）。 */
export interface GanttRow {
  conv: Conversation;
  leftPct: number;
  widthPct: number;
}

/** 纯布局：会话列表 + 时间窗口 → 行（裁剪到窗口、按开始时间倒序）+ 轴刻度。 */
export function buildGanttRows(
  convs: Conversation[],
  fromMs: number,
  toMs: number,
  opts: { maxRows?: number } = {},
): { rows: GanttRow[]; total: number; ticks: { pct: number; label: string }[] } {
  const maxRows = opts.maxRows ?? 80;
  const span = Math.max(1, toMs - fromMs);
  const spans = convs
    .filter((c) => typeof c.started_at_ms === "number" && (c.started_at_ms ?? 0) > 0)
    .map((c) => {
      const s = c.started_at_ms as number;
      return { conv: c, s, e: Math.max(s, c.updated_at_ms ?? s) };
    })
    .filter((sp) => sp.e >= fromMs && sp.s <= toMs)
    .sort((a, b) => b.s - a.s);
  const rows: GanttRow[] = spans.map((sp) => {
    const cs = Math.max(sp.s, fromMs);
    const ce = Math.min(sp.e, toMs);
    return {
      conv: sp.conv,
      leftPct: ((cs - fromMs) / span) * 100,
      widthPct: Math.max(((ce - cs) / span) * 100, 0.6), // 极短会话保底可见
    };
  });
  const ticks = Array.from({ length: 5 }, (_, i) => {
    const t = fromMs + (span * i) / 4;
    const d = new Date(t);
    return {
      pct: (i / 4) * 100,
      label: i === 4 ? "" : `${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`,
    };
  });
  return { rows: rows.slice(0, maxRows), total: rows.length, ticks };
}

/** 跨度人话：「45 秒」「3 小时 12 分」「2 天 4 小时」。 */
export function ganttSpanText(ms: number): string {
  if (ms < 1000) return "≤1 秒";
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s} 秒`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分钟`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时 ${m % 60} 分`;
  const d = Math.floor(h / 24);
  return `${d} 天 ${h % 24} 小时`;
}

/** ms → 「MM-DD HH:mm」（本地时区；数值毫秒构造 Date 在 WKWebView 安全）。 */
export function ganttTimeText(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** 当前时刻（「今天」刻度用；函数引用形式供 useState 初始化，绕开 render 期 purity 限制）。 */
const nowMs = () => Date.now();

export default function GanttConversations({
  convs,
  loading,
  fromMs,
  toMs,
  rangeLabel,
  onJumpToConversation,
}: {
  convs: Conversation[] | null;
  loading: boolean;
  fromMs: number;
  toMs: number;
  rangeLabel?: string;
  onJumpToConversation?: (conversationId: string) => void;
}) {
  const [tip, setTip] = useState<{ x: number; y: number; row: GanttRow } | null>(null);
  // 「今天」刻度：跨午夜自动换位（沿 ActivityView todayKey 的 1 分钟刷新模式）
  const [nowTick, setNowTick] = useState<number>(nowMs);
  useEffect(() => {
    const id = setInterval(() => setNowTick(Date.now()), 60_000);
    return () => clearInterval(id);
  }, []);

  const { rows, total, ticks } = useMemo(
    () => buildGanttRows(convs ?? [], fromMs, toMs),
    [convs, fromMs, toMs],
  );
  const providers = useMemo(
    () => [...new Set((convs ?? []).map((c) => c.provider))].sort(),
    [convs],
  );
  const nowPct = useMemo(() => {
    return nowTick >= fromMs && nowTick <= toMs
      ? ((nowTick - fromMs) / Math.max(1, toMs - fromMs)) * 100
      : null;
  }, [fromMs, toMs, nowTick]);

  const showSkeleton = loading || convs === null;

  return (
    <div className="ops-card">
      <CardTitle
        icon="chart"
        sub={rangeLabel ? `${total.toLocaleString()} 个会话 · ${rangeLabel}` : `${total.toLocaleString()} 个会话`}
        trailing={
          providers.length > 0 ? (
            <div className="gantt-legend">
              {providers.map((p) => (
                <span key={p} className="gantt-legend-item">
                  <span className="gantt-legend-dot" style={{ background: meta(p).color }} />
                  {meta(p).label}
                </span>
              ))}
            </div>
          ) : null
        }
      >
        会话甘特图
      </CardTitle>
      {showSkeleton ? (
        <Skeleton variant="list" count={6} />
      ) : rows.length === 0 ? (
        <InlineEmpty message="时间范围内没有会话" hint="同步 Agent 数据后，这里按时间跨度排布每个会话" />
      ) : (
        <div className="gantt-wrap">
          <div className="gantt-axis">
            <div className="gantt-axis-gutter" />
            <div className="gantt-axis-track">
              {ticks.map((t, i) => (
                <span key={i} className="gantt-axis-tick" style={{ left: `${t.pct}%` }} data-testid="gantt-axis-tick">
                  {t.label}
                </span>
              ))}
            </div>
          </div>
          <div className="gantt-scroll">
            {rows.map((row) => {
              const m = meta(row.conv.provider);
              const dur = Math.max(
                (row.conv.updated_at_ms ?? row.conv.started_at_ms ?? 0) -
                  (row.conv.started_at_ms ?? 0),
                0,
              );
              return (
                <div
                  key={row.conv.id}
                  className="gantt-row"
                  data-testid="gantt-row"
                  onClick={() => onJumpToConversation?.(row.conv.id)}
                  title={`${row.conv.user_title ?? row.conv.title ?? "(无标题)"} · ${m.label}`}
                >
                  <div className="gantt-label">
                    <span className="gantt-label-dot" style={{ background: m.color }} />
                    <span className="gantt-label-text">{row.conv.user_title ?? row.conv.title ?? "(无标题)"}</span>
                  </div>
                  <div className="gantt-track">
                    {nowPct !== null && <div className="gantt-now" style={{ left: `${nowPct}%` }} data-testid="gantt-today" />}
                    <div
                      className="gantt-bar"
                      data-testid="gantt-bar"
                      style={{ left: `${row.leftPct}%`, width: `${row.widthPct}%`, background: m.color }}
                      onMouseMove={(e) => setTip({ x: e.clientX, y: e.clientY, row })}
                      onMouseLeave={() => setTip(null)}
                    />
                  </div>
                  <span className="gantt-span">{ganttSpanText(dur)}</span>
                </div>
              );
            })}
            {total > rows.length && (
              <div className="gantt-more">仅显示最近 {rows.length} 条（共 {total.toLocaleString()}），缩小时间范围查看更早会话</div>
            )}
          </div>
        </div>
      )}
      {tip && (
        <div className="gantt-tooltip" style={{ left: tip.x + 14, top: tip.y + 14 }} data-testid="gantt-tooltip">
          <div className="tooltip-title">{tip.row.conv.user_title ?? tip.row.conv.title ?? "(无标题)"}</div>
          <div className="tooltip-row">
            <span className="tooltip-dot" style={{ background: meta(tip.row.conv.provider).color }} />
            <span>{meta(tip.row.conv.provider).label}</span>
          </div>
          <div className="tooltip-sub">
            {ganttTimeText(tip.row.conv.started_at_ms ?? 0)} ~{" "}
            {ganttTimeText(tip.row.conv.updated_at_ms ?? tip.row.conv.started_at_ms ?? 0)}
          </div>
          <div className="tooltip-sub">
            跨度 {ganttSpanText(Math.max((tip.row.conv.updated_at_ms ?? tip.row.conv.started_at_ms ?? 0) - (tip.row.conv.started_at_ms ?? 0), 0))}
          </div>
        </div>
      )}
    </div>
  );
}
