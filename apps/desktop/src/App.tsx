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
import SettingsView from "./SettingsView";
import BudgetBar from "./BudgetBar";
import { Toasts } from "./Toasts";
import { showToast, subscribeToasts, toastSnapshot, dismissToast } from "./toast";
import type { ListScope } from "./ConversationList";
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
  const [detailTags, setDetailTags] = useState<string[]>([]);
  const [knowledge, setKnowledge] = useState<ExtractionResult | null>(null);
  const [childConvs, setChildConvs] = useState<Record<string, Conversation[]>>({});
  const [expandedParents, setExpandedParents] = useState<Set<string>>(new Set());

  // ui state
  const [convsLoading, setConvsLoading] = useState(false);
  const [msgsLoading, setMsgsLoading] = useState(false);
  const [providerFilter, setProviderFilter] = useState<string | null>(null);
  const [scope, setScope] = useState<ListScope>("all");
  const [budgetInfo, setBudgetInfo] = useState<{
    costSoFar: number; tokensSoFar: number;
    projectedCost: number | null; projectedTokens: number | null;
    costLimit: number | null; tokenLimit: number | null; notify: boolean;
  } | null>(null);
  const [selectedWs, setSelectedWs] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResult[] | null>(null);
  const [highlightMsgId, setHighlightMsgId] = useState<string | null>(null);
  const [collapsedMsgs, setCollapsedMsgs] = useState<Set<string>>(new Set());
  const [timelineMode, setTimelineMode] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [importMenu, setImportMenu] = useState(false);
  // 未导入新内容计数（导入按钮红点）：同步/导入完成后重算
  const [newCount, setNewCount] = useState<import("./ImportMenu").NewCount | null>(null);
  const refreshNewCount = async () => {
    try { setNewCount(await invoke<import("./ImportMenu").NewCount>("sources_new_count", {})); }
    catch { /* 红点检测失败静默 */ }
  };
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [syncResult, setSyncResult] = useState<string | null>(null);
  // 自动同步间隔（分钟，0 = 关闭）：localStorage 快路径，DB app_settings 持久备份
  const [syncIntervalMin, setSyncIntervalMin] = useState(() => {
    const v = Number(localStorage.getItem("ch-sync-interval"));
    return [0, 5, 10, 30].includes(v) ? v : 10;
  });
  // 保留策略（天，0 = 关闭）与预算通知：localStorage 即时生效
  const [retentionDays, setRetentionDays] = useState(() => Number(localStorage.getItem("ch-retention-days") ?? "0") || 0);
  const [notifyOnExceed, setNotifyOnExceed] = useState(() => localStorage.getItem("ch-budget-notify") === "1");

  // source panel
  const [sourcePanel, setSourcePanel] = useState<SourceKey | null>(null);
  const [sourceSessions, setSourceSessions] = useState<SourceSession[]>([]);
  const [importing, setImporting] = useState(false);
  const [batchProgress, setBatchProgress] = useState<{done:number;total:number}|null>(null);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const [toastList, setToastList] = useState(toastSnapshot());
  useEffect(() => subscribeToasts(() => setToastList(toastSnapshot())), []);
  const showError = (e: unknown) => setError(typeof e === "string" ? e : (e as {message?:string}).message ?? String(e));

  // ── effects ──
  useEffect(() => { document.documentElement.dataset.theme = theme; localStorage.setItem("ch-theme", theme); }, [theme]);
  useEffect(() => { localStorage.setItem("ch-view", view); }, [view]);
  useEffect(() => { localStorage.setItem("ch-sidebar", sidebarCollapsed ? "1" : "0"); }, [sidebarCollapsed]);

  // 预算看门狗：预算/月用量/预测 → 全局预算条；超限且开启通知时弹一次（按月去重）
  const refreshBudget = async () => {
    try {
      const [budget, proj] = await Promise.all([
        invoke<{ monthly_token_limit: number | null; monthly_cost_limit: number | null; notify_on_exceed: boolean }>("budget_get"),
        invoke<{ tokens_so_far: number; cost_so_far: number; projected_tokens: number; projected_cost: number }>("ops_month_projection"),
      ]);
      const info = {
        costSoFar: proj.cost_so_far, tokensSoFar: proj.tokens_so_far,
        projectedCost: proj.projected_cost, projectedTokens: proj.projected_tokens,
        costLimit: budget.monthly_cost_limit ?? null, tokenLimit: budget.monthly_token_limit ?? null,
        notify: budget.notify_on_exceed,
      };
      setBudgetInfo(info);
      const over = (info.costLimit && info.costSoFar >= info.costLimit) || (info.tokenLimit && info.tokensSoFar >= info.tokenLimit);
      if (over && info.notify) {
        const key = `ch-budget-warned-${new Date().getFullYear()}-${new Date().getMonth()}`;
        if (!localStorage.getItem(key)) {
          localStorage.setItem(key, "1");
          showToast(`⚠ 预算已超限：当月 $${info.costSoFar.toFixed(2)} / 预算 $${info.costLimit}`, "error", 10000);
        }
      }
    } catch { /* 预算看门狗失败静默（空库等） */ }
  };

  useEffect(() => {
    loadConversations();
    const t = setTimeout(() => autoSync(), 600);
    // 周报自动生成（>7 天落盘一份）+ 保留策略自动执行 + 预算刷新
    const t2 = setTimeout(async () => {
      refreshBudget();
      refreshNewCount();
      try {
        const r = await invoke<{ generated: boolean; path: string | null }>("weekly_report_auto", {});
        if (r.generated && r.path) showToast(`📄 周报已自动生成：${r.path}`, "info", 8000);
      } catch { /* 失败静默：后台/可选操作 */ }
      try {
        const days = Number(localStorage.getItem("ch-retention-days") ?? "0");
        if (days > 0) {
          const r = await invoke<{ archived: number }>("retention_apply", { days });
          if (r.archived > 0) showToast(`🗄 保留策略：自动归档 ${r.archived} 条 ${days} 天前的会话`, "info");
        }
      } catch { /* 失败静默：后台/可选操作 */ }
    }, 3000);
    return () => { clearTimeout(t); clearTimeout(t2); };
  }, []);

  useEffect(() => {
    if (syncIntervalMin === 0) return; // 设置为关闭
    const interval = setInterval(() => {
      autoSync(true);
      invoke("ops_sync", {force:false}).then(() => { refreshBudget(); refreshNewCount(); }).catch(() => { /* 后台任务失败不打断 UI */ });
    }, syncIntervalMin * 60 * 1000);
    return () => clearInterval(interval);
  }, [syncIntervalMin]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); searchInputRef.current?.focus(); searchInputRef.current?.select(); }
      if (e.key === "Escape") { if (sourcePanel) setSourcePanel(null); else if (searchResults) { setSearchResults(null); setSearchQuery(""); } }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [sourcePanel, searchResults]);

  useEffect(() => { if (!searchResults) loadConversations(); }, [providerFilter, scope]);

  // ── data loading ──
  const loadConversations = async () => {
    setConvsLoading(true);
    try {
      const convs = await invoke<Conversation[]>("list_conversations", {
        workspaceId: null,
        provider: providerFilter,
        favorite: scope === "favorite" ? true : null,
        archived: scope === "archived" ? true : null,
        includeDeleted: scope === "deleted",
      });
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
      // 完成态统一为「已同步」：本次有导入则附明细，没有则「全部最新」
      setSyncResult(parts.length > 0 ? `✓ 已同步 · ${parts.join(" | ")}` : "✓ 已同步 · 全部最新");
      await loadConversations();
    } catch (e) {
      const msg = typeof e === "string" ? e : String(e);
      if (!msg.includes("同步中") && !msg.includes("重置中")) showError(e);
    }
    setSyncing(false);
    // 同步完成：重算红点（全部消化后熄灭）；状态条 15 秒后自动清除避免残留旧统计
    refreshNewCount();
    window.setTimeout(() => setSyncResult((cur) => cur?.startsWith("✓") ? null : cur), 15000);
  };

  const runManualSync = async () => {
    await autoSync();
  };

  const changeSyncInterval = (min: number) => {
    setSyncIntervalMin(min);
    localStorage.setItem("ch-sync-interval", String(min));
    invoke("app_setting_set", { key: "sync_interval_min", value: String(min) }).catch(() => { /* 持久化失败不影响本地生效 */ });
  };

  const changeRetentionDays = (days: number) => {
    setRetentionDays(days);
    localStorage.setItem("ch-retention-days", String(days));
    invoke("app_setting_set", { key: "retention_days", value: String(days) }).catch(() => { /* 持久化失败不影响本地生效 */ });
  };

  const changeNotifyOnExceed = async (v: boolean) => {
    setNotifyOnExceed(v);
    localStorage.setItem("ch-budget-notify", v ? "1" : "0");
    setBudgetInfo((p) => (p ? { ...p, notify: v } : p));
    try {
      const budget = await invoke<{ monthly_token_limit: number | null; monthly_cost_limit: number | null; notify_on_exceed: boolean }>("budget_get");
      await invoke("budget_set", { settings: { ...budget, notify_on_exceed: v } });
    } catch { /* 后端失败保留本地开关状态 */ }
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
      setDetailTags(detail.tags ?? []);
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
      setDetailTags(detail.tags ?? []);
      setDetailTags(detail.tags ?? []);
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
      refreshNewCount();
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
      setSyncResult(`✓ 已同步 · ${result.messages} 条消息已导入`);
      refreshNewCount();
      window.setTimeout(() => setSyncResult(null), 15000);
      alert(`✓ 导入成功\n消息 ${result.messages} 条 · 事件 ${result.events} 个 · 完整度 ${result.completeness}`);
      try {
        const prov = sourcePanel === "minimax" ? "minimax-code" : sourcePanel;
        const conv = await invoke<Conversation | null>("get_conversation_by_source", { provider: prov, sourceConversationId: sessionId });
        if (conv) { setView("chat"); await selectConversation(conv); }
      } catch { /* 失败静默：后台/可选操作 */ }
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
    setSyncResult(`✓ 已同步 · 批量新增 ${ok} 条`);
    refreshNewCount();
    window.setTimeout(() => setSyncResult(null), 15000);
    const skipped = sourceSessions.length - pending.length;
    alert(`批量增量导入\n新增 ${ok} 条${fail > 0 ? ` · 失败 ${fail} 条` : ""}${skipped > 0 ? ` · 已最新 ${skipped} 条` : ""}`);
  };

  const resetData = async () => {
    if (resetting) { setError("正在重置中，请稍候…"); return; }
    setResetting(true); setError(null);
    try {
      await invoke("reset_all_data");
      setConversations([]); setSelectedConv(null); setMessages([]); setEvents([]);
      setKnowledge(null); setSelectedWs(null); setProviderFilter(null); setDetailTags([]);
      setChildConvs({}); setExpandedParents(new Set());
      setSyncResult("已重置，后台重新加载中…");
    } catch (e) { showError(e); }
    setResetting(false);
    autoSync();
  };

  // ── 会话级治理动作 ──
  const toggleFavorite = async (c: Conversation) => {
    try {
      await invoke("set_favorite", { id: c.id, favorite: !c.favorite });
      setConversations((p) => p.map((x) => x.id === c.id ? { ...x, favorite: !x.favorite } : x));
      if (selectedConv?.id === c.id) setSelectedConv({ ...selectedConv, favorite: !c.favorite });
      loadDetail(c.id);
    } catch (e) { showError(e); }
  };

  const toggleArchive = async () => {
    if (!selectedConv) return;
    try {
      await invoke("set_archived", { id: selectedConv.id, archived: !selectedConv.archived });
      const next = { ...selectedConv, archived: !selectedConv.archived };
      setSelectedConv(next);
      await loadConversations();
      showToast(next.archived ? "🗄 已归档" : "📤 已取消归档");
    } catch (e) { showError(e); }
  };

  const softDeleteConv = async () => {
    if (!selectedConv) return;
    try {
      await invoke("delete_conversation", { id: selectedConv.id });
      showToast("🗑 已移入回收站（会话列表切到「已删除」可恢复）");
      setSelectedConv(null); setMessages([]); setEvents([]);
      await loadConversations();
    } catch (e) { showError(e); }
  };

  const hardDeleteConv = async () => {
    if (!selectedConv) return;
    try {
      await invoke("hard_delete_conversation", { id: selectedConv.id });
      showToast("⚡ 已彻底删除（含原始归档与索引文档）");
      setSelectedConv(null); setMessages([]); setEvents([]);
      await loadConversations();
    } catch (e) { showError(e); }
  };

  const restoreConv = async (c: Conversation) => {
    try {
      await invoke("restore_conversation", { id: c.id });
      await loadConversations();
      showToast("↩ 已恢复会话");
    } catch (e) { showError(e); }
  };

  const rescanAudit = async () => {
    if (!selectedConv) return;
    try {
      const findings = await invoke<unknown[]>("audit_scan_conversation", { conversationId: selectedConv.id });
      showToast(findings.length > 0
        ? `🔍 重扫完成：${findings.length} 条发现（详见安全页）`
        : "🔍 重扫完成：本会话无发现", findings.length > 0 ? "warn" : "info");
    } catch (e) { showError(e); }
  };

  const addTag = async (tag: string) => {
    if (!selectedConv) return;
    try { await invoke("add_tag", { id: selectedConv.id, tag }); await loadDetail(selectedConv.id); }
    catch (e) { showError(e); }
  };

  const removeTag = async (tag: string) => {
    if (!selectedConv) return;
    try { await invoke("remove_tag", { id: selectedConv.id, tag }); await loadDetail(selectedConv.id); }
    catch (e) { showError(e); }
  };

  /** 重拉详情（收藏/标签等局部状态同步）。 */
  const loadDetail = async (id: string) => {
    try {
      const detail = await invoke<import("./types").ConversationDetailDto>("get_conversation_detail", { conversationId: id });
      setSelectedConv(detail.conversation); setMessages(detail.messages); setEvents(detail.events);
      setCompletenessLabel(detail.completeness_label);
      setDetailTags(detail.tags ?? []);
    } catch { /* 静默刷新失败保持现状 */ }
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
              <span className="sync-status syncing-chip">⟳ 数据更新中…<button className="sync-cancel" onClick={() => invoke("cancel_sync").catch(() => { /* 后台任务失败不打断 UI */ })}>取消</button></span>
            ) : syncResult && <span className="sync-status done">{syncResult}</span>}
            <div className="search-box">
              <input ref={searchInputRef} type="text" placeholder="搜索所有会话…  (⌘K)"
                value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doSearch()} />
              <button onClick={doSearch}>搜索</button>
              {searchResults && <button onClick={() => { setSearchResults(null); setSearchQuery(""); }}>清除</button>}
            </div>
            <ImportMenu open={importMenu} onToggle={() => setImportMenu(!importMenu)} onSync={runManualSync} syncing={syncing} newCount={newCount}
              onSelect={(s) => { setImportMenu(false); if (s === "file") importHandler(); else loadSourceSessions(s as SourceKey); }} />
          </>)}
          {budgetInfo && (
            <BudgetBar
              costSoFar={budgetInfo.costSoFar} tokensSoFar={budgetInfo.tokensSoFar}
              projectedCost={budgetInfo.projectedCost} projectedTokens={budgetInfo.projectedTokens}
              costLimit={budgetInfo.costLimit} tokenLimit={budgetInfo.tokenLimit}
            />
          )}
          <button className="theme-toggle" onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>
            {theme === "dark" ? "☀" : "☾"}
          </button>
          <button className="settings-toggle" title="设置" onClick={() => setSettingsOpen(true)}>⚙</button>
        </div>

        <Toasts toasts={toastList} onDismiss={dismissToast} />

        {settingsOpen && (
          <SettingsView theme={theme} onThemeChange={setTheme}
            syncIntervalMin={syncIntervalMin} onSyncIntervalChange={changeSyncInterval}
            retentionDays={retentionDays} onRetentionDaysChange={changeRetentionDays}
            notifyOnExceed={notifyOnExceed} onNotifyOnExceedChange={changeNotifyOnExceed}
            onNavigate={(v) => setView(v)}
            onReset={resetData} resetting={resetting}
            onClose={() => setSettingsOpen(false)} />
        )}

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
                    scope={scope} onScopeChange={setScope}
                    onFilter={setProviderFilter} onSelect={selectConversation}
                    onToggleExpand={toggleExpand} onToggleFavorite={toggleFavorite} onRestore={restoreConv}
                    onClearWs={() => { setSelectedWs(null); setConversations([]); }} />}
            </div>
            <div className="panel">
              {selectedConv
                ? <ConversationDetail conv={selectedConv} messages={messages} events={events}
                    completenessLabel={completenessLabel} knowledge={knowledge}
                    loading={msgsLoading} exporting={exporting} timelineMode={timelineMode}
                    highlightMsgId={highlightMsgId} collapsedMsgs={collapsedMsgs}
                    tags={detailTags}
                    onToggleFavorite={() => selectedConv && toggleFavorite(selectedConv)}
                    onToggleArchive={toggleArchive}
                    onSoftDelete={softDeleteConv} onHardDelete={hardDeleteConv}
                    onAddTag={addTag} onRemoveTag={removeTag} onRescanAudit={rescanAudit}
                    onToggleTimeline={() => setTimelineMode(!timelineMode)}
                    onExport={exportCurrent} onExtractKnowledge={extractKnowledge}
                    onToggleCollapse={(id) => setCollapsedMsgs((p) => { const n = new Set(p); if (n.has(id)) { n.delete(id); } else { n.add(id); } return n; })} />
                : <div className="empty">选择一条会话查看详情</div>}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
