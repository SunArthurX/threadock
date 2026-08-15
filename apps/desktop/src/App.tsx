// App 主组件：布局 + 状态管理 + 导航（组件已拆分到独立文件）
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import OpsView from "./OpsView";
import SourcePanel from "./SourcePanel";
import ConversationList from "./ConversationList";
import ConversationDetail from "./ConversationDetail";
import SearchPanel from "./SearchPanel";
import ImportMenu from "./ImportMenu";
import type { Conversation, ConversationDetailDto, ExportOutput, ImportResultDto, SearchResult, SourceSession, ExtractionResult } from "./types";
import { sourceLabel } from "./types";

type View = "chat" | "overview" | "cost" | "security" | "assets";
type SourceKey = "zcode" | "claude-code" | "cursor" | "minimax" | "codex";

const NAV_ITEMS = [
  ["chat", "💬", "对话"], ["overview", "📊", "概览"], ["cost", "💰", "成本"],
  ["security", "🛡", "安全"], ["assets", "🧩", "资产"],
] as const;

export default function App() {
  const [view, setView] = useState<View>(() => {
    const v = localStorage.getItem("ch-view");
    return (["overview","cost","security","assets","chat"] as const).includes(v as View) ? v as View : "chat";
  });
  const [theme, setTheme] = useState<"dark"|"light">(() => (localStorage.getItem("ch-theme") as "dark"|"light") || "dark");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem("ch-sidebar") === "1");

  // data
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selectedConv, setSelectedConv] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<import("./types").Message[]>([]);
  const [events, setEvents] = useState<import("./types").EventDto[]>([]);
  const [completenessLabel, setCompletenessLabel] = useState("");
  const [knowledge, setKnowledge] = useState<ExtractionResult | null>(null);
  const [childConvs, setChildConvs] = useState<Record<string, Conversation[]>>({});
  const [expandedParents, setExpandedParents] = useState<Set<string>>(new Set());

  // ui state
  const [convsLoading, setConvsLoading] = useState(false);
  const [msgsLoading, setMsgsLoading] = useState(false);
  const [providerFilter, setProviderFilter] = useState<string | null>(null);
  const [selectedWs, setSelectedWs] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResult[] | null>(null);
  const [highlightMsgId, setHighlightMsgId] = useState<string | null>(null);
  const [collapsedMsgs, setCollapsedMsgs] = useState<Set<string>>(new Set());
  const [timelineMode, setTimelineMode] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [resetArmed, setResetArmed] = useState(false);
  const [importMenu, setImportMenu] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [syncResult, setSyncResult] = useState<string | null>(null);

  // source panel
  const [sourcePanel, setSourcePanel] = useState<SourceKey | null>(null);
  const [sourceSessions, setSourceSessions] = useState<SourceSession[]>([]);
  const [importing, setImporting] = useState(false);
  const [batchProgress, setBatchProgress] = useState<{done:number;total:number}|null>(null);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const showError = (e: unknown) => setError(typeof e === "string" ? e : (e as {message?:string}).message ?? String(e));

  // ── effects ──
  useEffect(() => { document.documentElement.dataset.theme = theme; localStorage.setItem("ch-theme", theme); }, [theme]);
  useEffect(() => { localStorage.setItem("ch-view", view); }, [view]);
  useEffect(() => { localStorage.setItem("ch-sidebar", sidebarCollapsed ? "1" : "0"); }, [sidebarCollapsed]);

  useEffect(() => {
    loadConversations();
    const t = setTimeout(() => autoSync(), 600);
    return () => clearTimeout(t);
  }, []);

  useEffect(() => {
    const interval = setInterval(() => { autoSync(true); invoke("ops_sync", {force:false}).catch(() => {}); }, 10 * 60 * 1000);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); searchInputRef.current?.focus(); searchInputRef.current?.select(); }
      if (e.key === "Escape") { if (sourcePanel) setSourcePanel(null); else if (searchResults) { setSearchResults(null); setSearchQuery(""); } }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [sourcePanel, searchResults]);

  useEffect(() => { if (!searchResults) loadConversations(); }, [providerFilter]);

  // ── data loading ──
  const loadConversations = async () => {
    setConvsLoading(true);
    try {
      const convs = await invoke<Conversation[]>("list_conversations", { workspaceId: null, provider: providerFilter });
      setConversations([...convs].sort((a, b) => (b.updated_at_ms ?? 0) - (a.updated_at_ms ?? 0)));
    } catch (e) { showError(e); }
    setConvsLoading(false);
  };

  const autoSync = async (silent = false) => {
    if (!silent) setSyncing(true);
    setError(null);
    try {
      const result = await invoke<Record<string, number>>("auto_sync", {});
      const parts: string[] = [];
      for (const [key, label] of [["zcode","ZCode"],["claude_code","Claude Code"],["cursor","Cursor"],["minimax","MiniMax"],["codex","Codex"]] as [string,string][]) {
        const ok = result[`${key}_imported`] ?? 0;
        if (ok > 0) parts.push(`${label}: ${ok} 新`);
      }
      setSyncResult(parts.length > 0 ? parts.join(" | ") : "无新数据");
      await loadConversations();
    } catch (e) {
      const msg = typeof e === "string" ? e : String(e);
      if (!msg.includes("同步中") && !msg.includes("重置中")) showError(e);
    }
    setSyncing(false);
  };

  const selectConversation = async (c: Conversation, highlightId?: string) => {
    setSelectedConv(c);
    setHighlightMsgId(highlightId ?? null);
    setCollapsedMsgs(new Set());
    setMsgsLoading(true);
    try {
      const detail = await invoke<ConversationDetailDto>("get_conversation_detail", { conversationId: c.id });
      setMessages(detail.messages); setEvents(detail.events);
      setCompletenessLabel(detail.completeness_label); setKnowledge(null);
      if (highlightId) setTimeout(() => document.getElementById(`msg-${highlightId}`)?.scrollIntoView({behavior:"smooth",block:"center"}), 100);
    } catch (e) { showError(e); }
    setMsgsLoading(false);
  };

  const toggleExpand = async (c: Conversation) => {
    const s = new Set(expandedParents);
    if (s.has(c.id)) s.delete(c.id);
    else { s.add(c.id); if (!childConvs[c.id]) {
      try {
        const children = await invoke<Conversation[]>("list_child_conversations", { parentSourceId: c.source_conversation_id, provider: c.provider });
        setChildConvs((p) => ({ ...p, [c.id]: children }));
      } catch (e) { showError(e); }
    }}
    setExpandedParents(s);
  };

  // ── actions ──
  const doSearch = async () => {
    if (!searchQuery.trim()) { setSearchResults(null); return; }
    try { setSearchResults(await invoke<SearchResult[]>("search", { query: searchQuery })); } catch (e) { showError(e); }
  };

  const jumpToSearchResult = async (r: SearchResult) => {
    try {
      const detail = await invoke<ConversationDetailDto>("get_conversation_detail", { conversationId: r.conversation_id });
      setSelectedConv(detail.conversation); setMessages(detail.messages); setEvents(detail.events);
      setCompletenessLabel(detail.completeness_label); setKnowledge(null);
      setHighlightMsgId(r.message_id); setCollapsedMsgs(new Set()); setSearchResults(null);
      setTimeout(() => document.getElementById(`msg-${r.message_id}`)?.scrollIntoView({behavior:"smooth",block:"center"}), 120);
    } catch (e) { showError(e); }
  };

  const exportCurrent = async (format: "markdown" | "json") => {
    if (!selectedConv) return;
    setExporting(true);
    try {
      const out = await invoke<ExportOutput>("export_conversation", { conversationId: selectedConv.id, format });
      const path = await save({ defaultPath: out.filename, filters: [{ name: format.toUpperCase(), extensions: [out.format] }] });
      if (typeof path === "string") await invoke("save_text_file", { path, content: out.content });
    } catch (e) { showError(e); }
    setExporting(false);
  };

  const extractKnowledge = async () => {
    if (!selectedConv) return;
    try { setKnowledge(await invoke<ExtractionResult>("extract_knowledge", { conversationId: selectedConv.id })); } catch (e) { showError(e); }
  };

  const importHandler = async () => {
    try {
      const selected = await open({ multiple: false, filters: [{ name: "Conversations", extensions: ["md","markdown","jsonl","ndjson"] }] });
      if (typeof selected !== "string") return;
      const result = await invoke<ImportResultDto>("import_file", { path: selected, workspaceName: null });
      await loadConversations();
      alert(`✓ 导入成功\n消息 ${result.messages} 条 · 完整度 ${result.completeness}`);
    } catch (e) { showError(e); }
  };

  const loadSourceSessions = async (source: SourceKey) => {
    setSourcePanel(source); setSourceSessions([]);
    try {
      const cmd = { "zcode":"list_zcode_sessions","claude-code":"list_claude_code_sessions","cursor":"list_cursor_sessions","minimax":"list_minimax_sessions","codex":"list_codex_sessions" }[source];
      setSourceSessions(await invoke<SourceSession[]>(cmd));
    } catch (e) { showError(e); }
  };

  const importFromSource = async (sessionId: string) => {
    const target = sourceSessions.find((s) => s.session_id === sessionId);
    if (target?.imported) return;
    setImporting(true);
    try {
      const cmd = { "zcode":"import_from_zcode","claude-code":"import_from_claude_code","cursor":"import_from_cursor","minimax":"import_from_minimax","codex":"import_from_codex" }[sourcePanel!]!;
      const result = await invoke<ImportResultDto>(cmd, { sessionId });
      await loadConversations(); setImporting(false); setSourcePanel(null);
      alert(`✓ 导入成功\n消息 ${result.messages} 条 · 事件 ${result.events} 个 · 完整度 ${result.completeness}`);
      try {
        const prov = sourcePanel === "minimax" ? "minimax-code" : sourcePanel;
        const conv = await invoke<Conversation | null>("get_conversation_by_source", { provider: prov, sourceConversationId: sessionId });
        if (conv) { setView("chat"); await selectConversation(conv); }
      } catch { }
    } catch (e) { setImporting(false); showError(e); }
  };

  const importAllFromSource = async () => {
    if (!sourcePanel) return;
    setImporting(true);
    const pending = sourceSessions.filter((s) => !s.imported);
    const cmd = { "zcode":"import_from_zcode","claude-code":"import_from_claude_code","cursor":"import_from_cursor","minimax":"import_from_minimax","codex":"import_from_codex" }[sourcePanel]!;
    let ok = 0, fail = 0;
    for (let i = 0; i < pending.length; i++) {
      setBatchProgress({ done: i, total: pending.length });
      try { await invoke<ImportResultDto>(cmd, { sessionId: pending[i].session_id }); ok++; } catch { fail++; }
    }
    await loadConversations();
    setImporting(false); setBatchProgress(null); setSourcePanel(null);
    const skipped = sourceSessions.length - pending.length;
    alert(`批量增量导入\n新增 ${ok} 条${fail > 0 ? ` · 失败 ${fail} 条` : ""}${skipped > 0 ? ` · 已最新 ${skipped} 条` : ""}`);
  };

  const resetData = async () => {
    if (resetting) { setError("正在重置中，请稍候…"); return; }
    setResetting(true); setError(null);
    try {
      await invoke("reset_all_data");
      setConversations([]); setSelectedConv(null); setMessages([]); setEvents([]);
      setKnowledge(null); setSelectedWs(null); setProviderFilter(null);
      setChildConvs({}); setExpandedParents(new Set());
      setSyncResult("已重置，后台重新加载中…");
    } catch (e) { showError(e); }
    setResetting(false); setResetArmed(false);
    autoSync();
  };

  const jumpFromAudit = async (provider: string, sourceConvId: string, messageId: string | null) => {
    setView("chat");
    try {
      const conv = await invoke<Conversation | null>("get_conversation_by_source", { provider, sourceConversationId: sourceConvId });
      if (conv) await selectConversation(conv, messageId ?? undefined);
      else setError("未找到对应会话");
    } catch (e) { showError(e); }
  };

  // ── render ──
  return (
    <div className={`app ${sidebarCollapsed ? "sidebar-collapsed" : ""}`}>
      <nav className="sidebar">
        <button className="sidebar-toggle" onClick={() => setSidebarCollapsed(!sidebarCollapsed)}>
          {sidebarCollapsed ? "»" : "«"}
        </button>
        {NAV_ITEMS.map(([v, icon, label]) => (
          <button key={v} className={`nav-item ${view === v ? "active" : ""}`} onClick={() => setView(v)} title={label}>
            <span className="nav-icon">{icon}</span>
            {!sidebarCollapsed && <span className="nav-label">{label}</span>}
          </button>
        ))}
      </nav>

      <div className="app-body">
        {error && <div className="error-banner" onClick={() => setError(null)}>{error} (点击关闭)</div>}

        <div className="topbar">
          <h1>Threadock</h1>
          {view === "chat" && (<>
            {syncing ? (
              <span className="sync-status syncing-chip">⟳ 数据更新中…<button className="sync-cancel" onClick={() => invoke("cancel_sync").catch(() => {})}>取消</button></span>
            ) : syncResult && <span className="sync-status done">{syncResult}</span>}
            <div className="search-box">
              <input ref={searchInputRef} type="text" placeholder="搜索所有会话…  (⌘K)"
                value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doSearch()} />
              <button onClick={doSearch}>搜索</button>
              {searchResults && <button onClick={() => { setSearchResults(null); setSearchQuery(""); }}>清除</button>}
            </div>
            <ImportMenu open={importMenu} onToggle={() => setImportMenu(!importMenu)}
              onSelect={(s) => { setImportMenu(false); if (s === "file") importHandler(); else loadSourceSessions(s as SourceKey); }} />
          </>)}
          <button className={`reset-btn ${resetArmed ? "armed" : ""}`} disabled={resetting}
            onClick={() => { if (resetArmed) resetData(); else { setResetArmed(true); setTimeout(() => setResetArmed(false), 3000); } }}>
            {resetting ? "重置中…" : resetArmed ? "确认重置？" : "↻ 重置"}
          </button>
          <button className="action-btn" title="增量导入（10 分钟自动执行）"
            onClick={async () => { setSyncing(true); try { await invoke("auto_sync", {}); } catch {} setSyncing(false); await loadConversations(); }}>
            {syncing ? "⟳" : "⇩ 增量"}
          </button>
          <button className="theme-toggle" onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>
            {theme === "dark" ? "☀" : "☾"}
          </button>
        </div>

        {sourcePanel && (
          <SourcePanel panel={sourcePanel === "minimax" ? "minimax-code" : sourcePanel}
            sessions={sourceSessions} importing={importing} progress={batchProgress}
            onImport={importFromSource} onImportAll={importAllFromSource}
            onClose={() => setSourcePanel(null)} sourceLabel={sourceLabel} />
        )}

        {view !== "chat" ? (
          <OpsView section={view} onJumpToConversation={jumpFromAudit} />
        ) : (
          <div className="main">
            <div className="panel">
              {searchResults
                ? <SearchPanel results={searchResults} query={searchQuery} onJump={jumpToSearchResult} />
                : <ConversationList conversations={conversations} selectedConv={selectedConv}
                    loading={convsLoading} providerFilter={providerFilter} selectedWs={selectedWs}
                    expandedParents={expandedParents} childConvs={childConvs}
                    onFilter={setProviderFilter} onSelect={selectConversation}
                    onToggleExpand={toggleExpand}
                    onClearWs={() => { setSelectedWs(null); setConversations([]); }} />}
            </div>
            <div className="panel">
              {selectedConv
                ? <ConversationDetail conv={selectedConv} messages={messages} events={events}
                    completenessLabel={completenessLabel} knowledge={knowledge}
                    loading={msgsLoading} exporting={exporting} timelineMode={timelineMode}
                    highlightMsgId={highlightMsgId} collapsedMsgs={collapsedMsgs}
                    onToggleTimeline={() => setTimelineMode(!timelineMode)}
                    onExport={exportCurrent} onExtractKnowledge={extractKnowledge}
                    onToggleCollapse={(id) => setCollapsedMsgs((p) => { const n = new Set(p); n.has(id) ? n.delete(id) : n.add(id); return n; })} />
                : <div className="empty">选择一条会话查看详情</div>}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
