// App 主组件：布局 + 状态管理 + 导航（组件已拆分到独立文件）
import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { copyToClipboard } from "./clipboard";
import OpsView from "./OpsView";
import ConversationList from "./ConversationList";
import ConversationDetail from "./ConversationDetail";
import SearchResultsPanel from "./SearchResultsPanel";
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
import StatusBar from "./StatusBar";
import { CommandPalette, type Page, type CommandActionId } from "./CommandPalette";
import { showToast, subscribeToasts, toastSnapshot, dismissToast } from "./toast";
import { loadNumberFormat, saveNumberFormat, loadCurrency, saveCurrency, loadDateFormat, saveDateFormat, type NumberFormat, type Currency, type DateFormat } from "./prefs";
import Resizer, { loadClampedNumber, saveNumber } from "./Resizer";
import ScrollArea, { type ScrollAreaRef } from "./ScrollArea";
import type { ListScope } from "./ConversationList";
import type { Conversation, ConversationDetailDto, ExportOutput, ImportResultDto, SearchHitGroup, SearchResult, ExtractionResult, KnowledgeEngine } from "./types";
import { Icon, type IconName } from "./Icon";
import { EmptyState } from "./EmptyState";

type View = Page;

// 侧栏分组：对话 / 治理 / 资料 — 每项带 SVG 图标名 + ⌘1..N 快捷键
type NavItem = { view: View; icon: IconName; label: string; section: "primary" | "ops" | "library"; shortcut: string };
const NAV_ITEMS: readonly NavItem[] = [
  { view: "chat",      icon: "chat",      label: "对话",   section: "primary", shortcut: "⌘1" },
  { view: "overview",  icon: "overview",  label: "概览",   section: "ops",     shortcut: "⌘2" },
  { view: "cost",      icon: "cost",      label: "成本",   section: "ops",     shortcut: "⌘3" },
  { view: "security",  icon: "shield",    label: "安全",   section: "ops",     shortcut: "⌘4" },
  { view: "activity",  icon: "calendar",  label: "活动",   section: "ops",     shortcut: "⌘5" },
  { view: "knowledge", icon: "library",   label: "知识库", section: "library", shortcut: "⌘6" },
  { view: "assets",    icon: "package",   label: "资产",   section: "library", shortcut: "⌘7" },
  { view: "projects",  icon: "folder",    label: "项目",   section: "library", shortcut: "⌘8" },
] as const;

/** 视图标签（用于 window.title 反映当前页）。 */
const VIEW_LABEL: Record<View, string> = {
  chat: "对话", overview: "概览", cost: "成本", security: "安全", assets: "资产",
  knowledge: "知识库", activity: "活动", projects: "项目",
};

/** 底部状态栏已拆为独立组件 ./StatusBar.tsx（自管 1s 刷新，避免整树重渲染）。 */

export default function App() {
  // 默认始终进入「对话」tab（chat 是主操作页；其他页通过 ⌘1..8 / 侧栏 / ⌘K 跳转）
  // 之前从 ch-view 持久化恢复，但用户重启 app 时通常想从主操作开始
  const [view, setView] = useState<View>("chat");
  // Command Palette（⌘K 全局搜索 + 跳转）
  const [cmdOpen, setCmdOpen] = useState(false);
  // 快捷键速查（⌘? 唤起）
  const [helpOpen, setHelpOpen] = useState(false);
  // 更新日志：版本变化时启动自动显示一次
  const [changelogOpen, setChangelogOpen] = useState(() => shouldShowChangelog());
  // 首次启动引导：未看过时自动显示；走完后通过设置「重新查看新手引导」唤起
  // （round 25：原右下角 ? 浮动按钮移到设置中，避免遮挡主内容）
  const [onboardingOpen, setOnboardingOpen] = useState(() => !isOnboardingSeen());
  const [theme, setTheme] = useState<"dark"|"light">(() => (localStorage.getItem("ch-theme") as "dark"|"light") || "light");
  const [textSize, setTextSize] = useState<"sm"|"md"|"lg"|"xl">(() => {
    const v = localStorage.getItem("ch-text-size");
    return v === "md" || v === "lg" || v === "xl" ? v : "sm";
  });
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
  // 知识提取引擎：rule 默认（离线确定性）；llm 需在设置中启用大模型
  const [knowledgeEngine, setKnowledgeEngine] = useState<KnowledgeEngine>("rule");
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
  // 搜索模式：命中按主对话分组（左栏），null = 非搜索模式
  const [searchGroups, setSearchGroups] = useState<SearchHitGroup[] | null>(null);
  // 角色筛选（"" 全部 / user / assistant）：透传后端重查
  const [searchRole, setSearchRole] = useState("");
  // 右栏命中步进：当前搜索在某会话树（主对话+子对话）内的全部命中与当前下标
  const [hitNav, setHitNav] = useState<{ query: string; hits: SearchResult[]; idx: number } | null>(null);
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
    // 走 available_providers（DISTINCT）避免对每条会话拉整行；之前用 list_conversations 全表扫描（P1-E1）
    try {
      const names = await invoke<string[]>("available_providers", {});
      setAvailableProviders(new Set(names));
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

  // 来源面板 + 单 IDE 导入已下线（统一走「增量同步」 + 「从文件导入」）；
  // 相关 state / SourceSession 类型 / SourcePanel 组件 import 都已清理。

  const searchInputRef = useRef<HTMLInputElement>(null);
  const detailPanelRef = useRef<ScrollAreaRef>(null);
  // 详情加载序号：每次 selectConversation / stepToHit / jumpFromAudit 进入即 +1，
  // 任何 await 之后置状态前比对「当前序号 === 函数入口捕获的序号」，避免快速 A→B 点击时
  // A 的稍后 await 回调把 B 的消息列表覆盖掉（P0-3）。
  const loadSeqRef = useRef(0);
  const [toastList, setToastList] = useState(toastSnapshot());
  useEffect(() => subscribeToasts(() => setToastList(toastSnapshot())), []);
  // 同步/导入进度（后端 sync_progress 事件驱动，顶部进度条展示）
  const [syncProgress, setSyncProgress] = useState<{ current: number; total: number; detail: string; finished: boolean } | null>(null);
  // 状态栏的 1s 时间刷新已迁出 App → ./StatusBar.tsx（自管状态，避免整树重渲染）
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

  /** 把后端 / Tauri 抛出的英文错误翻成简短中文，
   *  避免在顶栏暴露出 "Cannot read properties of undefined (reading 'invoke')" 这种开发态堆栈。 */
  function friendlyError(raw: string): string {
    const lower = raw.toLowerCase();
    if (lower.includes("reading 'invoke'") || lower.includes("reading \"invoke\"")) {
      return "桌面运行时未就绪（请在 Tauri 桌面里打开 Webview，或检查应用启动是否完成）";
    }
    if (lower.includes("network") || lower.includes("fetch")) {
      return "网络异常，请检查连接后重试";
    }
    if (lower.includes("permission") || lower.includes("denied")) {
      return "权限不足，请检查文件 / 系统权限";
    }
    if (lower.includes("not found") || lower.includes("未找到")) {
      return "未找到资源（可能已被删除）";
    }
    if (lower.includes("数据库") || lower.includes("database") || lower.includes("sql")) {
      return "数据库异常，建议重试或重启应用";
    }
    // 其它错误：截断到 120 字符，避免超长堆栈
    return raw.length > 120 ? raw.slice(0, 117) + "…" : raw;
  }

  // ── data loading ──
  // （声明需位于下方 effects 之前：避免 effect 引用「未提升的 const」触发 immutability 检查）
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
      // 后端可能把「同步后实际有数据的 provider 集合」一并返回，省去一次 available_providers 调用
      if (Array.isArray(result.providers)) {
        setAvailableProviders(new Set(result.providers as unknown as string[]));
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

  // 滚动并高亮定位到某条消息（命中步进 / 分组跳转共用）
  const scrollMsgIntoView = (mid: string) => {
    setTimeout(() => {
      document.getElementById(`msg-${mid}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
    }, 120);
  };

  // 打开一个分组行（主对话或子对话）：拉取该主对话树内全部命中并进入步进模式，
  // 起始命中 = 点击的会话内的第一条（点主对话头则从树内第一条开始）
  const openSearchGroup = async (g: SearchHitGroup) => {
    const q = searchQuery.trim();
    if (!q) return;
    try {
      const hits = await invoke<SearchResult[]>("search_tree_hits", {
        query: q, rootConversationId: g.root_conversation_id, role: searchRole || null,
      });
      if (!hits.length) { showToast("该会话（含子对话）没有可跳转的命中", "info"); return; }
      let idx = hits.findIndex((h) => h.conversation_id === g.conversation_id);
      if (idx < 0) idx = 0;
      setHitNav({ query: q, hits, idx });
      await stepToHit(hits[idx], idx);
    } catch (e) { showError(e); }
  };

  // 步进到第 idx 个命中：跨会话自动切换详情（走序号守卫防并发覆盖，P0-3）
  const stepToHit = async (hit: SearchResult, idx: number) => {
    setHitNav((p) => (p ? { ...p, idx } : p));
    if (selectedConv?.id === hit.conversation_id) {
      // 同一会话内移动：仅更新高亮并滚动
      setHighlightMsgId(hit.message_id);
      scrollMsgIntoView(hit.message_id);
      return;
    }
    const seq = ++loadSeqRef.current;
    try {
      const detail = await invoke<ConversationDetailDto>("get_conversation_detail", { conversationId: hit.conversation_id });
      if (loadSeqRef.current !== seq) return;
      setSelectedConv(detail.conversation); setMessages(detail.messages); setEvents(detail.events);
      setCompletenessLabel(detail.completeness_label); setKnowledge(null);
      setDetailTags(detail.tags ?? []);
      setHighlightMsgId(hit.message_id); setCollapsedMsgs(new Set());
      scrollMsgIntoView(hit.message_id);
    } catch (e) { showError(e); }
  };

  // ↑/↓ 步进（循环）；由右栏步进条按钮与全局方向键触发
  const stepHits = (dir: 1 | -1) => {
    if (!hitNav || hitNav.hits.length === 0) return;
    const next = (hitNav.idx + dir + hitNav.hits.length) % hitNav.hits.length;
    void stepToHit(hitNav.hits[next], next);
  };

  // j / k 列表导航（vim 风格）：在 conversations 数组里步进，自动加载详情并滚动到可见
  const navigateConv = useCallback((dir: 1 | -1 | 0, jump?: "first" | "last") => {
    if (conversations.length === 0) return;
    const curIdx = selectedConv ? conversations.findIndex((c) => c.id === selectedConv.id) : -1;
    let nextIdx: number;
    if (jump === "first") nextIdx = 0;
    else if (jump === "last") nextIdx = conversations.length - 1;
    else nextIdx = Math.max(0, Math.min(conversations.length - 1, curIdx + dir));
    const next = conversations[nextIdx];
    if (!next) return;
    void selectConversation(next);
    // 滚动列表容器让该行可见
    requestAnimationFrame(() => {
      const el = document.querySelector(`[data-conv-row="${CSS.escape(next.id)}"]`) as HTMLElement | null;
      el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    });
  }, [conversations, selectedConv]);

  // 退出搜索模式：清分组 + 步进 + 关键词
  const clearSearchMode = () => {
    setSearchGroups(null);
    setHitNav(null);
    setSearchQuery("");
  };

  // ── effects ──
  useEffect(() => {
    // 主题切换淡入：先加 fading 标记 → 等 220ms 后清掉（让 transition 走完）
    const html = document.documentElement;
    html.dataset.theme = theme;
    html.dataset.themeFading = "1";
    localStorage.setItem("ch-theme", theme);
    const t = window.setTimeout(() => { delete html.dataset.themeFading; }, 240);
    return () => window.clearTimeout(t);
  }, [theme]);

  useEffect(() => {
    const html = document.documentElement;
    html.dataset.textSize = textSize;
    html.dataset.textSizeFading = "1";
    localStorage.setItem("ch-text-size", textSize);
    const t = window.setTimeout(() => { delete html.dataset.textSizeFading; }, 240);
    return () => window.clearTimeout(t);
  }, [textSize]);
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

  // 挂载时执行一次：全量加载 + 延迟自动同步/周报/保留策略
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- loadConversations 内部同步 setConvsLoading(true)
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
    // 仅挂载时执行一次（初始加载 + 延迟自动同步/周报/保留策略）；
    // 引用的函数每次渲染重建，加入依赖会导致重复触发，有意省略。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (syncIntervalMin === 0) return; // 设置为关闭
    const interval = setInterval(() => {
      autoSync(true);
      invoke("ops_sync", {force:false}).then(() => { refreshBudget(); refreshNewCount(); }).catch(() => { /* 后台任务失败不打断 UI */ });
    }, syncIntervalMin * 60 * 1000);
    return () => clearInterval(interval);
    // 定时器只需跟随 syncIntervalMin 重建；autoSync 每次渲染均为新引用，
    // 加入依赖会让定时器被反复清除重建，有意省略。
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
      // ⌘, / Ctrl+, 打开设置（macOS 标准：Preferences 快捷键）
      if ((e.metaKey || e.ctrlKey) && e.key === "," && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        setSettingsOpen((v) => !v);
        return;
      }
      // ⌘D / Ctrl+D 复制当前会话 ID（macOS Finder 风格：⌘D 在列表场景里常用于「duplicate / 复制标识」）
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "d" && !e.shiftKey && !e.altKey) {
        if (selectedConv) {
          e.preventDefault();
          const id = selectedConv.source_conversation_id || selectedConv.id;
          void copyToClipboard(id).then((r) => {
            if (r.ok) showToast(`✓ 已复制会话 ID：${id}`, "info", 1800);
            else showToast(`✗ 复制失败：${r.error ?? "未知错误"}`, "error", 2500);
          });
          return;
        }
      }
      // ⌘F / Ctrl+F 焦点搜索框（preventDefault 屏蔽浏览器默认页内查找）
      // 当前在「对话」详情页时让 ConversationDetail 自己处理（详情内搜索），
      // 顶栏搜索框在该场景不存在（P1-C2 双抢焦点）。
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        if (view === "chat" && selectedConv) return; // 留给 ConversationDetail
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
        // ⌘1..8 顺序与侧栏分组展示顺序保持一致：
        // 对话(1) → 概览(2) → 成本(3) → 安全(4) → 活动(5) → 知识库(6) → 资产(7) → 项目(8)
        const order: Page[] = ["chat", "overview", "cost", "security", "activity", "knowledge", "assets", "projects"];
        const idx = Number(e.key) - 1;
        if (order[idx]) {
          setView(order[idx]);
          localStorage.setItem("ch-view", order[idx]);
        }
        return;
      }
      // 命中步进：↑/↓ 在当前会话树（主对话+子对话）的命中间跳转；
      // 输入框聚焦时不抢方向键（列表滚动 / 建议选择等场景）
      if (hitNav && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        const tag = (document.activeElement as HTMLElement | null)?.tagName ?? "";
        if (tag === "INPUT" || tag === "TEXTAREA") return;
        e.preventDefault();
        stepHits(e.key === "ArrowDown" ? 1 : -1);
        return;
      }
      // ⌘G / ⌘⇧G：跳到下一处 / 上一处搜索命中（macOS 标准）
      if (hitNav && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "g") {
        const tag = (document.activeElement as HTMLElement | null)?.tagName ?? "";
        if (tag === "INPUT" || tag === "TEXTAREA") return;
        e.preventDefault();
        stepHits(e.shiftKey ? -1 : 1);
        return;
      }
      // j / k vim 风格列表导航（仅对话页 + 无浮层 + 无修饰键 + 不在输入框）
      // 复用 selectConversation 自动加载详情；⌘J / ⌘K 跳到首 / 尾
      if (view === "chat" && !cmdOpen && !helpOpen && !settingsOpen && !changelogOpen) {
        const tag = (document.activeElement as HTMLElement | null)?.tagName ?? "";
        const inField = tag === "INPUT" || tag === "TEXTAREA" || (document.activeElement as HTMLElement | null)?.isContentEditable;
        if (!inField && !e.metaKey && !e.ctrlKey && !e.altKey) {
          if (e.key === "j" || e.key === "J") {
            e.preventDefault();
            navigateConv(1);
            return;
          }
          if (e.key === "k" || e.key === "K") {
            e.preventDefault();
            navigateConv(-1);
            return;
          }
        }
        if ((e.metaKey || e.ctrlKey) && (e.key === "j" || e.key === "J")) {
          e.preventDefault();
          navigateConv(0, "first");
          return;
        }
        if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
          e.preventDefault();
          navigateConv(0, "last");
          return;
        }
      }
      if (e.key === "Escape") {
        if (helpOpen) setHelpOpen(false);
        else if (settingsOpen) setSettingsOpen(false);
        else if (changelogOpen) setChangelogOpen(false);
        else if (cmdOpen) setCmdOpen(false);
        else if (searchGroups) clearSearchMode();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // runManualSync 每次渲染重建，加入依赖会让全局 keydown 监听器每渲染重挂一次；
    // 处理器所需的状态已尽数列入，有意省略该函数依赖。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchGroups, hitNav, helpOpen, cmdOpen, settingsOpen, changelogOpen, view, selectedConv]);

  // 筛选/范围变化且不在搜索结果模式时重载列表；searchGroups/loadConversations
  // 加入依赖会导致每次渲染重载（loadConversations 每次渲染重建），有意省略；
  // loadConversations 内部同步 setConvsLoading(true) 属 effect 数据加载模式，有意保留。
  // eslint-disable-next-line react-hooks/exhaustive-deps, react-hooks/set-state-in-effect
  useEffect(() => { if (!searchGroups) loadConversations(); }, [providerFilter, scope]);

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
    const seq = ++loadSeqRef.current;
    setSelectedConv(c);
    setHighlightMsgId(highlightId ?? null);
    setCollapsedMsgs(new Set());
    setMsgsLoading(true);
    try {
      const detail = await invoke<ConversationDetailDto>("get_conversation_detail", { conversationId: c.id });
      // 已被更新的加载覆盖（用户已切到其他会话）→ 静默丢弃
      if (loadSeqRef.current !== seq) return;
      setMessages(detail.messages); setEvents(detail.events);
      setCompletenessLabel(detail.completeness_label); setKnowledge(null);
      setDetailTags(detail.tags ?? []);
      if (highlightId) setTimeout(() => {
        // 滚动前再校验一次：避免 setTimeout 期间又切到别的会话
        if (loadSeqRef.current !== seq) return;
        document.getElementById(`msg-${highlightId}`)?.scrollIntoView({behavior:"smooth",block:"center"});
      }, 100);
    } catch (e) { showError(e); }
    if (loadSeqRef.current === seq) setMsgsLoading(false);
    // 加载该会话的私有笔记（失败/不存在 → null）
    try {
      const n = await invoke<{ note: string; updated_at: number } | null>("get_conversation_note", { id: c.id });
      if (loadSeqRef.current !== seq) return;
      setNoteText(n?.note ?? null);
    } catch { if (loadSeqRef.current === seq) setNoteText(null); }
    // 刷新全部标签（标签增删后保持最新，乐观策略：每次选会话都拉一次；命中缓存避免重复调用）
    if (loadSeqRef.current === seq) refreshAllTags();
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
  // 统一搜索入口：命中按主对话分组（左栏）；role 为 "" / "user" / "assistant"
  const runSearch = async (q: string, role: string) => {
    if (!q) { setSearchGroups(null); setHitNav(null); return; }
    try {
      setSearchGroups(await invoke<SearchHitGroup[]>("search_grouped", { query: q, role: role || null }));
      // 新查询重置右栏步进（旧会话树的命中不再有效）
      setHitNav(null);
      addSearchHistory(q);
    } catch (e) { showError(e); }
  };
  const doSearch = (overrideQuery?: string) => runSearch((overrideQuery ?? searchQuery).trim(), searchRole);

  // 搜索历史：localStorage 持久化最近 10 条；按使用时间倒序
  const SEARCH_HISTORY_KEY = "ch-search-history";
  const [searchHistory, setSearchHistory] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem(SEARCH_HISTORY_KEY) ?? "[]") as string[]; } catch { return []; }
  });
  const [historyOpen, setHistoryOpen] = useState(false);

  // ── 保存的搜索（plan §13.2，v1.0.0：跨会话持久，服务端 V14 表）──────
  const [savedSearches, setSavedSearches] = useState<{ id: string; name: string; query_text: string }[]>([]);
  const loadSavedSearches = async () => {
    try { setSavedSearches(await invoke("saved_search_list")); } catch { /* 后台加载失败不打断 UI */ }
  };
  // 挂载时拉一次保存搜索列表（数据加载模式，setState 在异步回调里）
  // eslint-disable-next-line react-hooks/set-state-in-effect -- 数据加载 effect：加载完成后才 setState，非同步级联
  useEffect(() => { void loadSavedSearches(); }, []);
  const deleteSavedSearch = async (id: string) => {
    try {
      await invoke("saved_search_delete", { id });
      await loadSavedSearches();
    } catch (e) { showError(e); }
  };
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

  const extractKnowledge = async (engineArg?: unknown) => {
    if (!selectedConv) return;
    // 防御性归一化：本函数可能被直接挂到 onClick 上（详情页工具栏），
    // 第一个参数是事件对象而非引擎名——非法值一律回退到当前引擎
    const engine: KnowledgeEngine =
      engineArg === "llm" || engineArg === "rule" ? engineArg : knowledgeEngine;
    try {
      const r = await invoke<ExtractionResult>("extract_knowledge", { conversationId: selectedConv.id, engine });
      setKnowledge(r);
      setKnowledgeEngine(engine);
      const empty = !r.summary && !(r.decisions ?? []).length && !(r.todos ?? []).length
        && !(r.errors ?? []).length && !(r.commands ?? []).length && !(r.files ?? []).length;
      if (empty) showToast("本会话未提取到知识要点（摘要/决策/TODO/错误/命令/文件 都为空）", "info");
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
  // ↑ 单 IDE 导入（按 ZCode/Claude/Cursor/MiniMax/Codex 单独 list + 选择性 import）已下线。
  //   现在导入只有两条入口：「增量同步」一次拉全部 + 「从文件导入」指定文件。
  //   importAllFromSource / loadSourceSessions / importFromSource 全部移除 —— sourcePanel 状态也清空。


  /** 重置后的统一刷新（实际删除由设置弹窗的 reset_range 命令完成）。 */
  const [resetting, setResetting] = useState(false);
  const resetData = async () => {
    setResetting(true);
    try {
      // reset_range 命令由 SettingsView 内部 await 执行；这里只负责 UI 状态清理 + 后续刷新。
      setConversations([]); setSelectedConv(null); setMessages([]); setEvents([]);
      setKnowledge(null); setSelectedWs(null); setProviderFilter(null); setDetailTags([]);
      setChildConvs({}); setExpandedParents(new Set());
      setNewCount(null);
      refreshNewCount();
      refreshProviders();
      window.setTimeout(async () => {
        // ① 重导会话 → ② 强制重算指标（reset_range 已删 usage_records，30 分钟
        // 节流会拦住常规 ops_sync，必须 force）→ ③ 概览/成本/红点自动刷新，
        // 全程无需手动点「立即全量同步指标」
        await autoSync(true);
        try {
          await invoke("ops_sync", { force: true });
          refreshBudget();
          refreshNewCount();
        } catch { /* 指标重算失败不打断：会话已恢复，下次启动/定时会补 */ }
      }, 1500);
    } finally {
      // 给 reset_range + 1.5s 后台重导留出 4 秒「禁用期」防止误连点；UI 反馈后解除
      window.setTimeout(() => setResetting(false), 4000);
    }
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
  // 共享软删 + 撤销 toast 助手（单条 / 批量共用，避免「两个命令 + source_conversation_id」错位）
  const performSoftDeleteWithUndo = async (targets: Conversation[]) => {
    if (targets.length === 0) return;
    const snapshot = targets.map((x) => ({ ...x }));
    for (const c of snapshot) {
      try { await invoke("delete_conversation", { id: c.id }); } catch { /* 单条失败不影响整体 */ }
    }
    await loadConversations();
    if (selectedConv && snapshot.some((c) => c.id === selectedConv.id)) setSelectedConv(null);
    const label = snapshot.length === 1
      ? `🗑 已移入回收站（${snapshot[0].user_title ?? snapshot[0].title ?? "未命名"}）`
      : `🗑 已删除 ${snapshot.length} 条会话`;
    showToast(
      label,
      "info",
      6000,
      async () => {
        for (const c of snapshot) {
          try { await invoke("restore_conversation", { id: c.id }); }
          catch { /* 单条失败不影响整体 */ }
        }
        await loadConversations();
        showToast(`↩ 已恢复 ${snapshot.length} 条会话`, "info");
      },
      "撤销删除",
    );
  };
  // 单条删除（带 undo）：薄壳转共享助手
  const deleteOneWithUndo = async (c: Conversation) => {
    await performSoftDeleteWithUndo([c]);
  };
  // 复制标题到剪贴板
  const copyConvTitle = async (c: Conversation) => {
    const t = c.user_title ?? c.title ?? "";
    try { await navigator.clipboard.writeText(t); showToast("✓ 标题已复制", "info", 1500); }
    catch { showToast("剪贴板不可用", "error"); }
  };
  // 跳到详情并滚动到指定消息：原 jumpToMessage 空 stub 已删除。
  // 跨会话跳到指定消息场景由 selectConversation(cid, mid) 直接承担（详见 P0-3 / P1-A3）。

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
    // 先抢一个序号，避免与并发 selectConversation 互相覆盖（selectConversation 内部还会再 +1，但 setView 同步置状态是安全的）
    const seq = ++loadSeqRef.current;
    setView("chat");
    try {
      const conv = await invoke<Conversation | null>("get_conversation_by_source", { provider, sourceConversationId: sourceConvId });
      if (loadSeqRef.current !== seq) return;
      if (conv) await selectConversation(conv, messageId ?? undefined);
      else setError("未找到对应会话");
    } catch (e) { showError(e); }
  };

  // ── render ──
  return (
    <div className={`app ${sidebarCollapsed ? "sidebar-collapsed" : ""}`}>
      {/* 无障碍：跳到主内容（Tab 1 可见，其它隐藏） */}
      <a href="#main-content" className="skip-to-content">跳到主内容</a>
      <nav className="sidebar" style={{ width: sidebarCollapsed ? 56 : sidebarWidth }}>
        <button className="sidebar-toggle" onClick={() => setSidebarCollapsed(!sidebarCollapsed)} title={sidebarCollapsed ? "展开侧栏" : "收起侧栏"}>
          <Icon name={sidebarCollapsed ? "chevron-right" : "chevron-left"} size={12} />
        </button>
        {(["primary", "ops", "library"] as const).map((section, i) => (
          <div key={section} className="sidebar-section">
            {!sidebarCollapsed && (
              <div className="sidebar-section-label">
                {section === "primary" ? "对话" : section === "ops" ? "治理" : "资料"}
              </div>
            )}
            {i > 0 && <div className="sidebar-divider" />}
            {NAV_ITEMS.filter((it) => it.section === section).map((it) => (
              <button
                key={it.view}
                className={`nav-item ${view === it.view ? "active" : ""}`}
                onClick={() => setView(it.view)}
                title={sidebarCollapsed ? `${it.label} · ${it.shortcut}` : it.label}
              >
                <span className="nav-icon"><Icon name={it.icon} size={16} /></span>
                {!sidebarCollapsed && <>
                  <span className="nav-label">{it.label}</span>
                  <span className="nav-shortcut">{it.shortcut}</span>
                </>}
              </button>
            ))}
          </div>
        ))}
      </nav>
      {!sidebarCollapsed && (
        <Resizer
          className="sidebar-resizer"
          onDrag={(dx) => setSidebarWidth((w) => { const n = Math.round(w + dx); const c = Math.max(120, Math.min(320, n)); saveNumber("ch-sidebar-width", c); return c; })}
          title="拖拽调整侧边栏宽度"
        />
      )}

      <div className="app-body" id="main-content" tabIndex={-1}>
        {error && (
          <div className="error-banner" onClick={() => setError(null)} role="alert">
            <span className="error-banner-icon"><Icon name="alert" size={14} /></span>
            <span className="error-banner-text">{friendlyError(error)}</span>
            <button
              className="error-banner-retry"
              onClick={(e) => {
                e.stopPropagation();
                setError(null);
                // 重连核心：刷新会话 + 重置 newCount
                void loadConversations();
                refreshNewCount();
              }}
              title="重新尝试当前操作"
            >
              <Icon name="sync" size={11} /> 重试
            </button>
            <button
              className="error-banner-close"
              onClick={(e) => { e.stopPropagation(); setError(null); }}
              title="关闭（点击 banner 任意处也可关闭）"
              aria-label="关闭错误提示"
            >
              <Icon name="close" size={12} />
            </button>
          </div>
        )}
        <ErrorBoundary>

        <div className="topbar">
          <button className="brand" onClick={() => setView("overview")} title="Threadock · 回到概览">
            <span className="brand-mark"><Icon name="logo" size={14} /></span>
            <span className="brand-name">Threadock</span>
            <span className="brand-tag">v1.1.1</span>
          </button>
          {view === "chat" && (<>
            {syncing ? (
              <span className="sync-status syncing-chip">
                <span className="dot" />
                {syncProgress && syncProgress.total > 0
                  ? `导入中 ${syncProgress.current}/${syncProgress.total}${syncProgress.detail && syncProgress.detail !== "done" ? ` · ${syncProgress.detail}` : ""}`
                  : "数据更新中…"}
                <button className="sync-cancel" onClick={() => invoke("cancel_sync").catch(() => { /* 后台任务失败不打断 UI */ })}>取消</button>
              </span>
            ) : syncResult && <span className="sync-status done"><span className="dot" />{syncResult}</span>}

            <div className="search-box">
              <input ref={searchInputRef} type="text" placeholder="搜索全部会话 · 支持 provider:/workspace:/type: 前缀"
                value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doSearch()}
                onFocus={() => setHistoryOpen(true)}
                onBlur={() => window.setTimeout(() => setHistoryOpen(false), 180)} />
              <Icon name="search" size={14} className="search-icon" />
              <div className="search-actions">
                <button onClick={() => doSearch()}>搜索</button>
              </div>
              {searchGroups && <button onClick={clearSearchMode}>清除</button>}
              {historyOpen && (searchHistory.length > 0 || savedSearches.length > 0) && !searchGroups && (
                <div className="search-history-dropdown" onMouseDown={(e) => e.preventDefault()}>
                  {savedSearches.length > 0 && (
                    <div className="search-history-head">
                      <span>保存的搜索</span>
                    </div>
                  )}
                  {savedSearches.map((s) => (
                    <button key={s.id} className="search-history-item" onClick={() => { setSearchQuery(s.query_text); setHistoryOpen(false); doSearch(s.query_text); }}>
                      <span className="search-history-q">⭐ {s.name}</span>
                      <span className="search-history-hint saved-search-del" title="删除这条保存的搜索"
                        onClick={(e) => { e.stopPropagation(); deleteSavedSearch(s.id); }}>×</span>
                    </button>
                  ))}
                  {searchHistory.length > 0 && (
                    <div className="search-history-head">
                      <span>最近搜索</span>
                      <button className="kb-copy" onClick={clearSearchHistory} title="清空历史">清空</button>
                    </div>
                  )}
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
              onSelect={() => { setImportMenu(false); importHandler(); }} />
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
          {/* 顶栏右移：原 FreshnessBadge 已下线（导入按钮自带红点提示新内容） */}
          <div className="topbar-actions">
            <button
              className="icon-btn"
              title="命令面板 (⌘K)"
              onClick={() => setCmdOpen(true)}
              aria-label="命令面板"
            ><Icon name="command" size={15} /></button>
            <button
              className="icon-btn"
              title="快捷键速查 (⌘?)"
              onClick={() => setHelpOpen(true)}
              aria-label="快捷键速查"
            ><Icon name="help" size={15} /></button>
            <button className="icon-btn" title="设置" onClick={() => setSettingsOpen(true)} aria-label="设置">
              <Icon name="settings" size={15} />
            </button>
          </div>
        </div>

        <Toasts toasts={toastList} onDismiss={dismissToast} />

        <CommandPalette
          open={cmdOpen}
          onClose={() => setCmdOpen(false)}
          onJumpPage={(p) => { setView(p); localStorage.setItem("ch-view", p); }}
          onJumpConversation={async (cid, mid) => {
            setView("chat");
            // 复用 selectConversation 走序号守卫路径（P0-3 + P1-A3 契约）
            const conv = conversations.find((c) => c.id === cid);
            if (conv) await selectConversation(conv, mid);
            else showError(`未找到会话：${cid}`);
          }}
          onAction={(action: CommandActionId) => {
            // P1-E3：⌘K 动作分发。映射到 App 已有 setState / 同步函数。
            switch (action) {
              case "open_settings": setSettingsOpen(true); break;
              case "trigger_sync": runManualSync(); showToast("⟳ 已触发同步", "info", 2000); break;
              case "toggle_theme": changeTheme(theme === "dark" ? "light" : "dark"); showToast(`✓ 主题已切换：${theme === "dark" ? "浅色" : "深色"}`, "info", 1500); break;
              case "show_shortcuts": setHelpOpen(true); break;
              case "open_reports": setReportsOpen(true); break;
              case "show_changelog": setChangelogOpen(true); break;
            }
          }}
        />

        {knowledge && selectedConv && (
          <KnowledgeModal
            knowledge={knowledge}
            conversationId={selectedConv.id}
            convTitle={selectedConv.user_title ?? selectedConv.title}
            onClose={() => setKnowledge(null)}
            engine={knowledgeEngine}
            onReextract={extractKnowledge}
            onJumpToConversation={async (cid) => {
              // 跨会话引用跳到其他会话：保留知识弹窗
              // 复用 selectConversation 走序号守卫（P0-3）
              const conv = conversations.find((c) => c.id === cid);
              if (conv) { setKnowledge(null); await selectConversation(conv); }
              else showError(`未找到会话：${cid}`);
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

        {/* round 25：右下角 ? 浮动按钮移到设置中（避免遮挡主内容） */}

        {settingsOpen && (
          <SettingsView theme={theme} onThemeChange={changeTheme}
            textSize={textSize} onTextSizeChange={setTextSize}
            syncIntervalMin={syncIntervalMin} onSyncIntervalChange={changeSyncInterval}
            retentionDays={retentionDays} onRetentionDaysChange={changeRetentionDays}
            notifyOnExceed={notifyOnExceed} onNotifyOnExceedChange={changeNotifyOnExceed}
            numberFormat={numberFormat} onNumberFormatChange={changeNumberFormat}
            currency={currency} onCurrencyChange={changeCurrency}
            dateFormat={dateFormat} onDateFormatChange={changeDateFormat}
            onNavigate={(v) => setView(v)}
            onReset={resetData} resetting={resetting}
            onClose={() => setSettingsOpen(false)}
            onShowChangelog={() => { setSettingsOpen(false); setChangelogOpen(true); }}
            onShowOnboarding={() => { resetOnboarding(); setSettingsOpen(false); setOnboardingOpen(true); }}
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

        {view === "knowledge" ? (
          <KnowledgeView onJump={async (cid, mid) => {
            setView("chat");
            // 复用 selectConversation 走序号守卫 + 滚动到 mid（P1-A3 契约：可选 message id）
            const conv = conversations.find((c) => c.id === cid);
            if (conv) await selectConversation(conv, mid);
            else showError(`未找到会话：${cid}`);
          }} />
        ) : view === "activity" ? (
          <ActivityView
            onJumpToConversation={async (cid) => {
              setView("chat");
              const conv = conversations.find((c) => c.id === cid);
              if (conv) await selectConversation(conv);
              else showError(`未找到会话：${cid}`);
            }}
          />
        ) : view === "projects" ? (
          <ProjectsView
            onJumpToConversation={async (cid) => {
              setView("chat");
              const conv = conversations.find((c) => c.id === cid);
              if (conv) await selectConversation(conv);
              else showError(`未找到会话：${cid}`);
            }}
          />
        ) : view !== "chat" ? (
          <OpsView section={view} onJumpToConversation={jumpFromAudit} onOpenReports={() => setReportsOpen(true)} />
        ) : (
          <div className="main" style={{ gridTemplateColumns: `${listWidth}px 6px 1fr` }}>
            <ScrollArea style={{ width: listWidth }}>
              {searchGroups
                ? <SearchResultsPanel groups={searchGroups} query={searchQuery} role={searchRole}
                    onRoleChange={(r) => { setSearchRole(r); void runSearch(searchQuery.trim(), r); }}
                    onOpen={openSearchGroup} activeConversationId={selectedConv?.id ?? null} />
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
                        try {
                          await invoke("set_favorite", { id, favorite });
                        } catch {
                          /* 单条失败不影响批量提交 */
                        }
                      }
                      await loadConversations();
                    }}
                    onBulkArchive={async (ids, archived) => {
                      for (const id of ids) {
                        try {
                          await invoke("set_archived", { id, archived });
                        } catch {
                          /* 单条失败不影响批量提交 */
                        }
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
                      // 复用单条同款助手：避免「用 source_conversation_id 撤销导致恢复失败」
                      const targets = conversations.filter((c) => ids.includes(c.id));
                      await performSoftDeleteWithUndo(targets);
                    }}
                    onBulkSplit={async (ids, name) => {
                      try {
                        await invoke("workspace_split", { conversationIds: ids, newName: name });
                        showToast(`✓ 已把 ${ids.length} 条会话拆分到「${name}」`, "info");
                        loadConversations();
                      } catch (e) { showError(e); }
                    }}
                    onClearWs={() => { setSelectedWs(null); setConversations([]); }} />}
            </ScrollArea>
            <Resizer
              onDrag={(dx) => setListWidth((w) => { const n = Math.round(w + dx); const c = Math.max(240, Math.min(540, n)); saveNumber("ch-list-width", c); return c; })}
              title="拖拽调整会话列表宽度"
            />
            {/* 右栏：命中步进条钉在滚动区域外（始终可见，不随内容滚走）+ 详情滚动区 */}
            <div className="detail-col">
              {/* 命中步进条：当前会话树（主对话+子对话）内的全部命中，↑/↓ 跨会话跳转 */}
              {hitNav && (
                <div className="hit-nav-bar">
                  <span className="hit-nav-query" title="当前搜索关键词">🎯 {hitNav.query}</span>
                  <span className="hit-nav-count">
                    {hitNav.hits.length === 0 ? "无命中" : `${hitNav.idx + 1} / ${hitNav.hits.length}`}
                  </span>
                  <button className="msg-search-btn" onClick={() => stepHits(-1)} disabled={hitNav.hits.length < 2} title="上一处命中（↑）">↑</button>
                  <button className="msg-search-btn" onClick={() => stepHits(1)} disabled={hitNav.hits.length < 2} title="下一处命中（↓）">↓</button>
                  <span className="hit-nav-hint">↑/↓ 在主对话与子对话的命中间跳转</span>
                  <button className="msg-search-btn" onClick={clearSearchMode} title="退出搜索模式（Esc）">✕</button>
                </div>
              )}
              <ScrollArea ref={detailPanelRef} style={{ flex: 1 }}>
              {selectedConv
                ? <ConversationDetail conv={selectedConv} messages={messages} events={events}
                    completenessLabel={completenessLabel}
                    scrollContainerRef={detailPanelRef}
                    loading={msgsLoading} exporting={exporting} timelineMode={timelineMode}
                    highlightMsgId={highlightMsgId} collapsedMsgs={collapsedMsgs}
                    tags={detailTags}
                    searchPreset={hitNav?.query ?? null}
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
                    onExport={exportCurrent} onExtractKnowledge={() => void extractKnowledge()}
                    onToggleCollapse={(id) => setCollapsedMsgs((p) => { const n = new Set(p); if (n.has(id)) { n.delete(id); } else { n.add(id); } return n; })} />
                : !conversations.length && !convsLoading ? (
                  <EmptyState
                    icon="mailbox"
                    size="lg"
                    title="还没有任何会话"
                    desc={<>把 Cursor / Claude Code / ZCode / Codex 里的历史对话同步进来，统一管理。</>}
                    action={
                      <>
                        <button className="action-btn primary" onClick={() => setImportMenu(true)}>
                          <Icon name="sync" size={12} /> 立即同步
                        </button>
                        <span className="empty-hint-muted">或按 <kbd>⌘K</kbd> 唤起命令面板</span>
                      </>
                    }
                  />
                ) : (
                  <EmptyState
                    icon="chat"
                    size="lg"
                    state={convsLoading ? "loading" : "default"}
                    title={convsLoading ? "正在拉取会话列表" : "选择一条会话查看详情"}
                    desc={convsLoading ? undefined : <>试试按 <kbd>⌘K</kbd> 搜索会话，或 <kbd>⌘1</kbd> 跳到本视图</>}
                  />
                )}
              </ScrollArea>
            </div>
          </div>
        )}
        </ErrorBoundary>
        <StatusBar syncResult={syncResult} syncing={syncing} viewLabel={VIEW_LABEL[view]} />
      </div>
    </div>
  );
}
