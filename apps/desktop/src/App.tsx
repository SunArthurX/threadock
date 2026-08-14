import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import OpsView from "./OpsView";

// ── 后端返回类型（与 Rust serde 对应）──────────────────────────────────

interface Conversation {
  id: string;
  provider: string;
  source_conversation_id: string;
  title: string | null;
  user_title: string | null;
  status: string | null;
  model: string | null;
  completeness_score: number | null;
  workspace_id: string | null;
  started_at_ms: number | null;
  updated_at_ms: number | null;
  source_parent_id: string | null;
  child_count: number;
}

interface Message {
  id: string;
  role: string;
  content_text: string | null;
  sequence_number: number;
  created_at_ms: number | null;
}

interface SearchResult {
  message_id: string;
  conversation_id: string;
  provider: string;
  role: string;
  title: string | null;
  snippet: string;
}

interface ImportResultDto {
  conversation_id: string;
  workspace_id: string | null;
  messages: number;
  events: number;
  completeness: string;
}

interface SourceSession {
  session_id: string;
  title: string;
  detail: string;
  message_count: number | null;
  imported: boolean;
}

interface EventDto {
  id: string;
  event_type: string;
  summary: string | null;
  sequence_number: number;
}

interface ConversationDetailDto {
  conversation: Conversation;
  messages: Message[];
  events: EventDto[];
  completeness_label: string;
}

interface ExtractionResult {
  summary: string;
  decisions: { decision: string }[];
  todos: { text: string }[];
  errors: { error: string }[];
  commands: string[];
  files: { path: string }[];
  extractor: string;
}

interface ExportOutput {
  content: string;
  format: string;
  filename: string;
}

// 单条消息折叠阈值（字符数）
const COLLAPSE_THRESHOLD = 600;

// ── 组件 ────────────────────────────────────────────────────────────────

export default function App() {
  // 面板级加载态：不阻塞整个应用，各区域独立显示加载中
  const [convsLoading, setConvsLoading] = useState(false);
  const [msgsLoading, setMsgsLoading] = useState(false);
  const [theme, setTheme] = useState<"dark" | "light">(() =>
    (localStorage.getItem("ch-theme") as "dark" | "light") || "dark"
  );
  const [view, setView] = useState<"chat" | "ops">(() =>
    (localStorage.getItem("ch-view") as "chat" | "ops") || "chat"
  );
  const [selectedWs, setSelectedWs] = useState<string | null>(null);
  const [providerFilter, setProviderFilter] = useState<string | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selectedConv, setSelectedConv] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [events, setEvents] = useState<EventDto[]>([]);
  const [completenessLabel, setCompletenessLabel] = useState<string>("");
  const [knowledge, setKnowledge] = useState<ExtractionResult | null>(null);
  const [sourcePanel, setSourcePanel] = useState<"zcode" | "claude-code" | "cursor" | "minimax" | "codex" | null>(null);
  const [sourceSessions, setSourceSessions] = useState<SourceSession[]>([]);
  const [importing, setImporting] = useState(false);
  const [batchProgress, setBatchProgress] = useState<{ done: number; total: number } | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResult[] | null>(null);
  const [highlightMsgId, setHighlightMsgId] = useState<string | null>(null);
  const [collapsedMsgs, setCollapsedMsgs] = useState<Set<string>>(new Set());
  const [expandedParents, setExpandedParents] = useState<Set<string>>(new Set());
  const [childConvs, setChildConvs] = useState<Record<string, Conversation[]>>({});
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [resetArmed, setResetArmed] = useState(false);
  const [importMenu, setImportMenu] = useState<"root" | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const detailRef = useRef<HTMLDivElement>(null);

  const showError = (e: unknown) =>
    setError(typeof e === "string" ? e : (e as { message?: string }).message ?? String(e));

  const [syncing, setSyncing] = useState(false);
  const [syncResult, setSyncResult] = useState<string | null>(null);



  // 自动同步（silent=true 时不阻塞，后台定时同步用）
  const autoSync = async (silent = false) => {
    if (!silent) setSyncing(true);
    setError(null);
    try {
      const result = await invoke<Record<string, number>>("auto_sync", {});
      const parts: string[] = [];
      const sources: [string, string][] = [
        ["zcode", "ZCode"],
        ["claude_code", "Claude Code"],
        ["cursor", "Cursor"],
        ["minimax", "MiniMax"],
        ["codex", "Codex"],
      ];
      for (const [key, label] of sources) {
        const ok = result[`${key}_imported`] ?? 0;
        const skip = result[`${key}_skipped`] ?? 0;
        if (ok > 0 || skip > 0) {
          parts.push(`${label}: ${ok} 新 / ${skip} 旧`);
        }
      }
      setSyncResult(parts.length > 0 ? parts.join(" | ") : "无新数据");
      await loadConversations();
    } catch (e) {
      const msg = typeof e === "string" ? e : (e as { message?: string }).message ?? String(e);
      // 防重入冲突时静默跳过（后台同步或重置进行中）
      if (!msg.includes("同步中") && !msg.includes("重置中")) {
        showError(e);
      }
    }
    setSyncing(false);
  };

  // 主题应用与持久化
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("ch-theme", theme);
  }, [theme]);

  // 视图切换持久化
  useEffect(() => {
    localStorage.setItem("ch-view", view);
  }, [view]);

  // 首次加载：首屏零争锁（先渲染面板数据），稍后再后台同步
  useEffect(() => {
    loadConversations();
    const t = setTimeout(() => autoSync(), 600);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 每 30 分钟全量数据更新（对话 + 治理指标），后台执行不打扰页面
  useEffect(() => {
    const interval = setInterval(() => {
      (async () => {
        await autoSync(true);
        try { await invoke("ops_sync", { force: false }); } catch { /* 节流/互斥时静默 */ }
      })();
    }, 30 * 60 * 1000);
    return () => clearInterval(interval);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // provider 筛选变化时重新加载会话
  useEffect(() => {
    loadConversations();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerFilter]);

  // ⌘K / Ctrl+K 聚焦搜索框
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      }
      if (e.key === "Escape") {
        if (sourcePanel) setSourcePanel(null);
        else if (searchResults) clearSearch();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sourcePanel, searchResults]);

  // 加载全部会话（按来源筛选）
  const loadConversations = async () => {
    setSelectedWs(null);
    setSearchResults(null);
    setExpandedParents(new Set());
    setChildConvs({});
    setConvsLoading(true);
    try {
      const convs = await invoke<Conversation[]>("list_conversations", {
        workspaceId: null,
        provider: providerFilter,
      });
      const sorted = [...convs].sort((a, b) => {
        const ta = a.updated_at_ms ?? 0;
        const tb = b.updated_at_ms ?? 0;
        return tb - ta;
      });
      setConversations(sorted);
      setSelectedConv(null);
      setMessages([]);
    } catch (e) {
      showError(e);
    }
    setConvsLoading(false);
  };

  // 选择会话 → 加载详情
  const selectConversation = async (c: Conversation, highlightId?: string) => {
    setSelectedConv(c);
    setHighlightMsgId(highlightId ?? null);
    setCollapsedMsgs(new Set());
    setMsgsLoading(true);
    try {
      const detail = await invoke<ConversationDetailDto>("get_conversation_detail", {
        conversationId: c.id,
      });
      setMessages(detail.messages);
      setEvents(detail.events);
      setCompletenessLabel(detail.completeness_label);
      setKnowledge(null);
      if (highlightId) {
        setTimeout(() => {
          document.getElementById(`msg-${highlightId}`)?.scrollIntoView({
            behavior: "smooth",
            block: "center",
          });
        }, 100);
      }
    } catch (e) {
      showError(e);
    }
    setMsgsLoading(false);
  };

  // 展开/折叠主任务的子任务列表
  const toggleExpand = async (c: Conversation) => {
    const newSet = new Set(expandedParents);
    if (newSet.has(c.id)) {
      newSet.delete(c.id);
    } else {
      newSet.add(c.id);
      // 首次展开时加载子任务
      if (!childConvs[c.id]) {
        try {
          const children = await invoke<Conversation[]>("list_child_conversations", {
            parentSourceId: c.source_conversation_id,
            provider: c.provider,
          });
          setChildConvs((prev) => ({ ...prev, [c.id]: children }));
        } catch (e) {
          showError(e);
        }
      }
    }
    setExpandedParents(newSet);
  };

  // 知识提取
  const extractKnowledge = async () => {
    if (!selectedConv) return;
    try {
      const result = await invoke<ExtractionResult>("extract_knowledge", {
        conversationId: selectedConv.id,
      });
      setKnowledge(result);
      setError(null);
    } catch (e) {
      showError(e);
    }
  };

  // 搜索
  const doSearch = async () => {
    if (!searchQuery.trim()) {
      setSearchResults(null);
      return;
    }
    try {
      const results = await invoke<SearchResult[]>("search", { query: searchQuery });
      setSearchResults(results);
      setError(null);
    } catch (e) {
      showError(e);
    }
  };

  const clearSearch = () => {
    setSearchResults(null);
    setSearchQuery("");
    setHighlightMsgId(null);
  };

  // 搜索结果点击跳转
  const jumpToSearchResult = async (r: SearchResult) => {
    try {
      const detail = await invoke<ConversationDetailDto>("get_conversation_detail", {
        conversationId: r.conversation_id,
      });
      const conv = detail.conversation;
      setSelectedConv(conv);
      setMessages(detail.messages);
      setEvents(detail.events);
      setCompletenessLabel(detail.completeness_label);
      setKnowledge(null);
      setHighlightMsgId(r.message_id);
      setCollapsedMsgs(new Set());
      setSearchResults(null);
      setTimeout(() => {
        document.getElementById(`msg-${r.message_id}`)?.scrollIntoView({
          behavior: "smooth",
          block: "center",
        });
      }, 120);
    } catch (e) {
      showError(e);
    }
  };

  // 审计命中 → 跳回对话视图定位会话（M4 治理动作，只读跳转）
  const jumpFromAudit = async (provider: string, sourceConvId: string, messageId: string | null) => {
    setView("chat");
    try {
      const conv = await invoke<Conversation | null>("get_conversation_by_source", {
        provider,
        sourceConversationId: sourceConvId,
      });
      if (!conv) {
        setError("未找到对应会话（可能已被重置，先同步数据）");
        return;
      }
      await selectConversation(conv, messageId ?? undefined);
    } catch (e) {
      showError(e);
    }
  };

  // 导入文件
  const importHandler = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Conversations", extensions: ["md", "markdown", "jsonl", "ndjson"] }],
      });
      if (typeof selected !== "string") return;
      const result = await invoke<ImportResultDto>("import_file", {
        path: selected,
        workspaceName: null,
      });
      await loadConversations();
      setError(null);
      alert(`导入成功：${result.messages} 条消息，完整度 ${result.completeness}`);
    } catch (e) {
      showError(e);
    }
  };

  // 导出当前会话
  const exportCurrent = async (format: "markdown" | "json") => {
    if (!selectedConv) return;
    setExporting(true);
    setError(null);
    try {
      const out = await invoke<ExportOutput>("export_conversation", {
        conversationId: selectedConv.id,
        format,
      });
      const filePath = await save({
        defaultPath: out.filename,
        filters: [{ name: format.toUpperCase(), extensions: [out.format] }],
      });
      if (typeof filePath === "string") {
        await invoke("save_text_file", { path: filePath, content: out.content });
        setError(null);
      }
    } catch (e) {
      showError(e);
    }
    setExporting(false);
  };

  // 重置所有数据
  // 重置：两步内联确认（替代阻塞式 native confirm）；
  // wipe 完成立即恢复可点击，重载完全后台（不 await）
  const resetData = async () => {
    if (resetting) {
      setError("正在重置中，请稍候…");
      return;
    }
    setResetting(true);
    setError(null);
    try {
      await invoke("reset_all_data");
      // 清空 UI，页面立即可用
      setConversations([]);
      setSelectedConv(null);
      setMessages([]);
      setEvents([]);
      setKnowledge(null);
      setSelectedWs(null);
      setProviderFilter(null);
      setChildConvs({});
      setExpandedParents(new Set());
      setSyncResult("已重置，后台重新加载中…");
    } catch (e) {
      const msg = typeof e === "string" ? e : (e as { message?: string }).message ?? String(e);
      if (msg.includes("重置中") || msg.includes("同步中")) {
        setError(msg);
      } else {
        showError(e);
      }
    }
    setResetting(false);
    setResetArmed(false);
    // 后台全量重载（不阻塞页面）
    autoSync();
  };

  // 折叠/展开单条消息
  const toggleCollapse = (id: string) => {
    setCollapsedMsgs((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // 时间格式化：YYYY-MM-DD HH:MM:SS
  const formatTime = (ms: number | null): string => {
    if (!ms) return "";
    const d = new Date(ms);
    const Y = d.getFullYear();
    const M = String(d.getMonth() + 1).padStart(2, "0");
    const D = String(d.getDate()).padStart(2, "0");
    const h = String(d.getHours()).padStart(2, "0");
    const m = String(d.getMinutes()).padStart(2, "0");
    const s = String(d.getSeconds()).padStart(2, "0");
    return `${Y}-${M}-${D} ${h}:${m}:${s}`;
  };



  // 来源标签
  const sourceLabel = (p: string): string => {
    const map: Record<string, string> = {
      "claude-code": "Claude Code",
      zcode: "ZCode",
      codex: "Codex",
      cursor: "Cursor",
      "minimax-code": "MiniMax",
      opencode: "OpenCode",
      generic: "导入",
    };
    return map[p] ?? p;
  };

  const providerLabel = (p: string) => sourceLabel(p);

  // 来源面板 key → provider 字符串
  const panelToProvider = (p: string): string => {
    if (p === "minimax") return "minimax-code";
    return p;
  };

  // 加载来源会话列表
  const loadSourceSessions = async (source: "zcode" | "claude-code" | "cursor" | "minimax" | "codex") => {
    setSourcePanel(source);
    setSourceSessions([]);
    setError(null);
    try {
      const cmd = {
        "zcode": "list_zcode_sessions",
        "claude-code": "list_claude_code_sessions",
        "cursor": "list_cursor_sessions",
        "minimax": "list_minimax_sessions",
        "codex": "list_codex_sessions",
      }[source];
      const sessions = await invoke<SourceSession[]>(cmd);
      setSourceSessions(sessions);
    } catch (e) {
      showError(e);
    }
  };

  // 从来源导入一条会话（已导入的直接跳过）
  const importFromSource = async (sessionId: string) => {
    const target = sourceSessions.find((s) => s.session_id === sessionId);
    if (target?.imported) return;
    setImporting(true);
    setError(null);
    try {
      const cmd = {
        "zcode": "import_from_zcode",
        "claude-code": "import_from_claude_code",
        "cursor": "import_from_cursor",
        "minimax": "import_from_minimax",
        "codex": "import_from_codex",
      }[sourcePanel!]!;
      const result = await invoke<ImportResultDto>(cmd, { sessionId });
      await loadConversations();
      setImporting(false);
      setSourcePanel(null);
      alert(
        `导入成功：${result.messages} 条消息，${result.events} 个事件，完整度 ${result.completeness}`
      );
    } catch (e) {
      setImporting(false);
      showError(e);
    }
  };

  // 批量导入
  const importAllFromSource = async () => {
    if (!sourcePanel) return;
    setImporting(true);
    setBatchProgress({ done: 0, total: sourceSessions.length });
    setError(null);
    const cmd = {
      "zcode": "import_from_zcode",
      "claude-code": "import_from_claude_code",
      "cursor": "import_from_cursor",
      "minimax": "import_from_minimax",
      "codex": "import_from_codex",
    }[sourcePanel]!;
    const pending = sourceSessions.filter((s) => !s.imported);
    let ok = 0;
    let fail = 0;
    for (let i = 0; i < pending.length; i++) {
      const s = pending[i];
      setBatchProgress({ done: i, total: pending.length });
      try {
        await invoke<ImportResultDto>(cmd, { sessionId: s.session_id });
        ok += 1;
      } catch {
        fail += 1;
      }
    }
    setBatchProgress({ done: pending.length, total: pending.length });
    await loadConversations();
    setImporting(false);
    setBatchProgress(null);
    setSourcePanel(null);
    alert(`批量导入完成：成功 ${ok} 条${fail > 0 ? `，失败 ${fail} 条` : ""}`);
  };

  // 事件类型中文标签
  const eventTypeLabel = (t: string): string => {
    const map: Record<string, string> = {
      command_started: "命令",
      command_completed: "命令完成",
      diff_generated: "变更",
      tool_call_started: "工具",
      tool_call_completed: "工具完成",
      file_read: "读取文件",
      file_created: "新建文件",
      file_updated: "修改文件",
      file_deleted: "删除文件",
      approval_requested: "请求审批",
      approval_granted: "批准",
      approval_denied: "拒绝",
      error: "错误",
      artifact_created: "产物",
    };
    return map[t] ?? t;
  };

  // 渲染单条消息内容（折叠长消息）
  const renderMessageContent = (m: Message) => {
    const text = m.content_text ?? "(空)";
    const isCollapsed = collapsedMsgs.has(m.id);
    const isLong = text.length > COLLAPSE_THRESHOLD;
    if (isLong && isCollapsed) {
      return (
        <>
          <div className="content">{text.slice(0, COLLAPSE_THRESHOLD)}…</div>
          <button className="collapse-btn" onClick={() => toggleCollapse(m.id)}>
            展开剩余 {text.length - COLLAPSE_THRESHOLD} 字 ▾
          </button>
        </>
      );
    }
    return (
      <>
        <div className="content">{text}</div>
        {isLong && (
          <button className="collapse-btn" onClick={() => toggleCollapse(m.id)}>
            收起 ▴
          </button>
        )}
      </>
    );
  };

  // 渲染会话列表项（主任务 + 可展开子任务）
  const renderConvItem = (c: Conversation, isChild = false) => (
    <div key={c.id}>
      <div
        className={`list-item ${isChild ? "child-item" : ""} ${selectedConv?.id === c.id ? "active" : ""}`}
        onClick={() => selectConversation(c)}
      >
        <div className="title">
          {!isChild && c.child_count > 0 && (
            <span
              className="expand-toggle"
              onClick={(e) => {
                e.stopPropagation();
                toggleExpand(c);
              }}
            >
              {expandedParents.has(c.id) ? "▼" : "▶"}
            </span>
          )}
          {isChild && <span className="child-arrow">↳</span>}
          {c.user_title ?? c.title ?? "(无标题)"}
        </div>
        <div className="meta">
          <span className={`badge source ${c.provider}`}>{sourceLabel(c.provider)}</span>
          <span className="meta-time">{formatTime(c.updated_at_ms)}</span>
          {!isChild && c.child_count > 0 && <span className="meta-child">{c.child_count} 子任务</span>}
          {c.model && <span className="meta-model">{c.model}</span>}
        </div>
      </div>
      {/* 展开的子任务列表 */}
      {!isChild && expandedParents.has(c.id) && childConvs[c.id] && (
        <div className="child-list">
          {childConvs[c.id].length === 0 && <div className="child-empty">无子任务</div>}
          {childConvs[c.id].map((child) => renderConvItem(child, true))}
        </div>
      )}
    </div>
  );

  return (
    <div className="app">
      {error && (
        <div className="error-banner" onClick={() => setError(null)}>
          {error} (点击关闭)
        </div>
      )}
      <div className="topbar">
        <h1>Conversation Hub</h1>
        {/* 视图切换：对话 | 治理 */}
        <div className="view-switcher">
          <button
            className={`view-tab ${view === "chat" ? "active" : ""}`}
            onClick={() => setView("chat")}
          >
            💬 对话
          </button>
          <button
            className={`view-tab ${view === "ops" ? "active" : ""}`}
            onClick={() => setView("ops")}
          >
            📊 治理
          </button>
        </div>
        {view === "chat" && (
          <>
            {syncing ? (
              <span className="sync-status syncing-chip">
                ⟳ 数据更新中…
                <button className="sync-cancel" onClick={() => invoke("cancel_sync").catch(() => {})}>取消</button>
              </span>
            ) : syncResult ? null : null}
            {view === "chat" && !syncing && syncResult && (
              <span className="sync-status done" title={syncResult}>
                ✓ {syncResult}
              </span>
            )}
            <div className="search-box">
              <input
                ref={searchInputRef}
                type="text"
                placeholder="搜索所有会话…  (⌘K)"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doSearch()}
              />
              <button onClick={doSearch}>搜索</button>
              {searchResults && <button onClick={clearSearch}>清除</button>}
            </div>
            {/* 导入来源：聚合下拉菜单 */}
            <div className="import-dropdown">
              <button
                className="import-trigger"
                onClick={() => setImportMenu(importMenu === null ? "root" : null)}
              >
                📥 导入 ▾
              </button>
              {importMenu === "root" && (
                <>
                  <div className="import-backdrop" onClick={() => setImportMenu(null)} />
                  <div className="import-menu">
                    <button onClick={() => { setImportMenu(null); importHandler(); }}>📄 从文件导入（Markdown/JSONL）</button>
                    <div className="import-menu-sep" />
                    <button onClick={() => { setImportMenu(null); loadSourceSessions("zcode"); }}>
                      <span className="badge source zcode">ZCode</span> 从 ZCode 导入
                    </button>
                    <button onClick={() => { setImportMenu(null); loadSourceSessions("claude-code"); }}>
                      <span className="badge source claude-code">Claude Code</span> 从 Claude Code 导入
                    </button>
                    <button onClick={() => { setImportMenu(null); loadSourceSessions("cursor"); }}>
                      <span className="badge source cursor">Cursor</span> 从 Cursor 导入
                    </button>
                    <button onClick={() => { setImportMenu(null); loadSourceSessions("minimax"); }}>
                      <span className="badge source minimax-code">MiniMax</span> 从 MiniMax 导入
                    </button>
                    <button onClick={() => { setImportMenu(null); loadSourceSessions("codex"); }}>
                      <span className="badge source codex">Codex</span> 从 Codex 导入
                    </button>
                  </div>
                </>
              )}
            </div>
          </>
        )}
        <button
          className={`reset-btn ${resetArmed ? "armed" : ""}`}
          disabled={resetting}
          onClick={() => {
            if (resetArmed) {
              resetData();
            } else {
              setResetArmed(true);
              // 3 秒未确认自动解除
              setTimeout(() => setResetArmed(false), 3000);
            }
          }}
          title="删除所有数据并重新加载（点两次确认）"
        >
          {resetting ? "重置中…" : resetArmed ? "确认重置？" : "↻ 重置"}
        </button>
        <button
          className="theme-toggle"
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
          title={theme === "dark" ? "切换到浅色" : "切换到深色"}
        >
          {theme === "dark" ? "☀" : "☾"}
        </button>
      </div>

      {/* 来源导入面板 */}
      {sourcePanel && (
        <div className="source-overlay">
          <div className="source-panel">
            <div className="source-header">
              <h3>
                {sourceLabel(panelToProvider(sourcePanel))} 会话
                <span className="source-count">({sourceSessions.length})</span>
              </h3>
              <div className="source-actions">
                <button
                  className="source-import-all"
                  disabled={importing || sourceSessions.length === 0}
                  onClick={importAllFromSource}
                >
                  全部导入
                </button>
                <button className="source-close" onClick={() => !importing && setSourcePanel(null)}>
                  ✕
                </button>
              </div>
            </div>
            {importing && (
              <div className="source-importing">
                {batchProgress
                  ? `批量导入中… ${batchProgress.done}/${batchProgress.total}`
                  : "导入中…"}
              </div>
            )}
            {batchProgress && (
              <div className="batch-progress">
                <div
                  className="batch-progress-bar"
                  style={{ width: `${(batchProgress.done / batchProgress.total) * 100}%` }}
                />
              </div>
            )}
            <div className="source-list">
              {sourceSessions.map((s) => (
                <div
                  key={s.session_id}
                  className={`source-item ${s.imported ? "imported" : ""}`}
                  onClick={() => !importing && !s.imported && importFromSource(s.session_id)}
                >
                  <div className="source-title">
                    {s.title || "(无标题)"}
                    {s.imported && <span className="imported-badge">✓ 已导入</span>}
                  </div>
                  <div className="source-meta">
                    {s.message_count != null && `${s.message_count} 消息 · `}
                    {s.detail}
                  </div>
                </div>
              ))}
              {sourceSessions.length === 0 && (
                <div className="source-empty">加载中或无数据…</div>
              )}
            </div>
          </div>
        </div>
      )}

      {view === "ops" ? (
        <OpsView onJumpToConversation={jumpFromAudit} />
      ) : (
      <div className="main">
        {/* 中栏：搜索结果 或 会话列表 */}
        <div className="panel">
          {searchResults ? (
            <>
              <div className="panel-header">
                搜索结果 ({searchResults.length}) · 关键词「{searchQuery}」
              </div>
              {searchResults.map((r) => (
                <div
                  key={r.message_id}
                  className="search-result"
                  onClick={() => jumpToSearchResult(r)}
                >
                  <div className="title">
                    {r.title ?? "(无标题)"}
                    <span className={`badge source ${r.provider}`}>{providerLabel(r.provider)}</span>
                    <span className="search-role">{r.role}</span>
                  </div>
                  <div
                    className="snippet"
                    dangerouslySetInnerHTML={{ __html: r.snippet }}
                  />
                </div>
              ))}
              {searchResults.length === 0 && <div className="empty">无匹配</div>}
            </>
          ) : (
            <>
              <div className="panel-header">
                会话 ({conversations.length})
                {selectedWs && (
                  <span
                    className="clear-ws"
                    onClick={() => {
                      setSelectedWs(null);
                      setConversations([]);
                    }}
                  >
                    ✕
                  </span>
                )}
              </div>
              {/* 标签筛选栏：按来源筛选 */}
              <div className="filter-bar">
                <button
                  className={`filter-chip ${providerFilter === null ? "active" : ""}`}
                  onClick={() => setProviderFilter(null)}
                >
                  全部
                </button>
                {["zcode", "claude-code", "cursor", "minimax-code", "codex"].map((p) => (
                  <button
                    key={p}
                    className={`filter-chip ${providerFilter === p ? "active" : ""}`}
                    onClick={() => setProviderFilter(p)}
                  >
                    {sourceLabel(p)}
                  </button>
                ))}
              </div>
              {convsLoading && (
                <div className="panel-loading">
                  <div className="spinner spinner-sm" />
                  <span>加载会话…</span>
                </div>
              )}
              {!convsLoading && conversations.map((c) => renderConvItem(c))}
              {!convsLoading && conversations.length === 0 && (
                <div className="empty">{selectedWs ? "该项目下暂无会话" : "选择左侧项目"}</div>
              )}
            </>
          )}
        </div>

        {/* 右栏：详情 */}
        <div className="panel" ref={detailRef}>
          {selectedConv ? (
            <div className="detail">
              <h2>
                {selectedConv.user_title ?? selectedConv.title ?? "(无标题)"}
                {completenessLabel && (
                  <span className={`badge completeness ${completenessLabel}`}>
                    {completenessLabel}
                  </span>
                )}
              </h2>
              <div className="meta-row">
                来源: {sourceLabel(selectedConv.provider)} · 模型: {selectedConv.model ?? "unknown"}
                {selectedConv.completeness_score != null &&
                  ` · ${(selectedConv.completeness_score * 100).toFixed(0)}%`}
                {selectedConv.started_at_ms && ` · ${formatTime(selectedConv.started_at_ms)}`}
                {selectedConv.source_parent_id && ` · 子任务`}
              </div>
              <div className="detail-actions">
                <button
                  className="action-btn"
                  disabled={exporting}
                  onClick={() => exportCurrent("markdown")}
                >
                  {exporting ? "导出中…" : "⤓ Markdown"}
                </button>
                <button
                  className="action-btn"
                  disabled={exporting}
                  onClick={() => exportCurrent("json")}
                >
                  ⤓ JSON
                </button>
              </div>
              {msgsLoading && (
                <div className="panel-loading">
                  <div className="spinner spinner-sm" />
                  <span>加载对话内容…</span>
                </div>
              )}
              {messages.map((m) => (
                <div
                  key={m.id}
                  id={`msg-${m.id}`}
                  className={`message ${m.role} ${highlightMsgId === m.id ? "highlighted" : ""}`}
                >
                  <div className="role">
                    <span className={`avatar ${m.role}`}>
                      {m.role === "user" ? "U" : m.role === "assistant" ? "AI" : m.role[0]?.toUpperCase()}
                    </span>
                    <span className="role-label">
                      {m.role === "user" ? "用户" : m.role === "assistant" ? "助手" : m.role}
                    </span>
                    {m.created_at_ms && (
                      <span className="msg-time">{formatTime(m.created_at_ms)}</span>
                    )}
                  </div>
                  {renderMessageContent(m)}
                </div>
              ))}
              {events.length > 0 && (
                <>
                  <div className="events-header">执行事件 ({events.length})</div>
                  {events.map((e) => (
                    <div key={e.id} className={`event ${e.event_type}`}>
                      <span className="event-type">{eventTypeLabel(e.event_type)}</span>
                      <span className="event-summary">{e.summary ?? ""}</span>
                    </div>
                  ))}
                </>
              )}
              <div className="knowledge-section">
                <button className="knowledge-btn" onClick={extractKnowledge}>
                  {knowledge ? "↻ 重新提取" : "🧠 知识提取"}
                </button>
                {knowledge && (
                  <div className="knowledge-result">
                    {knowledge.summary && (
                      <div className="knowledge-block summary">
                        <div className="knowledge-label">📖 摘要</div>
                        <div className="knowledge-text">{knowledge.summary}</div>
                      </div>
                    )}
                    {knowledge.decisions.length > 0 && (
                      <div className="knowledge-block decisions">
                        <div className="knowledge-label">🎯 决策（{knowledge.decisions.length}）</div>
                        {knowledge.decisions.map((d, i) => (
                          <div key={i} className="knowledge-item">• {d.decision}</div>
                        ))}
                      </div>
                    )}
                    {knowledge.todos.length > 0 && (
                      <div className="knowledge-block todos">
                        <div className="knowledge-label">📋 TODO（{knowledge.todos.length}）</div>
                        {knowledge.todos.map((t, i) => (
                          <div key={i} className="knowledge-item">• {t.text}</div>
                        ))}
                      </div>
                    )}
                    {knowledge.errors.length > 0 && (
                      <div className="knowledge-block errors">
                        <div className="knowledge-label">❌ 错误（{knowledge.errors.length}）</div>
                        {knowledge.errors.map((e, i) => (
                          <div key={i} className="knowledge-item">• {e.error}</div>
                        ))}
                      </div>
                    )}
                    {knowledge.commands.length > 0 && (
                      <div className="knowledge-block commands">
                        <div className="knowledge-label">⚙️ 命令（{knowledge.commands.length}）</div>
                        {knowledge.commands.map((c, i) => (
                          <div key={i} className="knowledge-item mono">• {c}</div>
                        ))}
                      </div>
                    )}
                    {knowledge.files.length > 0 && (
                      <div className="knowledge-block files">
                        <div className="knowledge-label">📄 涉及文件（{knowledge.files.length}）</div>
                        {knowledge.files.map((f, i) => (
                          <div key={i} className="knowledge-item mono">• {f.path}</div>
                        ))}
                      </div>
                    )}
                    <div className="knowledge-extractor">提取器：{knowledge.extractor}</div>
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="empty">选择一条会话查看详情</div>
          )}
        </div>
      </div>
      )}
    </div>
  );
}
