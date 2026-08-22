// 知识库页（第 6-10 轮优化）：单条复制/完成进度/空状态/快捷键/提取引导
// 第 11+ 轮：message_id 跳转（精确到消息）/ 全 6 类（加摘要、错误）/ Top 20 可点击 / 版本管理
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTime } from "./types";
import { usePager } from "./usePager";
import { MiniProgressShim } from "./progress";
import { showToast } from "./toast";
import { CardTitle } from "./CardTitle";
import { LoadingText } from "./EmptyState";

interface KbItem {
  text?: string;
  error?: string;
  summary?: string;
  /** 提取器判定的完成态：pending | done | stale（rule-v1 旧记录缺省 pending）。 */
  status?: string;
  conversation_id: string;
  title: string;
  message_id?: string;
}
interface KbCommand {
  cmd: string;
  count: number;
  last_conversation_id?: string;
}
interface KbFile {
  path: string;
  count: number;
  last_conversation_id?: string;
}
interface PendingItem {
  id: string;
  title: string;
  updated_at_ms: number;
  provider: string;
}
interface VersionItem {
  conversation_id: string;
  title: string;
  version: number;
  extracted_at: number;
  extractor: string;
}

interface KnowledgeBase {
  extracted: number;
  total_conversations: number;
  last_extract_ms: number;
  todos: KbItem[];
  decisions: KbItem[];
  errors: KbItem[];
  summaries: KbItem[];
  top_commands: KbCommand[];
  top_files: KbFile[];
  pending?: PendingItem[];
  versions?: VersionItem[];
}

interface PromptRow {
  message_id: string;
  conversation_id: string;
  text: string;
  title: string;
  created_at: number | null;
}

/** 提示词收藏（localStorage）。 */
export function loadPromptFavorites(): string[] {
  try {
    return JSON.parse(localStorage.getItem("ch-prompt-favs") ?? "[]") as string[];
  } catch {
    return [];
  }
}

export function togglePromptFavorite(id: string): string[] {
  const cur = loadPromptFavorites();
  const next = cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id];
  localStorage.setItem("ch-prompt-favs", JSON.stringify(next));
  return next;
}

/** 已完成 TODO（跨会话勾选，localStorage 持久）。 */
export function loadDoneTodos(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem("ch-todo-done") ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}

export function toggleDoneTodo(text: string): Set<string> {
  const cur = loadDoneTodos();
  if (cur.has(text)) {
    cur.delete(text);
  } else {
    cur.add(text);
  }
  localStorage.setItem("ch-todo-done", JSON.stringify([...cur]));
  return new Set(cur);
}

/** 「复活」覆盖集：提取器判 done/stale 但用户手动改回未完成的条目。 */
export function loadUndoneTodos(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem("ch-todo-undone") ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}

function persistSet(key: string, set: Set<string>): Set<string> {
  localStorage.setItem(key, JSON.stringify([...set]));
  return new Set(set);
}

/** 最终完成态 =（提取器判 done/stale 或手动勾完成）且未被「复活」覆盖。 */
export function resolveTodoDone(
  text: string,
  status: string | undefined,
  manualDone: Set<string> = loadDoneTodos(),
  manualUndone: Set<string> = loadUndoneTodos(),
): boolean {
  const resolvedByExtraction = status === "done" || status === "stale";
  return (resolvedByExtraction || manualDone.has(text)) && !manualUndone.has(text);
}

/** 隐藏已完成开关（默认开：LLM 会话里「说过就做完」的计划不再刷屏）。 */
export function loadHideDone(): boolean {
  return localStorage.getItem("ch-todo-hide-done") !== "0";
}

/** 知识库 → Markdown 纪要（导出用）。 */
export function knowledgeBaseToMarkdown(kb: {
  todos?: { text?: string; status?: string; title: string }[];
  decisions?: { text?: string; title: string }[];
  top_commands?: { cmd: string; count: number }[];
  top_files?: { path: string; count: number }[];
}): string {
  const lines: string[] = ["# 知识库纪要", ""];
  if ((kb.decisions ?? []).length > 0) {
    lines.push("## 决策", ...(kb.decisions ?? []).map((d) => `- ${d.text ?? ""}（${d.title}）`), "");
  }
  if ((kb.todos ?? []).length > 0) {
    lines.push(
      "## TODO",
      ...(kb.todos ?? []).map((t) =>
        `- [${resolveTodoDone(t.text ?? "", t.status) ? "x" : " "}] ${t.text ?? ""}（${t.title}）`,
      ),
      "",
    );
  }
  if ((kb.top_commands ?? []).length > 0) {
    lines.push("## 常用命令", ...(kb.top_commands ?? []).map((c) => `- \`${c.cmd}\` ×${c.count}`), "");
  }
  if ((kb.top_files ?? []).length > 0) {
    lines.push("## 高频文件", ...(kb.top_files ?? []).map((f) => `- ${f.path} ×${f.count}`), "");
  }
  return lines.join("\n");
}

/** 相对时间（"3 天前"）。 */
export function relativeTime(ms: number): string {
  if (!ms) return "从未";
  const diff = Date.now() - ms;
  const day = 86400000;
  if (diff < 3600000) return "刚刚";
  if (diff < day) return `${Math.floor(diff / 3600000)} 小时前`;
  if (diff < 30 * day) return `${Math.floor(diff / day)} 天前`;
  return formatTime(ms);
}

type KbTab = "todos" | "decisions" | "summaries" | "errors" | "prompts";

export default function KnowledgeView({ onJump }: { onJump: (conversationId: string, messageId?: string) => void }) {
  const [kb, setKb] = useState<KnowledgeBase | null>(null);
  const [prompts, setPrompts] = useState<PromptRow[]>([]);
  const [favs, setFavs] = useState<string[]>(loadPromptFavorites);
  const [doneTodos, setDoneTodos] = useState<Set<string>>(loadDoneTodos);
  const [undoneTodos, setUndoneTodos] = useState<Set<string>>(loadUndoneTodos);
  const [hideDone, setHideDone] = useState<boolean>(loadHideDone);
  const [extracting, setExtracting] = useState(false);
  const [tab, setTab] = useState<KbTab>("todos");
  const [search, setSearch] = useState("");
  const [modalKind, setModalKind] = useState<null | "pending" | "versions">(null);
  const searchRef = useRef<HTMLInputElement>(null);

  // 快捷键 ⌘F / Ctrl+F 聚焦搜索框
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
      if (e.key === "Escape" && modalKind) setModalKind(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [modalKind]);

  /** 单条复制（hover 出现的 📋 按钮用）。 */
  const copyText = async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      showToast(`✓ 已复制 ${label}`, "info");
    } catch {
      showToast("剪贴板不可用", "error");
    }
  };

  const load = async () => {
    try {
      // null（IPC 异常/空库）归一为空 KB：避免页面永远停在「加载中」
      setKb((await invoke<KnowledgeBase | null>("knowledge_base_list", {})) ?? {
        extracted: 0, total_conversations: 0, last_extract_ms: 0,
        todos: [], decisions: [], errors: [], summaries: [], top_commands: [], top_files: [],
      });
      setPrompts(((await invoke<{ prompts: PromptRow[] }>("recent_user_prompts", { limit: 100 })) ?? { prompts: [] }).prompts ?? []);
    } catch { /* 空库静默 */ }
  };
  // 挂载时拉取知识库数据（effect 数据加载模式：load 内含 setState，有意保留）
  // eslint-disable-next-line react-hooks/set-state-in-effect
  useEffect(() => { load(); }, []);

  const runExtract = async (force = false) => {
    setExtracting(true);
    try {
      await invoke("knowledge_extract_all", { force });
      await invoke("app_setting_set", { key: "last_knowledge_extract_ms", value: String(Date.now()) }).catch(() => {});
      await load();
    } catch { /* 失败下次再试 */ }
    setExtracting(false);
  };

  // 搜索过滤（当前 tab 生效；额外对命令/文件生效——P1-A4）；TODO 先过「隐藏已完成」
  const todoDone = (t: KbItem) => resolveTodoDone(t.text ?? "", t.status, doneTodos, undoneTodos);
  const filtered = useMemo(() => {
    const empty = {
      todos: [] as KbItem[],
      decisions: [] as KbItem[],
      summaries: [] as KbItem[],
      errors: [] as KbItem[],
      topCommands: [] as KbCommand[],
      topFiles: [] as KbFile[],
    };
    if (!kb) {
      return empty;
    }
    const baseTodos = hideDone ? kb.todos.filter((t) => !todoDone(t)) : kb.todos;
    const q = search.trim().toLowerCase();
    if (!q) {
      return {
        ...empty,
        todos: baseTodos,
        decisions: kb.decisions,
        summaries: kb.summaries ?? [],
        errors: kb.errors ?? [],
        topCommands: kb.top_commands,
        topFiles: kb.top_files,
      };
    }
    return {
      ...empty,
      todos: baseTodos.filter((t) => (t.text ?? "").toLowerCase().includes(q) || t.title.toLowerCase().includes(q)),
      decisions: kb.decisions.filter((d) => (d.text ?? "").toLowerCase().includes(q) || d.title.toLowerCase().includes(q)),
      summaries: (kb.summaries ?? []).filter((s) => (s.summary ?? s.text ?? "").toLowerCase().includes(q) || s.title.toLowerCase().includes(q)),
      errors: (kb.errors ?? []).filter((e) => (e.error ?? e.text ?? "").toLowerCase().includes(q) || e.title.toLowerCase().includes(q)),
      topCommands: kb.top_commands.filter((c) => c.cmd.toLowerCase().includes(q)),
      topFiles: kb.top_files.filter((f) => f.path.toLowerCase().includes(q)),
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kb, search, hideDone, doneTodos, undoneTodos]);

  const todoPager = usePager(filtered.todos, 50);
  const decisionPager = usePager(filtered.decisions, 50);
  const summaryPager = usePager(filtered.summaries, 50);
  const errorPager = usePager(filtered.errors, 50);
  // 收藏与最近列表分离：favPrompts 收藏区，promptPager 仅看非收藏部分（避免重复渲染 — P1-A6）
  const favPrompts = prompts.filter((p) => favs.includes(p.message_id));
  const nonFavPrompts = prompts.filter((p) => !favs.includes(p.message_id));
  const promptPager = usePager(
    search.trim()
      ? nonFavPrompts.filter((p) => p.text.toLowerCase().includes(search.trim().toLowerCase()))
      : nonFavPrompts,
    50,
  );
  // 搜索词变化时把 pager 拉回首页，避免列表缩短后卡在越界页
  useEffect(() => {
    todoPager.reset();
    decisionPager.reset();
    summaryPager.reset();
    errorPager.reset();
    promptPager.reset();
    /* eslint-disable-next-line react-hooks/exhaustive-deps */
  }, [search]);

  const pagerBar = (pg: { page: number; totalPages: number; total: number; needed: boolean; prev: () => void; next: () => void }) =>
    pg.needed ? (
      <div className="pager">
        <button className="pager-btn" onClick={pg.prev} disabled={pg.page === 0}>‹ 上一页</button>
        <span className="pager-info">{pg.page + 1} / {pg.totalPages} 页 · 共 {pg.total} 条</span>
        <button className="pager-btn" onClick={pg.next} disabled={pg.page >= pg.totalPages - 1}>下一页 ›</button>
      </div>
    ) : null;

  const empty = kb && kb.extracted === 0;
  // 完成态 = 提取器判定（done/stale）∪ 手动勾选 − 手动「复活」
  const todoDoneAll = (t: KbItem) => resolveTodoDone(t.text ?? "", t.status, doneTodos, undoneTodos);
  const openTodos = kb ? kb.todos.filter((t) => !todoDoneAll(t)).length : 0;
  const doneCount = kb ? kb.todos.length - openTodos : 0;
  const doneRatio = kb && kb.todos.length > 0 ? (doneCount / kb.todos.length) * 100 : 0;

  /** 勾选往返：未完成→手动完成；提取判完成→「复活」（两者都可再点回）。 */
  const toggleTodo = (t: KbItem) => {
    const text = t.text ?? "";
    const resolvedByExtraction = t.status === "done" || t.status === "stale";
    if (todoDoneAll(t)) {
      if (resolvedByExtraction) {
        setUndoneTodos(persistSet("ch-todo-undone", new Set([...undoneTodos, text])));
      } else {
        setDoneTodos(persistSet("ch-todo-done", new Set([...doneTodos].filter((x) => x !== text))));
      }
    } else {
      setDoneTodos(persistSet("ch-todo-done", new Set([...doneTodos, text])));
      if (undoneTodos.has(text)) {
        setUndoneTodos(persistSet("ch-todo-undone", new Set([...undoneTodos].filter((x) => x !== text))));
      }
    }
  };

  // 通用"标题 + 来源"行渲染：todos/decisions/summaries/errors 共用
  const kbItemRow = (
    item: KbItem,
    i: number,
    icon: string,
    copyLabel: string,
    bodyText: string,
    done?: { is: boolean; toggle: () => void },
  ) => {
    return (
      <div key={i} className={`kb-item ${done?.is ? "done" : ""}`}>
        {done ? (
          <span className="todo-check" title={done.is ? "标记未完成" : "标记已完成"} onClick={done.toggle}>
            {done.is ? "☑" : "☐"}
          </span>
        ) : (
          <span className="todo-check" aria-hidden style={{ visibility: "hidden" }}>☐</span>
        )}
        <span className="kb-text" title={bodyText}>
          {icon} {bodyText}
        </span>
        <button
          className="kb-src"
          onClick={() => onJump(item.conversation_id, item.message_id)}
          title={item.message_id ? "跳转到对应消息" : "跳转到会话"}
        >
          {item.title || "(无标题)"}
        </button>
        <button className="kb-copy" title={`复制${copyLabel}文本`} onClick={() => copyText(bodyText, copyLabel)}>📋</button>
      </div>
    );
  };

  return (
    <div className="knowledge-page">
      <div className="ops-card">
        <CardTitle icon="library" sub={kb ? (
          <>
            <button
              className="link-like"
              style={{ background: "none", border: "none", padding: 0, color: "inherit", cursor: "pointer", textDecoration: "underline" }}
              onClick={() => setModalKind("versions")}
              title="查看已提取会话的版本历史"
            >已提取 {kb.extracted}</button>
            {" / "}
            <span>{kb.total_conversations} 会话</span>
            {" · 未提取 "}
            <button
              className="link-like"
              style={{ background: "none", border: "none", padding: 0, color: "inherit", cursor: "pointer", textDecoration: "underline" }}
              onClick={() => setModalKind("pending")}
              title="查看尚未提取的会话清单"
            >{(kb.pending ?? []).length}</button>
            {" · 上次 "}{relativeTime(kb.last_extract_ms)}
          </>
        ) : <LoadingText text="正在加载知识库…" />} trailing={<>
          <button className="action-btn" disabled={extracting}
            onClick={() => runExtract(false)}>
            {extracting ? "提取中…" : kb && kb.extracted > 0 ? "↻ 提取新会话" : "▶ 首次提取全部"}
          </button>
          {kb && kb.extracted > 0 && kb.extracted < kb.total_conversations && (
            <button className="action-btn" disabled={extracting}
              onClick={() => runExtract(true)} title="重新提取全部（含已提取）">重提全部</button>
          )}
          {kb && kb.extracted > 0 && (
            <button className="action-btn"
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(knowledgeBaseToMarkdown(kb));
                  showToast("✓ 知识库纪要已复制到剪贴板", "info");
                } catch { showToast("剪贴板不可用", "error"); }
              }} title="全部知识导出为 Markdown 纪要（含 TODO 完成状态）">导出纪要</button>
          )}
        </>}>知识库</CardTitle>
        <MiniProgressShim show={extracting} label="知识提取" />
        {empty && (
          <div className="ops-table-empty">
            还没有积累任何知识——点击「首次提取全部」，把所有会话中的决策 / TODO / 命令 / 文件沉淀为可检索的知识库。
          </div>
        )}
        {kb && kb.extracted > 0 && (
          <>
            <div className="kb-grid">
              <div className="kb-stat"><b>{openTodos}</b><span>未完成 TODO</span></div>
              <div className="kb-stat"><b>{kb.decisions.length}</b><span>决策</span></div>
              <div className="kb-stat"><b>{(kb.summaries ?? []).length}</b><span>摘要</span></div>
              <div className="kb-stat"><b>{(kb.errors ?? []).length}</b><span>错误</span></div>
              <div className="kb-stat"><b>{kb.top_commands.length}</b><span>常用命令</span></div>
              <div className="kb-stat"><b>{kb.top_files.length}</b><span>高频文件</span></div>
            </div>
            {kb.todos.length > 0 && (
              <div className="kb-progress" title={`已了结 ${doneCount} / 共 ${kb.todos.length} 条 TODO（完成或过期）`}>
                <div className="kb-progress-bar"><div className="kb-progress-fill" style={{ width: `${doneRatio}%` }} /></div>
                <span className="kb-progress-label">✅ {doneCount} / {kb.todos.length} TODO 已了结（完成或过期）· {doneRatio.toFixed(0)}%</span>
              </div>
            )}
          </>
        )}
      </div>

      {kb && kb.extracted > 0 && (
        <>
          <div className="ops-card">
            <div className="scope-bar" style={{ alignItems: "center" }}>
              {([
                ["todos", `TODO（${filtered.todos.length}）`],
                ["decisions", `决策（${filtered.decisions.length}）`],
                ["summaries", `摘要（${filtered.summaries.length}）`],
                ["errors", `错误（${filtered.errors.length}）`],
                ["prompts", `我的提问（收藏 ${favPrompts.length}）`],
              ] as const).map(([k, label]) => (
                <button key={k} className={`scope-chip ${tab === k ? "active" : ""}`} onClick={() => setTab(k)}>{label}</button>
              ))}
              <label
                className="scope-chip"
                style={{ cursor: "pointer", display: "inline-flex", alignItems: "center", gap: 4, userSelect: "none" }}
                title="提取器判定「已完成 / 过期」的条目默认隐藏——点勾选框可复活单条"
              >
                <input
                  type="checkbox"
                  checked={hideDone}
                  onChange={(e) => {
                    setHideDone(e.target.checked);
                    localStorage.setItem("ch-todo-hide-done", e.target.checked ? "1" : "0");
                  }}
                />
                隐藏已完成
              </label>
              <input
                ref={searchRef}
                className="settings-confirm-input"
                style={{ marginLeft: "auto", width: 180, fontSize: 12 }}
                placeholder="🔍 搜索知识条目…（⌘F）"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            {tab === "todos" && (
              <div className="kb-list">
                {todoPager.slice.length === 0 && (
                  <div className="ops-table-empty">
                    {search
                      ? "🔍 无匹配条目"
                      : kb?.todos.length === 0
                        ? "🎯 当前没有提取到 TODO —— 多用 Agent 处理任务后再提取"
                        : hideDone
                          ? "✅ 没有未完成的 TODO —— 关闭「隐藏已完成」可查看全部历史条目"
                          : "✅ 所有 TODO 都已勾选完成"}
                  </div>
                )}
                {todoPager.slice.map((t, i) => {
                  const text = t.text ?? "";
                  const stale = t.status === "stale";
                  return kbItemRow(t, i, stale ? "⊘" : "", "TODO", text, {
                    is: todoDone(t),
                    toggle: () => toggleTodo(t),
                  });
                })}
                {pagerBar(todoPager)}
              </div>
            )}
            {tab === "decisions" && (
              <div className="kb-list">
                {decisionPager.slice.length === 0 && (
                  <div className="ops-table-empty">
                    {search ? "🔍 无匹配条目" : "🎯 还没有决策记录 —— 让 Agent 多做方案对比、选型讨论"}
                  </div>
                )}
                {decisionPager.slice.map((d, i) => kbItemRow(d, i, "🎯", "决策", d.text ?? ""))}
                {pagerBar(decisionPager)}
              </div>
            )}
            {tab === "summaries" && (
              <div className="kb-list">
                {summaryPager.slice.length === 0 && (
                  <div className="ops-table-empty">
                    {search ? "🔍 无匹配条目" : "📖 还没有摘要 —— 多数会话都已自动生成主题/问题/要点摘要"}
                  </div>
                )}
                {summaryPager.slice.map((s, i) => kbItemRow(s, i, "📖", "摘要", s.summary ?? s.text ?? ""))}
                {pagerBar(summaryPager)}
              </div>
            )}
            {tab === "errors" && (
              <div className="kb-list">
                {errorPager.slice.length === 0 && (
                  <div className="ops-table-empty">
                    {search ? "🔍 无匹配条目" : "❌ 还没有错误记录 —— 多数正常会话不会触发"}
                  </div>
                )}
                {errorPager.slice.map((e, i) => kbItemRow(e, i, "❌", "错误", e.error ?? e.text ?? ""))}
                {pagerBar(errorPager)}
              </div>
            )}
            {tab === "prompts" && (
              <div className="kb-list">
                {favPrompts.length === 0 && (
                  <div className="ops-table-empty">还没有收藏的提问 —— 在下方点 ☆ 把好用的 prompt 沉淀下来</div>
                )}
                {favPrompts.map((p) => (
                  <div key={`fav-${p.message_id}`} className="kb-item">
                    <span className="fav-toggle on" onClick={() => setFavs(togglePromptFavorite(p.message_id))}>★</span>
                    <span className="kb-text prompt" title={p.text}>{p.text}</span>
                    <button
                      className="kb-src"
                      onClick={() => onJump(p.conversation_id, p.message_id)}
                      title="跳转到该提问"
                    >
                      {p.title}
                    </button>
                    <button className="kb-copy" title="复制 prompt" onClick={() => copyText(p.text, "prompt")}>📋</button>
                  </div>
                ))}
                {promptPager.slice.length > 0 && <div className="automation-sub">最近提问（点 ☆ 收藏）</div>}
                {promptPager.slice.map((p) => (
                  <div key={p.message_id} className="kb-item">
                    <span className={`fav-toggle ${favs.includes(p.message_id) ? "on" : ""}`}
                      onClick={() => setFavs(togglePromptFavorite(p.message_id))}>{favs.includes(p.message_id) ? "★" : "☆"}</span>
                    <span className="kb-text prompt" title={p.text}>{p.text}</span>
                    <button
                      className="kb-src"
                      onClick={() => onJump(p.conversation_id, p.message_id)}
                      title="跳转到该提问"
                    >
                      {p.title}
                    </button>
                    <button className="kb-copy" title="复制 prompt" onClick={() => copyText(p.text, "prompt")}>📋</button>
                  </div>
                ))}
                {pagerBar(promptPager)}
              </div>
            )}
          </div>

          <div className="ops-card">
            <CardTitle icon="terminal">常用命令 Top 20</CardTitle>
            {filtered.topCommands.length === 0 ? <div className="ops-table-empty">{search ? "无匹配" : "无数据"}</div> : (
              <div className="kb-list">
                {filtered.topCommands.map((c, i) => (
                  <div key={i} className="kb-item">
                    <span className="kb-count">{c.count}×</span>
                    <span className="kb-text mono">{c.cmd}</span>
                    {c.last_conversation_id && (
                      <button
                        className="kb-src"
                        onClick={() => onJump(c.last_conversation_id!)}
                        title="跳转到最近一次使用此命令的会话"
                      >
                        跳到会话 ↗
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="ops-card">
            <CardTitle icon="file">高频文件 Top 20</CardTitle>
            {filtered.topFiles.length === 0 ? <div className="ops-table-empty">{search ? "无匹配" : "无数据"}</div> : (
              <div className="kb-list">
                {filtered.topFiles.map((f, i) => (
                  <div key={i} className="kb-item">
                    <span className="kb-count">{f.count}×</span>
                    <span className="kb-text mono">{f.path}</span>
                    {f.last_conversation_id && (
                      <button
                        className="kb-src"
                        onClick={() => onJump(f.last_conversation_id!)}
                        title="跳转到最近一次涉及此文件的会话"
                      >
                        跳到会话 ↗
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      )}

      {/* P1-E5：版本管理弹窗（pending / versions） */}
      {modalKind && kb && (
        <div className="settings-backdrop" onClick={() => setModalKind(null)}>
          <div className="settings-modal" style={{ maxWidth: 640 }} onClick={(e) => e.stopPropagation()}>
            <div className="settings-header">
              <h2>
                {modalKind === "pending" ? "⏳ 未提取的会话" : "🗂 提取版本历史"}
                <span className="ops-card-sub" style={{ marginLeft: 8 }}>
                  {modalKind === "pending"
                    ? `共 ${(kb.pending ?? []).length} 条`
                    : `共 ${(kb.versions ?? []).length} 条`}
                </span>
              </h2>
              <button className="settings-close" onClick={() => setModalKind(null)}>✕</button>
            </div>
            <div className="settings-body" style={{ padding: 12 }}>
              {modalKind === "pending" && (
                (kb.pending ?? []).length === 0
                  ? <div className="ops-table-empty">所有会话都已提取</div>
                  : (kb.pending ?? []).map((p) => (
                      <div key={p.id} className="kb-item">
                        <span className="badge source">{p.provider}</span>
                        <span className="kb-text" title={p.title}>{p.title}</span>
                        <span className="kb-src" style={{ color: "var(--text-muted)" }}>{relativeTime(p.updated_at_ms)}</span>
                        <button className="kb-src" onClick={() => onJump(p.id)} title="跳转到该会话">跳到会话 ↗</button>
                      </div>
                    ))
              )}
              {modalKind === "versions" && (
                (kb.versions ?? []).length === 0
                  ? <div className="ops-table-empty">暂无提取记录</div>
                  : (kb.versions ?? []).map((v) => (
                      <div key={v.conversation_id} className="kb-item">
                        <span className="badge source">v{v.version}</span>
                        <span className="kb-text" title={v.title}>{v.title}</span>
                        <span className="kb-src" style={{ color: "var(--text-muted)" }}>{v.extractor} · {relativeTime(v.extracted_at)}</span>
                        <button className="kb-src" onClick={() => onJump(v.conversation_id)} title="跳转到该会话">跳到会话 ↗</button>
                      </div>
                    ))
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
