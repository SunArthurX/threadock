// 会话详情组件（消息/时间线/事件/知识提取/导出/内搜索/复制/原始视图/来源应用）
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Message, EventDto, Conversation, sourceLabel, formatTime, eventTypeLabel } from "./types";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";
import { copyToClipboard } from "./clipboard";
import MessageBlock from "./MessageBlock";
import PrivateNoteSection from "./PrivateNoteSection";

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

  // 「滚到底部」浮动按钮：用户向上滚超过 200px 时显示
  const [showJumpBottom, setShowJumpBottom] = useState(false);

  // ── 原始视图 / 来源应用 / 恢复命令（plan P2-3，v1.0.0）──────────────
  const [rawView, setRawView] = useState(false);
  const [rawContent, setRawContent] = useState<string | null>(null);
  // 切换会话时退出原始视图（受控状态随 conv.id 重置）
  // eslint-disable-next-line react-hooks/set-state-in-effect -- conv.id 变化时同步重置派生 UI 状态，属 prop 同步模式
  useEffect(() => { setRawView(false); setRawContent(null); }, [conv.id]);
  const toggleRawView = async () => {
    if (rawView) { setRawView(false); return; }
    try {
      const raw = await invoke<string | null>("conversation_raw", { conversationId: conv.id });
      setRawContent(raw ?? null);
      setRawView(true);
    } catch (e) { showToast(typeof e === "string" ? e : String(e), "error"); }
  };
  const openSourceApp = async () => {
    try {
      const msg = await invoke<string>("open_source_app", { provider: conv.provider });
      showToast(msg, "info");
    } catch (e) { showToast(typeof e === "string" ? e : String(e), "error"); }
  };
  const copyResumeCommand = async () => {
    try {
      const cmd = await invoke<string | null>("resume_command", { conversationId: conv.id });
      if (!cmd) { showToast("该来源不支持恢复命令（仅 claude-code / codex CLI 支持）", "info"); return; }
      const r = await copyToClipboard(cmd);
      if (r.ok) showToast(`✓ 已复制：${cmd}`, "info");
      else showToast(r.error ?? "复制失败", "error");
    } catch (e) { showToast(typeof e === "string" ? e : String(e), "error"); }
  };

  /** 解析父级滚动容器：ScrollArea 的 ref 可能是 { inner } 包装，也可能是原生 HTMLElement。 */
  const scrollEl = useCallback((): HTMLElement | null => {
    const cur = scrollContainerRef?.current ?? null;
    if (!cur) return null;
    return "inner" in cur ? cur.inner : cur;
  }, [scrollContainerRef]);
  useEffect(() => {
    const el = scrollEl();
    if (!el) return;
    const onScroll = () => {
      const dist = el.scrollHeight - el.clientHeight - el.scrollTop;
      setShowJumpBottom(dist > 200);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => el.removeEventListener("scroll", onScroll);
  }, [scrollEl, scrollContainerRef, conv.id, timelineMode, messages.length]);
  const jumpToBottom = () => {
    const el = scrollEl();
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
        setSearchIdx(0);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [searchOpen]);

  // 计算匹配：返回在 messages 中（按 onlyUser 过滤后）的索引列表
  const visibleMsgs = onlyUser ? messages.filter((m) => m.role === "user") : messages;
  // 时间线合并数组（消息 + 事件，按时间排序，截断 2000）—— 用于时间线模式搜索
  const timelineItems = useMemo<Array<{ kind: "msg" | "event"; id: string; ts: number }>>(() => {
    return [
      ...visibleMsgs.map((m) => ({ kind: "msg" as const, id: m.id, ts: m.created_at_ms ?? 0 })),
      ...events.map((e) => ({ kind: "event" as const, id: e.id, ts: e.created_at_ms ?? 0 })),
    ].sort((a, b) => a.ts - b.ts).slice(0, 2000);
  }, [visibleMsgs, events]);
  // 匹配描述：消息模式下用 msgId；时间线模式下用 (kind,id) 二元组
  type Match = { kind: "msg" | "event"; id: string; msgIdx?: number; tlIdx?: number };
  const matches = useMemo<Match[]>(() => {
    if (!search.trim()) return [];
    const lower = search.toLowerCase();
    if (timelineMode) {
      return timelineItems
        .map((it, i): Match | null => {
          if (it.kind === "msg") {
            const m = messages.find((mm) => mm.id === it.id);
            const text = m?.content_text ?? "";
            return text.toLowerCase().includes(lower) ? { kind: "msg", id: it.id, tlIdx: i } : null;
          }
          const e = events.find((ee) => ee.id === it.id);
          const text = e?.summary ?? "";
          return text.toLowerCase().includes(lower) ? { kind: "event", id: it.id, tlIdx: i } : null;
        })
        .filter((m): m is Match => m !== null);
    }
    return visibleMsgs
      .map((m, i): Match | null => (m.content_text ?? "").toLowerCase().includes(lower) ? { kind: "msg", id: m.id, msgIdx: i } : null)
      .filter((m): m is Match => m !== null);
  }, [visibleMsgs, events, timelineItems, search, timelineMode, messages]);
  const currentMatch = matches[searchIdx];

  const scrollToMessage = (match: typeof currentMatch) => {
    if (!match) return;
    const sel = timelineMode
      ? (match.kind === "msg" ? `#tl-msg-${match.id}` : `#tl-event-${match.id}`)
      : `#msg-${match.id}`;
    const el = document.querySelector(sel);
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
    const r = await copyToClipboard(text);
    if (r.ok) showToast("✓ 消息已复制", "info");
    else showToast(`剪贴板不可用：${r.error ?? "unknown"}`, "error", 6000);
  };
  /** 复制 message_id（排错用：粘到 issue 里能直接定位 DB 行）。 */
  const copyMsgId = async (id: string) => {
    const r = await copyToClipboard(id);
    if (r.ok) showToast(`✓ message_id 已复制 (${id.slice(0, 12)}…)`, "info");
    else showToast(`剪贴板不可用：${r.error ?? "unknown"}`, "error", 6000);
  };
  /** 复制整条会话的纯文本（user + assistant 顺序拼接，无 metadata）。 */
  const copyAllMessages = async () => {
    const lines = visibleMsgs.map((m) => {
      const role = m.role === "user" ? "我" : m.role === "assistant" ? "AI" : m.role;
      const ts = m.created_at_ms ? new Date(m.created_at_ms).toLocaleString("zh-CN") : "";
      return `[${ts}] ${role}:\n${m.content_text ?? ""}`;
    });
    const text = lines.join("\n\n");
    const r = await copyToClipboard(text);
    if (r.ok) showToast(`✓ 已复制 ${lines.length} 条消息`, "info");
    else showToast(`剪贴板不可用：${r.error ?? "unknown"}`, "error", 6000);
  };
  /** 切分消息文本为「代码块 + 普通文本」段 —— 委托给 messageRender.splitCodeBlocks（独立可测）。 */


  const renderTimeline = () => (
    <div className="conversation-timeline">
      {timelineItems.map((item) => {
        const isCurrent = !!search.trim() && currentMatch?.kind === item.kind && currentMatch?.id === item.id;
        if (item.kind === "msg") {
          const m = messages.find((mm) => mm.id === item.id);
          if (!m) return null;
          return (
            <div
              key={`m-${m.id}`}
              id={`tl-msg-${m.id}`}
              className={`tl-item tl-${m.role} ${isCurrent ? "current-match" : ""}`}
            >
              <div className="tl-dot" />
              <div className="tl-time">{m.created_at_ms ? new Date(m.created_at_ms).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : ""}</div>
              <div className="tl-content">
                <div className="tl-role">{m.role === "user" ? "👤 用户" : m.role === "assistant" ? "🤖 助手" : m.role}</div>
                <div className="tl-text">{(m.content_text ?? "").slice(0, 200)}</div>
                {isCurrent && <span className="msg-match-marker" aria-hidden>🎯</span>}
              </div>
            </div>
          );
        }
        const e = events.find((ee) => ee.id === item.id);
        if (!e) return null;
        return (
          <div
            key={`e-${e.id}`}
            id={`tl-event-${e.id}`}
            className={`tl-item tl-event ${isCurrent ? "current-match" : ""}`}
          >
            <div className="tl-dot tl-dot-event" />
            <div className="tl-time">{e.created_at_ms ? new Date(e.created_at_ms).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : ""}</div>
            <div className="tl-content">
              <div className="tl-role tl-role-event">⚡ {eventTypeLabel(e.event_type)}</div>
              {e.summary && <div className="tl-text mono" style={{ fontSize: 10 }}>{e.summary.slice(0, 80)}</div>}
              {isCurrent && <span className="msg-match-marker" aria-hidden>🎯</span>}
            </div>
          </div>
        );
      })}
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
        <button
          className={`action-btn ${rawView ? "active" : ""}`}
          onClick={toggleRawView}
          title="切换原始视图：显示 Raw Store 里的未标准化原始归档（plan P2-3）"
        >
          {rawView ? "🔤 统一视图" : "🗂 原始视图"}
        </button>
        <button className="action-btn" onClick={openSourceApp} title="打开该会话的来源应用（Cursor / ZCode / MiniMax Code）">
          ↗ 来源应用
        </button>
        <button className="action-btn" onClick={copyResumeCommand} title="复制「恢复原会话」命令（claude-code / codex CLI 来源支持）">
          ⏯ 恢复命令
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
            onChange={(e) => { setSearch(e.target.value); setSearchIdx(0); }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                if (e.shiftKey) prevMatch();
                else nextMatch();
              }
            }}
          />
          <span className="msg-search-count">
            {search.trim() ? (matches.length === 0 ? "无匹配" : `${searchIdx + 1} / ${matches.length}`) : ""}
          </span>
          <button className="msg-search-btn" onClick={prevMatch} disabled={matches.length === 0}>↑</button>
          <button className="msg-search-btn" onClick={nextMatch} disabled={matches.length === 0}>↓</button>
          <button className="msg-search-btn" onClick={() => { setSearchOpen(false); setSearch(""); setSearchIdx(0); }} title="关闭（Esc）">✕</button>
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
              onChange={(e) => { setTagInput(e.target.value); setSugIdx(0); }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && tagInput.trim()) { onAddTag(tagInput.trim()); setTagInput(""); setSugIdx(0); setShowSuggest(false); }
                if (e.key === "Escape") setShowSuggest(false);
                if (e.key === "ArrowDown" && suggests.length > 0) { e.preventDefault(); setSugIdx((i) => Math.min(i + 1, suggests.length - 1)); }
                if (e.key === "ArrowUp" && suggests.length > 0) { e.preventDefault(); setSugIdx((i) => Math.max(i - 1, 0)); }
              }}
              onFocus={() => setShowSuggest(true)}
              onBlur={() => window.setTimeout(() => setShowSuggest(false), 150)}
            />
            {showSuggest && suggests.length > 0 && (
              <ScrollArea className="tag-suggest" onMouseDown={(e: React.MouseEvent<HTMLDivElement>) => e.preventDefault()}>
                {suggests.map((s, i) => (
                  <button
                    key={s.tag}
                    className={`tag-suggest-item ${i === sugIdx ? "active" : ""}`}
                    onClick={() => { onAddTag(s.tag); setTagInput(""); setSugIdx(0); setShowSuggest(false); }}
                  >
                    <span>#{s.tag}</span>
                    <span className="tag-suggest-count">{s.count}</span>
                  </button>
                ))}
              </ScrollArea>
            )}
          </div>
        </div>
      )}
      {/* 私有笔记（不参与搜索/导出/统计；折叠默认开，保存自动） */}
      {onNoteChange && (
        <PrivateNoteSection key={conv.id} note={note ?? ""} onChange={onNoteChange} />
      )}
      {loading && <div className="panel-loading"><div className="spinner spinner-sm" /><span>加载对话内容…</span></div>}
      {/* 原始视图（plan P2-3）：Raw Store 未标准化归档，只读展示 */}
      {rawView && !loading && (
        rawContent === null
          ? <div className="empty">该会话没有原始归档（直读导入的来源不落 Raw Store）</div>
          : <pre className="raw-payload-view">{rawContent}</pre>
      )}
      {!rawView && timelineMode && !loading && renderTimeline()}
      {!rawView && !timelineMode && visibleMsgs.map((m) => {
        const isMatch = !!search.trim() && currentMatch?.kind === "msg" && currentMatch?.id === m.id;
        return (
        <div key={m.id} id={`msg-${m.id}`} className={`message ${m.role} ${highlightMsgId === m.id ? "highlighted" : ""} ${isMatch ? "current-match" : ""}`}>
          <div className="role">
            <span className={`avatar ${m.role}`}>{m.role === "user" ? "U" : m.role === "assistant" ? "AI" : m.role[0]?.toUpperCase()}</span>
            <span className="role-label">{m.role === "user" ? "用户" : m.role === "assistant" ? "助手" : m.role}</span>
            {m.created_at_ms && <span className="msg-time">{formatTime(m.created_at_ms)}</span>}
          </div>
          <MessageBlock
            message={m}
            isMatch={isMatch}
            searchQuery={search}
            isCollapsed={collapsedMsgs.has(m.id)}
            onToggleCollapse={onToggleCollapse}
            onCopyMessage={copyMessage}
            onCopyMsgId={copyMsgId}
          />
        </div>
        );
      })}
      {!rawView && events.length > 0 && (<>
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
