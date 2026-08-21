// 消息名下的执行事件：紧凑行挂在消息气泡下，点击行展开详情
//（完整摘要 / 状态 / 起止时间与耗时 / payload JSON）。超过 4 条折叠，
// 「还有 N 条」展开——避免长工程会话一条消息挂几十个工具调用撑爆 DOM。
import { useState } from "react";
import { eventTypeLabel, formatTime } from "./types";
import type { EventDto } from "./types";
import { EVENT_ROWS_COLLAPSED } from "./eventGrouping";

/** 事件行前的图标（与 eventTypeLabel 的 19 类对应常见子集，其余统一 ⚙）。 */
const EVENT_ICONS: Record<string, string> = {
  command_started: "▶", command_completed: "✔", command_failed: "✖",
  tool_call_started: "🔧", tool_call_completed: "✅",
  file_read: "📄", file_created: "🆕", file_updated: "✏️", file_deleted: "🗑",
  diff_generated: "🔀", approval_requested: "❓", approval_granted: "👍", approval_denied: "🚫",
  error: "❌", artifact_created: "📦",
};

function durationOf(e: EventDto): string | null {
  if (e.created_at_ms == null || e.completed_at_ms == null) return null;
  const ms = e.completed_at_ms - e.created_at_ms;
  if (ms < 0) return null;
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) {
    const s = ms / 1000;
    return s % 1 === 0 ? `${s}s` : `${s.toFixed(1)}s`;
  }
  return `${Math.floor(ms / 60_000)}m${Math.round((ms % 60_000) / 1000)}s`;
}

/** payload JSON 字符串 → 美化文本（解析失败按原文展示）。 */
function prettyPayload(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

function EventDetail({ e }: { e: EventDto }) {
  const dur = durationOf(e);
  return (
    <div className="msg-event-detail">
      {e.summary && <div className="msg-event-detail-summary mono">{e.summary}</div>}
      <div className="msg-event-detail-meta">
        <span>类型 {e.event_type}</span>
        {e.status && <span>状态 {e.status}</span>}
        {e.sequence_number != null && <span>序号 #{e.sequence_number}</span>}
        {e.created_at_ms && <span>{formatTime(e.created_at_ms)}</span>}
        {dur && <span>耗时 {dur}</span>}
      </div>
      {e.payload_json && (
        <pre className="msg-event-detail-payload mono">{prettyPayload(e.payload_json)}</pre>
      )}
    </div>
  );
}

function EventRow({ e }: { e: EventDto }) {
  const [open, setOpen] = useState(false);
  const summary = e.summary ?? "";
  return (
    <div className={`msg-event-row-wrap ${open ? "open" : ""}`}>
      <button
        className={`msg-event-row ${e.event_type === "error" ? "is-error" : ""}`}
        onClick={() => setOpen((o) => !o)}
        title={open ? "收起详情" : "展开详情"}
      >
        <span className="msg-event-icon">{EVENT_ICONS[e.event_type] ?? "⚙"}</span>
        <span className="msg-event-type">{eventTypeLabel(e.event_type)}</span>
        {summary && (
          <span className="msg-event-summary mono" title={summary}>
            {summary.length > 90 ? `${summary.slice(0, 90)}…` : summary}
          </span>
        )}
        <span className="msg-event-caret">{open ? "▾" : "▸"}</span>
      </button>
      {open && <EventDetail e={e} />}
    </div>
  );
}

/** 消息名下事件组。`label` 用于孤儿组（会话前置事件）标题。 */
export default function MessageEvents({ events, label }: { events: EventDto[]; label?: string }) {
  const [expanded, setExpanded] = useState(false);
  if (events.length === 0) return null;
  const shown = expanded ? events : events.slice(0, EVENT_ROWS_COLLAPSED);
  const hidden = events.length - shown.length;
  return (
    <div className="msg-events">
      {label && <div className="msg-events-label">{label}</div>}
      {shown.map((e) => (
        <EventRow key={e.id} e={e} />
      ))}
      {hidden > 0 && (
        <button className="msg-event-more" onClick={() => setExpanded(true)}>
          还有 {hidden} 条 ▾
        </button>
      )}
      {expanded && events.length > EVENT_ROWS_COLLAPSED && (
        <button className="msg-event-more" onClick={() => setExpanded(false)}>收起 ▴</button>
      )}
    </div>
  );
}
