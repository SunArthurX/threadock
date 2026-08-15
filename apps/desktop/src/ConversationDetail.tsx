// 会话详情组件（消息/时间线/事件/知识提取/导出）
import { Message, EventDto, Conversation, COLLAPSE_THRESHOLD, sourceLabel, formatTime, eventTypeLabel } from "./types";

interface Props {
  conv: Conversation;
  messages: Message[];
  events: EventDto[];
  completenessLabel: string;
  loading: boolean;
  exporting: boolean;
  timelineMode: boolean;
  highlightMsgId: string | null;
  collapsedMsgs: Set<string>;
  tags: string[];
  onToggleTimeline: () => void;
  onExport: (format: "markdown" | "json") => void;
  onExtractKnowledge: () => void;
  onToggleCollapse: (id: string) => void;
  onToggleFavorite: () => void;
  onToggleArchive: () => void;
  onAddTag: (tag: string) => void;
  onRemoveTag: (tag: string) => void;
  onRescanAudit: () => void;
}

import { useState } from "react";

export default function ConversationDetail({
  conv, messages, events, completenessLabel, loading, exporting,
  timelineMode, highlightMsgId, collapsedMsgs, tags,
  onToggleTimeline, onExport, onExtractKnowledge, onToggleCollapse,
  onToggleFavorite, onToggleArchive,
  onAddTag, onRemoveTag, onRescanAudit,
}: Props) {
  const [tagInput, setTagInput] = useState("");
  const [downloadOpen, setDownloadOpen] = useState(false);
  const renderContent = (m: Message) => {
    const text = m.content_text ?? "(空)";
    const isCollapsed = collapsedMsgs.has(m.id);
    const isLong = text.length > COLLAPSE_THRESHOLD;
    if (isLong && isCollapsed) {
      return (<>
        <div className="content">{text.slice(0, COLLAPSE_THRESHOLD)}…</div>
        <button className="collapse-btn" onClick={() => onToggleCollapse(m.id)}>
          展开剩余 {text.length - COLLAPSE_THRESHOLD} 字 ▾
        </button>
      </>);
    }
    return (<>
      <div className="content">{text}</div>
      {isLong && <button className="collapse-btn" onClick={() => onToggleCollapse(m.id)}>收起 ▴</button>}
    </>);
  };

  const renderTimeline = () => (
    <div className="conversation-timeline">
      {(() => {
        interface TI { kind: "msg" | "event"; ts: number; data: unknown }
        // 按时间归并排序（旧实现直接拼接不排序、事件无时间、截断 100）
        const items: TI[] = [
          ...messages.map((m) => ({ kind: "msg" as const, ts: m.created_at_ms ?? 0, data: m })),
          ...events.map((e) => ({ kind: "event" as const, ts: e.created_at_ms ?? 0, data: e })),
        ].sort((a, b) => a.ts - b.ts);
        return items.slice(0, 2000).map((item, i) => {
          if (item.kind === "msg") {
            const m = item.data as Message;
            return (
              <div key={i} className={`tl-item tl-${m.role}`}>
                <div className="tl-dot" />
                <div className="tl-time">{m.created_at_ms ? new Date(m.created_at_ms).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : ""}</div>
                <div className="tl-content">
                  <div className="tl-role">{m.role === "user" ? "👤 用户" : m.role === "assistant" ? "🤖 助手" : m.role}</div>
                  <div className="tl-text">{(m.content_text ?? "").slice(0, 200)}</div>
                </div>
              </div>
            );
          }
          const e = item.data as EventDto;
          return (
            <div key={i} className="tl-item tl-event">
              <div className="tl-dot tl-dot-event" />
              <div className="tl-time">{e.created_at_ms ? new Date(e.created_at_ms).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : ""}</div>
              <div className="tl-content">
                <div className="tl-role tl-role-event">⚡ {eventTypeLabel(e.event_type)}</div>
                {e.summary && <div className="tl-text mono" style={{ fontSize: 10 }}>{e.summary.slice(0, 80)}</div>}
              </div>
            </div>
          );
        });
      })()}
    </div>
  );

  return (
    <div className="detail">
      <h2>
        {conv.user_title ?? conv.title ?? "(无标题)"}
        {completenessLabel && <span className={`badge completeness ${completenessLabel}`}>{completenessLabel}</span>}
      </h2>
      <div className="meta-row">
        来源: {sourceLabel(conv.provider)} · 模型: {conv.model ?? "unknown"}
        {conv.completeness_score != null && ` · ${(conv.completeness_score * 100).toFixed(0)}%`}
        {conv.started_at_ms && ` · ${formatTime(conv.started_at_ms)}`}
        {conv.source_parent_id && " · 子任务"}
      </div>
      <div className="detail-actions">
        <button className={`action-btn ${timelineMode ? "active" : ""}`} onClick={onToggleTimeline}>
          {timelineMode ? "💬 消息" : "🕐 时间线"}
        </button>
        <button className="action-btn" onClick={onExtractKnowledge} disabled={loading || messages.length === 0}>
          {loading ? "提取中…" : "✨ 知识"}
        </button>
        <button className="action-btn" onClick={onRescanAudit} title="用审计规则扫描此会话（敏感信息 + 危险命令），结果以通知弹出">🔍 重扫</button>
        <button className="action-btn" onClick={onToggleFavorite}>{conv.favorite ? "★ 已收藏" : "☆ 收藏"}</button>
        <button className="action-btn" onClick={onToggleArchive}>{conv.archived ? "📤 取消归档" : "🗄 归档"}</button>
        <div className="download-dropdown">
          <button className="action-btn" disabled={exporting} onClick={() => setDownloadOpen(!downloadOpen)}>
            {exporting ? "导出中…" : "⤓ 下载 ▾"}
          </button>
          {downloadOpen && (
            <>
              <div className="import-backdrop" onClick={() => setDownloadOpen(false)} />
              <div className="import-menu download-menu">
                <button onClick={() => { setDownloadOpen(false); onExport("markdown"); }}>📄 Markdown（.md）</button>
                <button onClick={() => { setDownloadOpen(false); onExport("json"); }}>🧾 JSON（.json）</button>
              </div>
            </>
          )}
        </div>
      </div>
      {/* 标签行始终显示（含输入框） */}
      {(
        <div className="tag-row">
          {tags.map((t) => (
            <span key={t} className="tag-chip" title="点击移除标签" onClick={() => onRemoveTag(t)}>
              #{t} <span className="tag-x">✕</span>
            </span>
          ))}
          <input
            className="tag-input"
            value={tagInput}
            placeholder="+ 标签"
            onChange={(e) => setTagInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && tagInput.trim()) { onAddTag(tagInput.trim()); setTagInput(""); }
            }}
          />
        </div>
      )}
      {loading && <div className="panel-loading"><div className="spinner spinner-sm" /><span>加载对话内容…</span></div>}
      {timelineMode && !loading && renderTimeline()}
      {!timelineMode && messages.map((m) => (
        <div key={m.id} id={`msg-${m.id}`} className={`message ${m.role} ${highlightMsgId === m.id ? "highlighted" : ""}`}>
          <div className="role">
            <span className={`avatar ${m.role}`}>{m.role === "user" ? "U" : m.role === "assistant" ? "AI" : m.role[0]?.toUpperCase()}</span>
            <span className="role-label">{m.role === "user" ? "用户" : m.role === "assistant" ? "助手" : m.role}</span>
            {m.created_at_ms && <span className="msg-time">{formatTime(m.created_at_ms)}</span>}
          </div>
          {renderContent(m)}
        </div>
      ))}
      {events.length > 0 && (<>
        <div className="events-header">执行事件 ({events.length})</div>
        {events.map((e) => (
          <div key={e.id} className={`event ${e.event_type}`}>
            <span className="event-type">{eventTypeLabel(e.event_type)}</span>
            <span className="event-summary">{e.summary ?? ""}</span>
          </div>
        ))}
      </>)}
    </div>
  );


}
