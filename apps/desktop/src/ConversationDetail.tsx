// 会话详情组件（消息/时间线/事件/知识提取/导出/内搜索/复制）
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Message, EventDto, Conversation, COLLAPSE_THRESHOLD, sourceLabel, formatTime, eventTypeLabel } from "./types";
import { showToast } from "./toast";
import { splitCodeBlocks } from "./messageRender";
import { highlightCode } from "./codeHighlight.tsx";

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
  /** 父级滚动容器的 ref（用于「滚到底部」按钮 + 滚动检测） */
  scrollContainerRef?: React.RefObject<{ inner: HTMLElement | null } | HTMLElement | null>;
  onToggleTimeline: () => void;
  onExport: (format: "markdown" | "json") => void;
  onExtractKnowledge: () => void;
  onToggleCollapse: (id: string) => void;
  onAddTag: (tag: string) => void;
  onRemoveTag: (tag: string) => void;
  onRescanAudit: () => void;
  /** 改写 user_title（空串/null 表示清除）。 */
  onRenameTitle?: (title: string | null) => Promise<void> | void;
  /** 私有笔记（仅个人，不参与搜索/导出/统计） */
  note?: string | null;
  /** 保存/清除私有笔记。空串/null 表示删除。 */
  onNoteChange?: (note: string | null) => Promise<void> | void;
  /** 全部标签（按使用频次倒序），供输入时自动补全 */
  allTags?: { tag: string; count: number }[];
}

export default function ConversationDetail({
  conv, messages, events, completenessLabel, loading, exporting,
  timelineMode, highlightMsgId, collapsedMsgs, tags,
  scrollContainerRef, onToggleTimeline, onExport, onExtractKnowledge, onToggleCollapse,
  onAddTag, onRemoveTag, onRescanAudit, onRenameTitle,
  note, onNoteChange, allTags,
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

  // inline 标题编辑：双击标题进入 input；Enter 保存 / Esc 取消 / 失焦保存
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState(conv.user_title ?? "");

  // 标签自动补全：取已存在标签按子串匹配 + 排除当前会话已有的
  const [showSuggest, setShowSuggest] = useState(false);
  const [sugIdx, setSugIdx] = useState(0);
  const suggests = useMemo(() => {
    const list = allTags ?? [];
    const kw = tagInput.trim().toLowerCase();
    const tagSet = new Set(tags);
    const filtered = list.filter((s) => !tagSet.has(s.tag) && (kw === "" || s.tag.toLowerCase().includes(kw)));
    return filtered.slice(0, 8);
  }, [allTags, tags, tagInput]);
  useEffect(() => { setSugIdx(0); }, [tagInput]);

  // 「滚到底部」浮动按钮：用户向上滚超过 200px 时显示
  const [showJumpBottom, setShowJumpBottom] = useState(false);
  useEffect(() => {
    const el = (scrollContainerRef?.current as any)?.inner ?? scrollContainerRef?.current;
    if (!el) return;
    const onScroll = () => {
      const dist = el.scrollHeight - el.clientHeight - el.scrollTop;
      setShowJumpBottom(dist > 200);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => el.removeEventListener("scroll", onScroll);
  }, [scrollContainerRef, conv.id, timelineMode, messages.length]);
  const jumpToBottom = () => {
    const el = (scrollContainerRef?.current as any)?.inner ?? scrollContainerRef?.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  };

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
  /** 切分消息文本为「代码块 + 普通文本」段 —— 委托给 messageRender.splitCodeBlocks（独立可测）。 */

/** 渲染消息内容：高亮搜索关键词 + 复制按钮 + 长消息折叠 + ```代码块``` 渲染。 */
  const renderContent = (m: Message, idx: number) => {
    const text = m.content_text ?? "(空)";
    const isCollapsed = collapsedMsgs.has(m.id);
    const isLong = text.length > COLLAPSE_THRESHOLD;
    const isCurrentMatch = currentMatch === idx && search.trim();
    // 高亮匹配片段（不区分大小写，保留原大小写）
    const lower = search.trim().toLowerCase();
    const highlight = (s: string, keyPrefix: string): ReactNode => {
      if (!lower) return s;
      const out: ReactNode[] = [];
      const lowerS = s.toLowerCase();
      let i = 0;
      while (i < s.length) {
        const hit = lowerS.indexOf(lower, i);
        if (hit < 0) { out.push(s.slice(i)); break; }
        if (hit > i) out.push(s.slice(i, hit));
        out.push(<mark key={`${keyPrefix}-${hit}`} className="msg-search-hit">{s.slice(hit, hit + lower.length)}</mark>);
        i = hit + lower.length;
      }
      return out.length === 1 && typeof out[0] === "string" ? out[0] : <>{out}</>;
    };
    const segs = splitCodeBlocks(text);
    return (
      <>
        <div className="content">
          {segs.map((seg, si) =>
            seg.kind === "code" ? (
              <div key={`cb-${si}`} className="msg-code-block">
                <div className="msg-code-head">
                  <span className="msg-code-lang">{seg.lang || "text"}</span>
                  <button
                    className="msg-action-btn"
                    onClick={() => copyMessage(seg.content)}
                    title="复制代码块"
                  >📋</button>
                </div>
                <pre className="msg-code-pre"><code>{highlightCode(seg.content, seg.lang)}</code></pre>
              </div>
            ) : (
              <span key={`tx-${si}`}>{highlight(seg.content, `tx-${si}`)}</span>
            ),
          )}
        </div>
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

  const submitTitle = async () => {
    if (!onRenameTitle) { setEditingTitle(false); return; }
    const next = titleDraft.trim();
    const prev = conv.user_title?.trim() ?? "";
    if (next === prev) { setEditingTitle(false); return; }
    try {
      await onRenameTitle(next ? next : null);
    } catch { /* 失败保持草稿 */ }
    setEditingTitle(false);
  };

  return (
    <div className="detail">
      <h2
        className={editingTitle ? "editing" : ""}
        onDoubleClick={() => { if (onRenameTitle) { setTitleDraft(conv.user_title ?? ""); setEditingTitle(true); } }}
        title={onRenameTitle ? "双击改标题（自定义后展示你的标题）" : undefined}
      >
        {editingTitle ? (
          <input
            className="title-input"
            value={titleDraft}
            autoFocus
            placeholder="自定义标题（空 = 恢复原始）"
            onChange={(e) => setTitleDraft(e.target.value)}
            onBlur={submitTitle}
            onKeyDown={(e) => {
              if (e.key === "Enter") submitTitle();
              if (e.key === "Escape") { setTitleDraft(conv.user_title ?? ""); setEditingTitle(false); }
            }}
          />
        ) : (
          <span className="title-text">
            {conv.user_title ?? conv.title ?? "(无标题)"}
            {onRenameTitle && <span className="title-edit-hint" title="双击改标题">✎</span>}
          </span>
        )}
        {completenessLabel && <span className={`badge completeness ${completenessLabel}`}>{completenessLabel}</span>}
      </h2>
      <div className="meta-row">
        来源: {sourceLabel(conv.provider)} · 模型: {conv.model ?? "unknown"}
        {conv.completeness_score != null && ` · ${(conv.completeness_score * 100).toFixed(0)}%`}
        {conv.started_at_ms && ` · ${formatTime(conv.started_at_ms)}`}
        {conv.source_parent_id && " · 子任务"}
      </div>
      <div className="detail-actions">
        {/* 收藏 / 归档 已移至右键菜单（避免顶栏拥挤，参考 macOS 设计） */}
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
      {/* 标签行始终显示（含输入框 + 自动补全） */}
      {(
        <div className="tag-row">
          {tags.map((t) => (
            <span key={t} className="tag-chip" title="点击移除标签" onClick={() => onRemoveTag(t)}>
              #{t} <span className="tag-x">✕</span>
            </span>
          ))}
          <div className="tag-input-wrap">
            <input
              className="tag-input"
              value={tagInput}
              placeholder="+ 标签"
              onChange={(e) => setTagInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && tagInput.trim()) { onAddTag(tagInput.trim()); setTagInput(""); setShowSuggest(false); }
                if (e.key === "Escape") setShowSuggest(false);
                if (e.key === "ArrowDown" && suggests.length > 0) { e.preventDefault(); setSugIdx((i) => Math.min(i + 1, suggests.length - 1)); }
                if (e.key === "ArrowUp" && suggests.length > 0) { e.preventDefault(); setSugIdx((i) => Math.max(i - 1, 0)); }
              }}
              onFocus={() => setShowSuggest(true)}
              onBlur={() => window.setTimeout(() => setShowSuggest(false), 150)}
            />
            {showSuggest && suggests.length > 0 && (
              <div className="tag-suggest" onMouseDown={(e) => e.preventDefault()}>
                {suggests.map((s, i) => (
                  <button
                    key={s.tag}
                    className={`tag-suggest-item ${i === sugIdx ? "active" : ""}`}
                    onClick={() => { onAddTag(s.tag); setTagInput(""); setShowSuggest(false); }}
                  >
                    <span>#{s.tag}</span>
                    <span className="tag-suggest-count">{s.count}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
      {/* 私有笔记（不参与搜索/导出/统计；折叠默认开，保存自动） */}
      {onNoteChange && (
        <PrivateNoteSection note={note ?? ""} onChange={onNoteChange} />
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
      {showJumpBottom && (
        <button className="jump-bottom-btn" onClick={jumpToBottom} title="滚到底部">↓</button>
      )}
    </div>
  );
}

/** 私有笔记 section：折叠默认开，autosave on blur（不参与搜索/导出/统计）。 */
function PrivateNoteSection({ note, onChange }: { note: string; onChange: (n: string | null) => void }) {
  const [text, setText] = useState(note);
  // 受控但允许本地编辑（保存前不写回父级，autosave 触发）
  useEffect(() => { setText(note); }, [note]);
  const [saved, setSaved] = useState<"idle" | "saving" | "saved">("idle");
  const save = async (next: string) => {
    const trimmed = next.trim();
    if (trimmed === (note ?? "").trim()) { setSaved("idle"); return; }
    setSaved("saving");
    try { await onChange(trimmed || null); setSaved("saved"); window.setTimeout(() => setSaved("idle"), 1500); }
    catch { setSaved("idle"); }
  };
  const placeholder = "📝 私人笔记（不参与搜索/导出/统计）";
  return (
    <details className="private-note" open={!!note}>
      <summary>
        📝 私人笔记 {saved === "saving" && <span className="private-note-status">保存中…</span>}
        {saved === "saved" && <span className="private-note-status saved">✓ 已保存</span>}
      </summary>
      <textarea
        className="private-note-text"
        value={text}
        placeholder={placeholder}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => save(e.target.value)}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            save(text);
            (e.target as HTMLTextAreaElement).blur();
          }
        }}
        rows={3}
      />
      <div className="private-note-hint">
        ⌘+Enter 保存 · 失焦自动保存 · 清空内容后失焦 = 删除笔记
      </div>
    </details>
  );
}
