// 会话详情组件（消息/时间线/事件/知识提取/导出/内搜索/复制）
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Message, EventDto, Conversation, COLLAPSE_THRESHOLD, sourceLabel, formatTime, eventTypeLabel } from "./types";
import { showToast } from "./toast";

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

export default function ConversationDetail({
  conv, messages, events, completenessLabel, loading, exporting,
  timelineMode, highlightMsgId, collapsedMsgs, tags,
  onToggleTimeline, onExport, onExtractKnowledge, onToggleCollapse,
  onToggleFavorite, onToggleArchive,
  onAddTag, onRemoveTag, onRescanAudit,
}: Props) {
  const [tagInput, setTagInput] = useState("");
  const [downloadOpen, setDownloadOpen] = useState(false);
  /** 只看用户消息（我的提问）：消息视图与时间线同时生效。 */
  const [onlyUser, setOnlyUser] = useState(false);
  /** 消息内搜索（⌘F 唤起）：实时高亮 + 跳到第 N 个匹配。 */
  const [search, setSearch] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchIdx, setSearchIdx] = useState(0);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // ⌘F / Ctrl+F 唤起消息内搜索
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        // 顶层 ⌘K 面板是跳转 / 搜索会话；详情页的 ⌘F 是消息内搜索
        e.preventDefault();
        setSearchOpen(true);
        setTimeout(() => searchInputRef.current?.focus(), 30);
        setTimeout(() => searchInputRef.current?.select(), 30);
      } else if (e.key === "Escape" && searchOpen) {
        setSearchOpen(false);
        setSearch("");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [searchOpen]);

  // 计算匹配：返回在 messages 中（按 onlyUser 过滤后）的索引列表
  const visibleMsgs = onlyUser ? messages.filter((m) => m.role === "user") : messages;
  const matches = useMemo(() => {
    if (!search.trim()) return [] as number[];
    const lower = search.toLowerCase();
    return visibleMsgs
      .map((m, i) => (m.content_text ?? "").toLowerCase().includes(lower) ? i : -1)
      .filter((i) => i >= 0);
  }, [visibleMsgs, search]);
  // 切换 search / matches 时回正 idx
  useEffect(() => { setSearchIdx(0); }, [search, matches.length]);
  const currentMatch = matches[searchIdx] ?? -1;

  const scrollToMessage = (idx: number) => {
    const el = document.getElementById(`msg-${visibleMsgs[idx]?.id}`);
    if (el) el.scrollIntoView({ behavior: "smooth", block: "center" });
  };
  const nextMatch = () => {
    if (matches.length === 0) return;
    const next = (searchIdx + 1) % matches.length;
    setSearchIdx(next);
    scrollToMessage(matches[next]);
  };
  const prevMatch = () => {
    if (matches.length === 0) return;
    const prev = (searchIdx - 1 + matches.length) % matches.length;
    setSearchIdx(prev);
    scrollToMessage(matches[prev]);
  };

  /** 复制消息文本。 */
  const copyMessage = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      showToast("✓ 消息已复制", "info");
    } catch { showToast("剪贴板不可用", "error"); }
  };
  /** 复制 message_id（排错用：粘到 issue 里能直接定位 DB 行）。 */
  const copyMsgId = async (id: string) => {
    try {
      await navigator.clipboard.writeText(id);
      showToast(`✓ message_id 已复制 (${id.slice(0, 12)}…)`, "info");
    } catch { showToast("剪贴板不可用", "error"); }
  };
  /** 复制整条会话的纯文本（user + assistant 顺序拼接，无 metadata）。 */
  const copyAllMessages = async () => {
    const lines = visibleMsgs.map((m) => {
      const role = m.role === "user" ? "我" : m.role === "assistant" ? "AI" : m.role;
      const ts = m.created_at_ms ? new Date(m.created_at_ms).toLocaleString("zh-CN") : "";
      return `[${ts}] ${role}:\n${m.content_text ?? ""}`;
    });
    try {
      await navigator.clipboard.writeText(lines.join("\n\n"));
      showToast(`✓ 已复制 ${lines.length} 条消息`, "info");
    } catch { showToast("剪贴板不可用", "error"); }
  };
  /** 渲染消息内容：高亮搜索关键词 + 复制按钮 + 长消息折叠。 */
  const renderContent = (m: Message, idx: number) => {
    const text = m.content_text ?? "(空)";
    const isCollapsed = collapsedMsgs.has(m.id);
    const isLong = text.length > COLLAPSE_THRESHOLD;
    const isCurrentMatch = currentMatch === idx && search.trim();
    // 高亮匹配片段（不区分大小写，保留原大小写）
    const lower = search.trim().toLowerCase();
    const nodes: ReactNode[] = [];
    if (lower) {
      let i = 0;
      const lowerText = text.toLowerCase();
      while (i < text.length) {
        const hit = lowerText.indexOf(lower, i);
        if (hit < 0) { nodes.push(text.slice(i)); break; }
        if (hit > i) nodes.push(text.slice(i, hit));
        nodes.push(<mark key={`hl-${hit}`} className="msg-search-hit">{text.slice(hit, hit + lower.length)}</mark>);
        i = hit + lower.length;
      }
    } else {
      nodes.push(text);
    }
    return (
      <>
        <div className="content">{nodes}</div>
        <div className="msg-actions">
          {isLong && (
            <button className="msg-action-btn" onClick={() => onToggleCollapse(m.id)}>
              {isCollapsed ? `展开剩余 ${text.length - COLLAPSE_THRESHOLD} 字 ▾` : "收起 ▴"}
            </button>
          )}
          <button className="msg-action-btn" onClick={() => copyMessage(text)} title="复制本条消息">📋</button>
          <button className="msg-action-btn" onClick={() => copyMsgId(m.id)} title="复制 message_id（排错）">🆔</button>
        </div>
        {isCurrentMatch && <div className="msg-match-marker" aria-hidden>🎯</div>}
      </>
    );
  };

  const renderTimeline = () => (
    <div className="conversation-timeline">
      {(() => {
        interface TI { kind: "msg" | "event"; ts: number; data: unknown }
        // 按时间归并排序（旧实现直接拼接不排序、事件无时间、截断 100）
        const visibleMsgs = onlyUser ? messages.filter((m) => m.role === "user") : messages;
        const items: TI[] = [
          ...visibleMsgs.map((m) => ({ kind: "msg" as const, ts: m.created_at_ms ?? 0, data: m })),
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
        <button className="action-btn" onClick={onToggleFavorite}>{conv.favorite ? "★ 已收藏" : "☆ 收藏"}</button>
        <button className={`action-btn ${timelineMode ? "active" : ""}`} onClick={onToggleTimeline}>
          {timelineMode ? "💬 消息" : "🕐 时间线"}
        </button>
        <button className="action-btn" onClick={onExtractKnowledge} disabled={loading || messages.length === 0}>
          {loading ? "提取中…" : "✨ 知识"}
        </button>
        <button className="action-btn" onClick={onRescanAudit} title="用审计规则扫描此会话（敏感信息 + 危险命令），结果以通知弹出">🔍 重扫</button>
        <button
          className={`action-btn ${onlyUser ? "active" : ""}`}
          onClick={() => setOnlyUser(!onlyUser)}
          title="开启后仅展示我自己发出的消息（消息视图与时间线同时生效）"
        >
          👤 仅用户消息
        </button>
        <button className="action-btn" onClick={onToggleArchive}>{conv.archived ? "📤 取消归档" : "🗄 归档"}</button>
        <button
          className="action-btn"
          onClick={() => setSearchOpen((v) => !v)}
          title="在此会话内搜索消息（⌘F）"
        >🔍 搜索消息</button>
        <button className="action-btn" onClick={copyAllMessages} title={`复制 ${visibleMsgs.length} 条消息为纯文本`}>
          📋 复制全部
        </button>
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

      {searchOpen && (
        <div className="msg-search-bar">
          <input
            ref={searchInputRef}
            className="msg-search-input"
            value={search}
            placeholder="在消息中搜索关键词…"
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") { e.preventDefault(); e.shiftKey ? prevMatch() : nextMatch(); }
            }}
          />
          <span className="msg-search-count">
            {search.trim() ? (matches.length === 0 ? "无匹配" : `${searchIdx + 1} / ${matches.length}`) : ""}
          </span>
          <button className="msg-search-btn" onClick={prevMatch} disabled={matches.length === 0}>↑</button>
          <button className="msg-search-btn" onClick={nextMatch} disabled={matches.length === 0}>↓</button>
          <button className="msg-search-btn" onClick={() => { setSearchOpen(false); setSearch(""); }} title="关闭（Esc）">✕</button>
        </div>
      )}
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
      {!timelineMode && visibleMsgs.map((m, idx) => (
        <div key={m.id} id={`msg-${m.id}`} className={`message ${m.role} ${highlightMsgId === m.id ? "highlighted" : ""} ${currentMatch === idx ? "current-match" : ""}`}>
          <div className="role">
            <span className={`avatar ${m.role}`}>{m.role === "user" ? "U" : m.role === "assistant" ? "AI" : m.role[0]?.toUpperCase()}</span>
            <span className="role-label">{m.role === "user" ? "用户" : m.role === "assistant" ? "助手" : m.role}</span>
            {m.created_at_ms && <span className="msg-time">{formatTime(m.created_at_ms)}</span>}
          </div>
          {renderContent(m, idx)}
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
