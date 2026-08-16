// App 主组件：布局 + 状态管理 + 导航（组件已拆分到独立文件）
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import OpsView from "./OpsView";
import SourcePanel from "./SourcePanel";
import ConversationList from "./ConversationList";
import ConversationDetail from "./ConversationDetail";
import SearchPanel from "./SearchPanel";
import ImportMenu from "./ImportMenu";
import SettingsView from "./SettingsView";
import BudgetBar from "./BudgetBar";
import KnowledgeModal from "./KnowledgeModal";
import KnowledgeView from "./KnowledgeView";
import ActivityView from "./ActivityView";
import ProjectsView from "./ProjectsView";
import ReportModal from "./ReportModal";
import HelpShortcuts from "./HelpShortcuts";
import ChangelogModal, { shouldShowChangelog } from "./ChangelogModal";
import OnboardingTour, { isOnboardingSeen, markOnboardingSeen, resetOnboarding } from "./OnboardingTour";
import { Toasts } from "./Toasts";
import ErrorBoundary from "./ErrorBoundary";
import { CommandPalette, type Page } from "./CommandPalette";
import { showToast, subscribeToasts, toastSnapshot, dismissToast } from "./toast";
import { loadNumberFormat, saveNumberFormat, loadCurrency, saveCurrency, loadDateFormat, saveDateFormat, type NumberFormat, type Currency, type DateFormat } from "./prefs";
import Resizer, { loadClampedNumber, saveNumber } from "./Resizer";
import type { ListScope } from "./ConversationList";
import type { Conversation, ConversationDetailDto, ExportOutput, ImportResultDto, SearchResult, SourceSession, ExtractionResult } from "./types";
import { sourceLabel } from "./types";

type View = Page;
type SourceKey = "zcode" | "claude-code" | "cursor" | "minimax" | "codex";

const NAV_ITEMS = [
  ["chat", "💬", "对话"], ["overview", "📊", "概览"], ["cost", "💰", "成本"],
  ["security", "🛡", "安全"], ["assets", "🧩", "资产"],
  ["knowledge", "📚", "知识库"], ["activity", "📆", "活动"], ["projects", "📁", "项目"],
] as const;

/** 视图标签（用于 window.title 反映当前页）。 */
const VIEW_LABEL: Record<View, string> = {
  chat: "对话", overview: "概览", cost: "成本", security: "安全", assets: "资产",
  knowledge: "知识库", activity: "活动", projects: "项目",
};

/** 数据新鲜度徽标：拉 last_ops_sync_ms / last_conv_sync_ms，按时间窗口分绿/黄/橙。 */
function FreshnessBadge() {
  const [ms, setMs] = useState<number | null>(null);
  useEffect(() => {
    let cancelled = false;
    const fetch_ = async () => {
      try {
        const v = await invoke<string | null>("app_setting_get", { key: "last_conv_sync_ms" });
        if (cancelled) return;
        setMs(v ? Number(v) : null);
      } catch { /* 静默 */ }
    };
    fetch_();
    const id = window.setInterval(fetch_, 30_000);
    return () => { cancelled = true; window.clearInterval(id); };
  }, []);
  if (ms == null || ms === 0) {
    return <span className="freshness-badge freshness-missing" title="尚无同步记录（点击导入会话）">⚪ 未同步</span>;
  }
  const ageMs = Date.now() - ms;
  const min = Math.floor(ageMs / 60_000);
  const fmt = (n: number) => n < 60 ? `${n} 分钟前` : n < 1440 ? `${Math.floor(n / 60)} 小时前` : `${Math.floor(n / 1440)} 天前`;
  if (ageMs < 5 * 60_000) {
    return <span className="freshness-badge freshness-fresh" title={`上次同步：${fmt(min)}`}>🟢 {fmt(min)}</span>;
  }
  if (ageMs < 30 * 60_000) {
    return <span className="freshness-badge freshness-warn" title={`上次同步：${fmt(min)} — 建议点导入重新同步`}>🟡 {fmt(min)}</span>;
  }
  return <span className="freshness-badge freshness-stale" title={`上次同步：${fmt(min)} — 数据可能过期，点导入刷新`}>🟠 {fmt(min)}</span>;
}

/** 底部状态栏：当前页 + 同步状态 + 实时时间 + 快捷键提示。 */
function StatusBar({ syncResult, syncing, nowMs, viewLabel }: { syncResult: string | null; syncing: boolean; nowMs: number; viewLabel: string }) {
  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  const mod = isMac ? "⌘" : "Ctrl";
  const time = new Date(nowMs).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
  return (
    <div className="status-bar">
      <span className="status-cell">📍 {viewLabel}</span>
      <span className={`status-cell status-sync ${syncing ? "syncing" : syncResult ? "done" : ""}`}>
        {syncing ? "⟳ 同步中…" : (syncResult ?? "○ 待同步")}
      </span>
      <span className="status-cell status-spacer" />
      <span className="status-cell status-hint">{mod}K 命令 · {mod}? 速查 · {mod}F 搜索 · {mod}R 刷新</span>
      <span className="status-cell status-time">{time}</span>
    </div>
  );
}

export default function App() {
  const [view, setView] = useState<View>(() => {
    const v = localStorage.getItem("ch-view");
    return (["overview","cost","security","assets","knowledge","activity","projects","chat"] as const).includes(v as View) ? v as View : "chat";
  });
  // Command Palette（⌘K 全局搜索 + 跳转）
  const [cmdOpen, setCmdOpen] = useState(false);
  // 快捷键速查（⌘? 唤起）
  const [helpOpen, setHelpOpen] = useState(false);
  // 更新日志：版本变化时启动自动显示一次
  const [changelogOpen, setChangelogOpen] = useState(() => shouldShowChangelog());
  // 首次启动引导：未看过时自动显示；走完后只通过右下角「?」按钮重新唤起
  const [onboardingOpen, setOnboardingOpen] = useState(() => !isOnboardingSeen());
  const [theme, setTheme] = useState<"dark"|"light">(() => (localStorage.getItem("ch-theme") as "dark"|"light") || "dark");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem("ch-sidebar") === "1");
  // 侧边栏 / 会话列表 宽度（可拖拽，localStorage 持久化）
  const [sidebarWidth, setSidebarWidth] = useState(() => loadClampedNumber("ch-sidebar-width", 160, 48, 320));
  const [listWidth, setListWidth] = useState(() => loadClampedNumber("ch-list-width", 340, 240, 540));

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
  const [importMenu, setImportMenu] = useState(false);
  // 私有笔记（仅本地状态；切换会话时重新加载）
  const [noteText, setNoteText] = useState<string | null>(null);
  // 全部标签（按使用频次倒序；懒加载，启动后 1.5s 拉一次）
  const [allTags, setAllTags] = useState<{ tag: string; count: number }[]>([]);
  // 未导入新内容计数（导入按钮红点）：同步/导入完成后重算
  const [newCount, setNewCount] = useState<import("./ImportMenu").NewCount | null>(null);
  const refreshNewCount = async () => {
    try { setNewCount(await invoke<import("./ImportMenu").NewCount>("sources_new_count", {})); }
    catch { /* 红点检测失败静默 */ }
  };
  // 库中实际存在会话的来源集合（无数据的来源不在筛选栏显示，如未装 Cursor）
  const [availableProviders, setAvailableProviders] = useState<Set<string>>(new Set());
  const refreshProviders = async () => {
    try {
      const all = await invoke<Conversation[]>("list_conversations", {
        workspaceId: null, provider: null, favorite: null, archived: null, includeDeleted: false,
      });
      setAvailableProviders(new Set(all.map((c) => c.provider)));
    } catch { /* 静默：失败时保持全部 chips */ }
  };
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [reportsOpen, setReportsOpen] = useState(false);
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
  // 显示偏好（数字格式 / 货币 / 日期格式）
  const [numberFormat, setNumberFormat] = useState<NumberFormat>(loadNumberFormat);
  const [currency, setCurrency] = useState<Currency>(loadCurrency);
  const [dateFormat, setDateFormat] = useState<DateFormat>(loadDateFormat);
  const changeNumberFormat = (f: NumberFormat) => { setNumberFormat(f); saveNumberFormat(f); invoke("app_setting_set", { key: "pref_number_format", value: f }).catch(() => {}); };
  const changeCurrency = (c: Currency) => { setCurrency(c); saveCurrency(c); invoke("app_setting_set", { key: "pref_currency", value: c }).catch(() => {}); };
  const changeDateFormat = (f: DateFormat) => { setDateFormat(f); saveDateFormat(f); invoke("app_setting_set", { key: "pref_date_format", value: f }).catch(() => {}); };
  const changeTheme = (t: "dark" | "light") => { setTheme(t); invoke("app_setting_set", { key: "pref_theme", value: t }).catch(() => {}); };

  // source panel
  const [sourcePanel, setSourcePanel] = useState<SourceKey | null>(null);
  const [sourceSessions, setSourceSessions] = useState<SourceSession[]>([]);
  const [importing, setImporting] = useState(false);
  const [batchProgress, setBatchProgress] = useState<{done:number;total:number}|null>(null);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const detailPanelRef = useRef<HTMLDivElement>(null);
  const [toastList, setToastList] = useState(toastSnapshot());
  useEffect(() => subscribeToasts(() => setToastList(toastSnapshot())), []);
  // 同步/导入进度（后端 sync_progress 事件驱动，顶部进度条展示）
  const [syncProgress, setSyncProgress] = useState<{ current: number; total: number; detail: string; finished: boolean } | null>(null);
  // 状态栏：当前时间（每秒刷新）
  const [nowMs, setNowMs] = useState(Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  useEffect(() => {
    const un = listen<{ current: number; total: number; detail: string; finished: boolean }>("sync_progress", (e) => {
      setSyncProgress(e.payload);
      if (e.payload.finished) {
        window.setTimeout(() => setSyncProgress(null), 1500);
      }
    });
    return () => { un.then((f) => f()); };
  }, []);
  const showError = (e: unknown) => setError(typeof e === "string" ? e : (e as {message?:string}).message ?? String(e));

  // ── effects ──
  useEffect(() => { document.documentElement.dataset.theme = theme; localStorage.setItem("ch-theme", theme); }, [theme]);
  useEffect(() => { localStorage.setItem("ch-view", view); }, [view]);
  useEffect(() => { localStorage.setItem("ch-sidebar", sidebarCollapsed ? "1" : "0"); }, [sidebarCollapsed]);
  // Window title 反映当前页（OS 任务栏/活动指示友好）
  useEffect(() => {
    const sub = selectedConv ? ` · ${selectedConv.user_title ?? selectedConv.title ?? "未命名"}` : "";
    document.title = `Threadock · ${VIEW_LABEL[view]}${sub}`;
  }, [view, selectedConv]);

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
      refreshProviders();
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
      // ⌘K / Ctrl+K 唤起 Command Palette
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setCmdOpen((v) => !v);
        return;
      }
      // ⌘? / Ctrl+? 唤起快捷键速查
      if ((e.metaKey || e.ctrlKey) && (e.key === "?" || (e.shiftKey && e.key === "/"))) {
        e.preventDefault();
        setHelpOpen((v) => !v);
        return;
      }
      // ⌘F / Ctrl+F 焦点搜索框（preventDefault 屏蔽浏览器默认页内查找）
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
        return;
      }
      // ⌘R / Ctrl+R 手动刷新（preventDefault 屏蔽浏览器刷新；走数据重载流程）
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        runManualSync();
        showToast("↻ 已触发数据刷新", "info", 2000);
        return;
      }
      // ⌘1..8 直接跳页
      if ((e.metaKey || e.ctrlKey) && /^[1-8]$/.test(e.key)) {
        e.preventDefault();
        const order: Page[] = ["chat", "overview", "cost", "security", "assets", "knowledge", "activity", "projects"];
        const idx = Number(e.key) - 1;
        if (order[idx]) {
          setView(order[idx]);
          localStorage.setItem("ch-view", order[idx]);
        }
        return;
      }
      if (e.key === "Escape") {
        if (helpOpen) setHelpOpen(false);
        else if (cmdOpen) setCmdOpen(false);
        else if (sourcePanel) setSourcePanel(null);
        else if (searchResults) { setSearchResults(null); setSearchQuery(""); }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [sourcePanel, searchResults, helpOpen, cmdOpen]);

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
      // 同步返回的剩余计数：红点/菜单瞬时刷新（免去 sources_new_count 二次全量扫描）
      if (result.new_counts) {
        setNewCount({ ...(result.new_counts as unknown as Record<string, number>), total: result.new_total ?? 0 });
      }
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
      if (msg.includes("同步中") || msg.includes("重置中")) {
        // 已有同步在进行（多为启动自动同步与手点并发）：明确告知而非静默无反应
        if (!silent) showToast("⟳ 已有同步正在进行，完成后将自动刷新", "info");
      } else {
        showError(e);
      }
    }
    setSyncing(false);
    // 同步完成：红点已由返回值瞬时更新；来源显隐与状态条清理
    refreshProviders();
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
    // 加载该会话的私有笔记（失败/不存在 → null）
    try {
      const n = await invoke<{ note: string; updated_at: number } | null>("get_conversation_note", { id: c.id });
      setNoteText(n?.note ?? null);
    } catch { setNoteText(null); }
    // 刷新全部标签（标签增删后保持最新，乐观策略：每次选会话都拉一次；命中缓存避免重复调用）
    refreshAllTags();
  };
  const refreshAllTags = async () => {
    try { setAllTags(await invoke<{ tag: string; count: number }[]>("list_all_tags", { limit: 100 })); }
    catch { /* 静默 */ }
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
  const doSearch = async (overrideQuery?: string) => {
    // 空关键词 + 勾选「仅我的提问」= 全量我的提问；两者皆空则清空结果
    const q = (overrideQuery ?? searchQuery).trim();
    if (!q) { setSearchResults(null); return; }
    try {
      setSearchResults(await invoke<SearchResult[]>("search", { query: q }));
      addSearchHistory(q);
    } catch (e) { showError(e); }
  };

  // 搜索历史：localStorage 持久化最近 10 条；按使用时间倒序
  const SEARCH_HISTORY_KEY = "ch-search-history";
  const [searchHistory, setSearchHistory] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem(SEARCH_HISTORY_KEY) ?? "[]") as string[]; } catch { return []; }
  });
  const [historyOpen, setHistoryOpen] = useState(false);
  const addSearchHistory = (q: string) => {
    const trimmed = q.trim();
    if (!trimmed) return;
    setSearchHistory((prev) => {
      const next = [trimmed, ...prev.filter((x) => x !== trimmed)].slice(0, 10);
      try { localStorage.setItem(SEARCH_HISTORY_KEY, JSON.stringify(next)); } catch { /* 静默 */ }
      return next;
    });
  };
  const clearSearchHistory = () => {
    setSearchHistory([]);
    try { localStorage.removeItem(SEARCH_HISTORY_KEY); } catch { /* 静默 */ }
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
    try {
      const r = await invoke<ExtractionResult>("extract_knowledge", { conversationId: selectedConv.id });
      setKnowledge(r);
      const empty = !r.summary && !(r.decisions ?? []).length && !(r.todos ?? []).length
        && !(r.errors ?? []).length && !(r.commands ?? []).length && !(r.files ?? []).length;
      if (empty) showToast("✨ 本会话未提取到知识要点", "info");
    } catch (e) {
      // 提取失败不打断浏览：toast 呈现原因（此前弹 error-banner 会被误认为页面异常）
      showToast(`知识提取失败：${typeof e === "string" ? e : (e as { message?: string }).message ?? String(e)}`, "error");
    }
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
    // 已导入也允许重导：导入幂等（upsert 覆盖），用于修正历史数据
    const target = sourceSessions.find((s) => s.session_id === sessionId);
    const wasImported = target?.imported ?? false;
    setImporting(true);
    try {
      const cmd = { "zcode":"import_from_zcode","claude-code":"import_from_claude_code","cursor":"import_from_cursor","minimax":"import_from_minimax","codex":"import_from_codex" }[sourcePanel!]!;
      const result = await invoke<ImportResultDto>(cmd, { sessionId });
      await loadConversations(); setImporting(false); setSourcePanel(null);
      setSyncResult(`✓ 已同步 · ${result.messages} 条消息已导入`);
      refreshNewCount();
      window.setTimeout(() => setSyncResult(null), 15000);
      alert(`✓ ${wasImported ? "重新导入" : "导入"}成功\n消息 ${result.messages} 条 · 事件 ${result.events} 个 · 完整度 ${result.completeness}`);
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

  /** 重置后的统一刷新（实际删除由设置弹窗的 reset_range 命令完成）。 */
  const resetData = async () => {
    setConversations([]); setSelectedConv(null); setMessages([]); setEvents([]);
    setKnowledge(null); setSelectedWs(null); setProviderFilter(null); setDetailTags([]);
    setChildConvs({}); setExpandedParents(new Set());
    setNewCount(null);
    refreshNewCount();
    refreshProviders();
    window.setTimeout(() => autoSync(true), 1500);
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
  // 单条归档（右键菜单触发）：用 archiveOne 替代旧 toggleArchive（已移至右键菜单）
  const archiveOne = async (c: Conversation) => {
    try {
      await invoke("set_archived", { id: c.id, archived: !c.archived });
      await loadConversations();
      if (selectedConv?.id === c.id) setSelectedConv({ ...selectedConv, archived: !c.archived });
      showToast(!c.archived ? "🗄 已归档" : "📤 已取消归档");
    } catch (e) { showError(e); }
  };
  // 单条删除（带 undo）：复用 onBulkDelete 内部循环
  const deleteOneWithUndo = async (c: Conversation) => {
    const snapshot = conversations.filter((x) => x.id === c.id).map((x) => ({ ...x }));
    try {
      for (const id of [c.id]) {
        try { await invoke("soft_delete_conversation", { id }); } catch { /* 单条失败不影响整体 */ }
      }
      await loadConversations();
      if (selectedConv?.id === c.id) setSelectedConv(null);
      showToast(
        `🗑 已移入回收站（${c.user_title ?? c.title ?? "未命名"}）`,
        "info",
        6000,
        async () => {
          try { await invoke("restore_conversation", { id: c.id }); await loadConversations(); showToast("↩ 已恢复", "info"); }
          catch (e) { showError(e); }
        },
        "撤销",
      );
    } catch (e) { showError(e); }
    void snapshot;
  };
  // 复制标题到剪贴板
  const copyConvTitle = async (c: Conversation) => {
    const t = c.user_title ?? c.title ?? "";
    try { await navigator.clipboard.writeText(t); showToast("✓ 标题已复制", "info", 1500); }
    catch { showToast("剪贴板不可用", "error"); }
  };
  // 跳到详情并滚动到指定消息
  const jumpToMessage = async (_c: Conversation, _messageId?: string) => {
    // 预留给未来「跨会话跳到指定消息」场景；当前由 jumpFromAudit 直接调用 selectConversation。
  };
  void jumpToMessage;

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
    try { await invoke("add_tag", { id: selectedConv.id, tag }); await loadDetail(selectedConv.id); refreshAllTags(); }
    catch (e) { showError(e); }
  };

  const removeTag = async (tag: string) => {
    if (!selectedConv) return;
    try { await invoke("remove_tag", { id: selectedConv.id, tag }); await loadDetail(selectedConv.id); refreshAllTags(); }
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
      <nav className="sidebar" style={{ width: sidebarCollapsed ? 48 : sidebarWidth }}>
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
      {!sidebarCollapsed && (
        <Resizer
          className="sidebar-resizer"
          onDrag={(dx) => setSidebarWidth((w) => { const n = Math.round(w + dx); const c = Math.max(120, Math.min(320, n)); saveNumber("ch-sidebar-width", c); return c; })}
          title="拖拽调整侧边栏宽度"
        />
      )}

      <div className="app-body">
        {error && <div className="error-banner" onClick={() => setError(null)}>{error} (点击关闭)</div>}
        <ErrorBoundary>

        <div className="topbar">
          <h1>Threadock</h1>
          {view === "chat" && (<>
            {syncing ? (
              <span className="sync-status syncing-chip">
                ⟳ {syncProgress && syncProgress.total > 0
                  ? `导入中 ${syncProgress.current}/${syncProgress.total}${syncProgress.detail && syncProgress.detail !== "done" ? ` · ${syncProgress.detail}` : ""}`
                  : "数据更新中…"}
                <button className="sync-cancel" onClick={() => invoke("cancel_sync").catch(() => { /* 后台任务失败不打断 UI */ })}>取消</button>
              </span>
            ) : syncResult && <span className="sync-status done">{syncResult}</span>}

            <div className="search-box">
              <input ref={searchInputRef} type="text" placeholder="搜索所有会话…  (⌘K 唤起命令面板)"
                value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doSearch()}
                onFocus={() => setHistoryOpen(true)}
                onBlur={() => window.setTimeout(() => setHistoryOpen(false), 180)} />
              <button onClick={() => doSearch()}>搜索</button>
              {searchResults && <button onClick={() => { setSearchResults(null); setSearchQuery(""); }}>清除</button>}
              {historyOpen && searchHistory.length > 0 && !searchResults && (
                <div className="search-history-dropdown" onMouseDown={(e) => e.preventDefault()}>
                  <div className="search-history-head">
                    <span>最近搜索</span>
                    <button className="kb-copy" onClick={clearSearchHistory} title="清空历史">清空</button>
                  </div>
                  {searchHistory.map((q, i) => (
                    <button key={i} className="search-history-item" onClick={() => { setSearchQuery(q); setHistoryOpen(false); doSearch(q); }}>
                      <span className="search-history-q">{q}</span>
                      <span className="search-history-hint">↵</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
            <ImportMenu open={importMenu} onToggle={() => setImportMenu(!importMenu)} onSync={runManualSync} syncing={syncing} newCount={newCount}
              onSelect={(s) => { setImportMenu(false); if (s === "file") importHandler(); else loadSourceSessions(s as SourceKey); }} />
          </>)}
          {syncProgress && syncProgress.total > 0 && (
            <div className={`sync-progress ${syncProgress.finished ? "done" : ""}`} title={`${syncProgress.detail} ${syncProgress.current}/${syncProgress.total}`}>
              <div
                className="sync-progress-fill"
                style={{ width: `${Math.min(100, (syncProgress.current / syncProgress.total) * 100)}%` }}
              />
            </div>
          )}
          {budgetInfo && (
            <BudgetBar
              costSoFar={budgetInfo.costSoFar} tokensSoFar={budgetInfo.tokensSoFar}
              projectedCost={budgetInfo.projectedCost} projectedTokens={budgetInfo.projectedTokens}
              costLimit={budgetInfo.costLimit} tokenLimit={budgetInfo.tokenLimit}
            />
          )}
          {/* 数据新鲜度：5 分内绿 / 30 分内黄 / 超期橙（悬停看精确时间） */}
          <FreshnessBadge />
          {/* 主题 / 备份 / 命令面板按钮已移至设置面板（避免顶栏拥挤，参考 macOS 设计） */}
          <button
            className="settings-toggle"
            title="快捷键速查 (⌘?)"
            onClick={() => setHelpOpen(true)}
            style={{ fontSize: 13 }}
          >?</button>
          <button className="settings-toggle" title="设置" onClick={() => setSettingsOpen(true)}>⚙</button>
        </div>

        <Toasts toasts={toastList} onDismiss={dismissToast} />

        <CommandPalette
          open={cmdOpen}
          onClose={() => setCmdOpen(false)}
          onJumpPage={(p) => { setView(p); localStorage.setItem("ch-view", p); }}
          onJumpConversation={async (cid) => {
            setView("chat");
            try {
              const detail = await invoke<import("./types").ConversationDetailDto>("get_conversation_detail", { conversationId: cid });
              setSelectedConv(detail.conversation); setMessages(detail.messages); setEvents(detail.events);
              setCompletenessLabel(detail.completeness_label); setDetailTags(detail.tags ?? []);
            } catch (e) { showError(e); }
          }}
        />

        {knowledge && selectedConv && (
          <KnowledgeModal
            knowledge={knowledge}
            conversationId={selectedConv.id}
            convTitle={selectedConv.user_title ?? selectedConv.title}
            onClose={() => setKnowledge(null)}
            onReextract={extractKnowledge}
            onJumpToConversation={(cid) => {
              // 跨会话引用跳到其他会话：保留知识弹窗
              invoke<ConversationDetailDto>("get_conversation_detail", { conversationId: cid })
                .then((d) => { setSelectedConv(d.conversation); setMessages(d.messages); setEvents(d.events); setCompletenessLabel(d.completeness_label); setDetailTags(d.tags ?? []); setKnowledge(null); });
            }}
          />
        )}

        {reportsOpen && <ReportModal onClose={() => setReportsOpen(false)} />}

        {helpOpen && <HelpShortcuts onClose={() => setHelpOpen(false)} />}

        {changelogOpen && <ChangelogModal onClose={() => setChangelogOpen(false)} />}

        {onboardingOpen && (
          <OnboardingTour
            onClose={() => { markOnboardingSeen(); setOnboardingOpen(false); }}
          />
        )}

        {isOnboardingSeen() && !onboardingOpen && (
          <button
            className="onboarding-fab"
            onClick={() => { resetOnboarding(); setOnboardingOpen(true); }}
            title="重新查看新手引导"
            aria-label="新手引导"
            data-testid="onboarding-fab"
          >?</button>
        )}

        {settingsOpen && (
          <SettingsView theme={theme} onThemeChange={changeTheme}
            syncIntervalMin={syncIntervalMin} onSyncIntervalChange={changeSyncInterval}
            retentionDays={retentionDays} onRetentionDaysChange={changeRetentionDays}
            notifyOnExceed={notifyOnExceed} onNotifyOnExceedChange={changeNotifyOnExceed}
            numberFormat={numberFormat} onNumberFormatChange={changeNumberFormat}
            currency={currency} onCurrencyChange={changeCurrency}
            dateFormat={dateFormat} onDateFormatChange={changeDateFormat}
            onNavigate={(v) => setView(v)}
            onReset={resetData} resetting={false}
            onClose={() => setSettingsOpen(false)}
            onShowChangelog={() => { setSettingsOpen(false); setChangelogOpen(true); }}
            onReapplyImportedPrefs={(): void => {
              // 从 localStorage 重新读所有偏好（避免刷新页面）
              const v = localStorage.getItem("ch-theme");
              if (v === "light" || v === "dark") changeTheme(v);
              const nf = localStorage.getItem("ch-pref-number");
              if (nf === "raw" || nf === "k" || nf === "wan" || nf === "yi") setNumberFormat(nf);
              const cu = localStorage.getItem("ch-pref-currency");
              if (cu === "USD" || cu === "CNY") setCurrency(cu);
              const df = localStorage.getItem("ch-pref-date");
              if (df === "relative" || df === "absolute" || df === "iso") setDateFormat(df);
            }} />
        )}

        {sourcePanel && (
          <SourcePanel panel={sourcePanel === "minimax" ? "minimax-code" : sourcePanel}
            sessions={sourceSessions} importing={importing} progress={batchProgress}
            onImport={importFromSource} onImportAll={importAllFromSource}
            onClose={() => setSourcePanel(null)} sourceLabel={sourceLabel} />
        )}

        {view === "knowledge" ? (
          <KnowledgeView onJump={async (cid) => {
            setView("chat");
            try {
              const detail = await invoke<import("./types").ConversationDetailDto>("get_conversation_detail", { conversationId: cid });
              setSelectedConv(detail.conversation); setMessages(detail.messages); setEvents(detail.events);
              setCompletenessLabel(detail.completeness_label); setDetailTags(detail.tags ?? []);
            } catch (e) { showError(e); }
          }} />
        ) : view === "activity" ? (
          <ActivityView
            onJumpToConversation={async (cid) => {
              setView("chat");
              try {
                const detail = await invoke<import("./types").ConversationDetailDto>("get_conversation_detail", { conversationId: cid });
                setSelectedConv(detail.conversation); setMessages(detail.messages); setEvents(detail.events);
                setCompletenessLabel(detail.completeness_label); setDetailTags(detail.tags ?? []);
              } catch (e) { showError(e); }
            }}
          />
        ) : view === "projects" ? (
          <ProjectsView
            onJumpToConversation={async (cid) => {
              setView("chat");
              try {
                const detail = await invoke<import("./types").ConversationDetailDto>("get_conversation_detail", { conversationId: cid });
                setSelectedConv(detail.conversation); setMessages(detail.messages); setEvents(detail.events);
                setCompletenessLabel(detail.completeness_label); setDetailTags(detail.tags ?? []);
              } catch (e) { showError(e); }
            }}
          />
        ) : view !== "chat" ? (
          <OpsView section={view} onJumpToConversation={jumpFromAudit} onOpenReports={() => setReportsOpen(true)} />
        ) : (
          <div className="main" style={{ gridTemplateColumns: `${listWidth}px 6px 1fr` }}>
            <div className="panel" style={{ width: listWidth }}>
              {searchResults
                ? <SearchPanel results={searchResults} query={searchQuery} onJump={jumpToSearchResult} />
                : <ConversationList conversations={conversations} selectedConv={selectedConv}
                    loading={convsLoading} providerFilter={providerFilter} selectedWs={selectedWs}
                    expandedParents={expandedParents} childConvs={childConvs}
                    scope={scope} onScopeChange={setScope} availableProviders={availableProviders}
                    onFilter={setProviderFilter} onSelect={selectConversation}
                    onToggleExpand={toggleExpand} onToggleFavorite={toggleFavorite}
                    onArchiveOne={archiveOne} onDeleteOne={deleteOneWithUndo} onCopyTitle={copyConvTitle}
                    onRestore={restoreConv}
                    onBulkFavorite={async (ids, favorite) => {
                      // 单次逐条 set_favorite：未来如需可换 batch 接口
                      for (const id of ids) {
                        try { await invoke("set_favorite", { id, favorite }); } catch {}
                      }
                      await loadConversations();
                    }}
                    onBulkArchive={async (ids, archived) => {
                      for (const id of ids) {
                        try { await invoke("set_archived", { id, archived }); } catch {}
                      }
                      await loadConversations();
                    }}
                    onBulkAddTag={async (ids, tag) => {
                      // 逐条 add_tag：未来如需可加 batch 接口
                      for (const id of ids) {
                        try { await invoke("add_tag", { id, tag }); } catch { /* 单条失败不影响整体 */ }
                      }
                      showToast(`✓ 已为 ${ids.length} 条会话加标签 #${tag}`, "info");
                    }}
                    onBulkDelete={async (ids) => {
                      // 软删前先记录原状态，撤销时能恢复
                      const snapshot = conversations.filter((c) => ids.includes(c.id));
                      for (const id of ids) {
                        try { await invoke("delete_conversation", { id }); } catch {}
                      }
                      await loadConversations();
                      if (snapshot.length > 0) {
                        showToast(
                          `🗑 已删除 ${snapshot.length} 条会话`,
                          "info",
                          6000,
                          async () => {
                            // 撤销：用 restore_conversation 命令（按 source_conversation_id）
                            for (const c of snapshot) {
                              try {
                                await invoke("restore_conversation", {
                                  id: c.source_conversation_id,
                                });
                              } catch { /* 单条失败不影响整体 */ }
                            }
                            await loadConversations();
                            showToast(`↩ 已恢复 ${snapshot.length} 条会话`, "info");
                          },
                          "撤销删除",
                        );
                      }
                    }}
                    onClearWs={() => { setSelectedWs(null); setConversations([]); }} />}
            </div>
            <Resizer
              onDrag={(dx) => setListWidth((w) => { const n = Math.round(w + dx); const c = Math.max(240, Math.min(540, n)); saveNumber("ch-list-width", c); return c; })}
              title="拖拽调整会话列表宽度"
            />
            <div className="panel" ref={detailPanelRef}>
              {selectedConv
                ? <ConversationDetail conv={selectedConv} messages={messages} events={events}
                    completenessLabel={completenessLabel}
                    scrollContainerRef={detailPanelRef}
                    loading={msgsLoading} exporting={exporting} timelineMode={timelineMode}
                    highlightMsgId={highlightMsgId} collapsedMsgs={collapsedMsgs}
                    tags={detailTags}
                    onAddTag={addTag} onRemoveTag={removeTag} onRescanAudit={rescanAudit}
                    note={noteText}
                    onNoteChange={async (text) => {
                      if (!selectedConv) return;
                      try {
                        await invoke("set_conversation_note", { id: selectedConv.id, note: text });
                        setNoteText(text);
                      } catch (e) {
                        showToast(`保存笔记失败：${String(e)}`, "error");
                      }
                    }}
                    allTags={allTags}
                    onRenameTitle={async (title) => {
                      try {
                        await invoke("set_user_title", { id: selectedConv!.id, title });
                        // 本地同步 selectedConv + 列表
                        const next = { ...selectedConv!, user_title: title };
                        setSelectedConv(next);
                        setConversations((p) => p.map((c) => c.id === next.id ? next : c));
                        showToast(title ? "✓ 标题已更新" : "✓ 已恢复原始标题", "info", 2000);
                      } catch (e) {
                        showToast(`改标题失败：${String(e)}`, "error");
                      }
                    }}
                    onToggleTimeline={() => setTimelineMode(!timelineMode)}
                    onExport={exportCurrent} onExtractKnowledge={extractKnowledge}
                    onToggleCollapse={(id) => setCollapsedMsgs((p) => { const n = new Set(p); if (n.has(id)) { n.delete(id); } else { n.add(id); } return n; })} />
                : <div className="empty empty-cta">
                    {!conversations.length && !convsLoading ? (
                      <>
                        <div className="empty-icon">📥</div>
                        <div className="empty-title">还没有任何会话</div>
                        <div className="empty-hint">
                          点
                          <button className="action-btn" style={{ margin: "0 6px" }} onClick={() => setImportMenu(true)}>⬇ 导入会话</button>
                          把 Cursor / Claude Code / ZCode / Codex 里的历史对话同步进来
                        </div>
                        <div className="empty-hint" style={{ marginTop: 8, opacity: 0.7 }}>或按 <kbd>⌘K</kbd> 唤起命令面板</div>
                      </>
                    ) : (
                      <>
                        <div className="empty-icon">💬</div>
                        <div className="empty-title">选择一条会话查看详情</div>
                        <div className="empty-hint">
                          {convsLoading ? "加载中…" : (
                            <>试试按 <kbd>⌘K</kbd> 搜索会话，或 <kbd>⌘1</kbd> 跳到本视图</>
                          )}
                        </div>
                      </>
                    )}
                  </div>}
            </div>
          </div>
        )}
        </ErrorBoundary>
        <StatusBar syncResult={syncResult} syncing={syncing} nowMs={nowMs} viewLabel={VIEW_LABEL[view]} />
      </div>
    </div>
  );
}
