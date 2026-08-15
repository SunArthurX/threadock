// 知识库页（5 轮优化版）：分页/搜索/TODO完成勾选/导出纪要/提取引导
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTime } from "./types";
import { usePager } from "./usePager";
import { MiniProgressShim } from "./progress";
import { showToast } from "./toast";

interface KnowledgeBase {
  extracted: number;
  total_conversations: number;
  last_extract_ms: number;
  todos: { text: string; conversation_id: string; title: string }[];
  decisions: { text: string; conversation_id: string; title: string }[];
  top_commands: { cmd: string; count: number }[];
  top_files: { path: string; count: number }[];
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

/** 知识库 → Markdown 纪要（导出用）。 */
export function knowledgeBaseToMarkdown(kb: {
  todos: { text: string; title: string }[];
  decisions: { text: string; title: string }[];
  top_commands: { cmd: string; count: number }[];
  top_files: { path: string; count: number }[];
}): string {
  const done = loadDoneTodos();
  const lines: string[] = ["# 知识库纪要", ""];
  if (kb.decisions.length > 0) {
    lines.push("## 决策", ...kb.decisions.map((d) => `- ${d.text}（${d.title}）`), "");
  }
  if (kb.todos.length > 0) {
    lines.push("## TODO", ...kb.todos.map((t) => `- [${done.has(t.text) ? "x" : " "}] ${t.text}（${t.title}）`), "");
  }
  if (kb.top_commands.length > 0) {
    lines.push("## 常用命令", ...kb.top_commands.map((c) => `- \`${c.cmd}\` ×${c.count}`), "");
  }
  if (kb.top_files.length > 0) {
    lines.push("## 高频文件", ...kb.top_files.map((f) => `- ${f.path} ×${f.count}`), "");
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

export default function KnowledgeView({ onJump }: { onJump: (conversationId: string) => void }) {
  const [kb, setKb] = useState<KnowledgeBase | null>(null);
  const [prompts, setPrompts] = useState<PromptRow[]>([]);
  const [favs, setFavs] = useState<string[]>(loadPromptFavorites);
  const [doneTodos, setDoneTodos] = useState<Set<string>>(loadDoneTodos);
  const [extracting, setExtracting] = useState(false);
  const [tab, setTab] = useState<"todos" | "decisions" | "prompts">("todos");
  const [search, setSearch] = useState("");

  const load = async () => {
    try {
      setKb(await invoke<KnowledgeBase>("knowledge_base_list", {}));
      setPrompts((await invoke<{ prompts: PromptRow[] }>("recent_user_prompts", { limit: 100 })).prompts);
    } catch { /* 空库静默 */ }
  };
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

  // 搜索过滤（当前 tab 生效）
  const filtered = useMemo(() => {
    if (!kb) return { todos: [], decisions: [] };
    const q = search.trim().toLowerCase();
    if (!q) return kb;
    return {
      todos: kb.todos.filter((t) => t.text.toLowerCase().includes(q) || t.title.toLowerCase().includes(q)),
      decisions: kb.decisions.filter((d) => d.text.toLowerCase().includes(q) || d.title.toLowerCase().includes(q)),
    };
  }, [kb, search]);

  const todoPager = usePager(filtered.todos, 50);
  const decisionPager = usePager(filtered.decisions, 50);
  const promptPager = usePager(
    search.trim()
      ? prompts.filter((p) => p.text.toLowerCase().includes(search.trim().toLowerCase()))
      : prompts,
    50,
  );
  const favPrompts = prompts.filter((p) => favs.includes(p.message_id));

  const pagerBar = (pg: { page: number; totalPages: number; total: number; needed: boolean; prev: () => void; next: () => void }) =>
    pg.needed ? (
      <div className="pager">
        <button className="pager-btn" onClick={pg.prev} disabled={pg.page === 0}>‹ 上一页</button>
        <span className="pager-info">{pg.page + 1} / {pg.totalPages} 页 · 共 {pg.total} 条</span>
        <button className="pager-btn" onClick={pg.next} disabled={pg.page >= pg.totalPages - 1}>下一页 ›</button>
      </div>
    ) : null;

  const empty = kb && kb.extracted === 0;
  const openTodos = kb ? kb.todos.filter((t) => !doneTodos.has(t.text)).length : 0;

  return (
    <div className="knowledge-page">
      <div className="ops-card">
        <div className="ops-card-title">
          📚 知识库
          <span className="ops-card-sub">
            {kb ? `已提取 ${kb.extracted}/${kb.total_conversations} 会话 · 上次 ${relativeTime(kb.last_extract_ms)}` : "加载中…"}
          </span>
          <button className="action-btn" style={{ marginLeft: "auto", fontSize: 11 }} disabled={extracting}
            onClick={() => runExtract(false)}>
            {extracting ? "提取中…" : kb && kb.extracted > 0 ? "↻ 提取新会话" : "▶ 首次提取全部"}
          </button>
          {kb && kb.extracted > 0 && kb.extracted < kb.total_conversations && (
            <button className="action-btn" style={{ fontSize: 11 }} disabled={extracting}
              onClick={() => runExtract(true)} title="重新提取全部（含已提取）">重提全部</button>
          )}
          {kb && kb.extracted > 0 && (
            <button className="action-btn" style={{ fontSize: 11 }}
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(knowledgeBaseToMarkdown(kb));
                  showToast("✓ 知识库纪要已复制到剪贴板", "info");
                } catch { showToast("剪贴板不可用", "error"); }
              }} title="全部知识导出为 Markdown 纪要（含 TODO 完成状态）">⧉ 导出纪要</button>
          )}
        </div>
        <MiniProgressShim show={extracting} label="知识提取" />
        {empty && (
          <div className="ops-table-empty">
            还没有积累任何知识——点击「首次提取全部」，把所有会话中的决策 / TODO / 命令 / 文件沉淀为可检索的知识库。
          </div>
        )}
        {kb && kb.extracted > 0 && (
          <div className="kb-grid">
            <div className="kb-stat"><b>{openTodos}</b><span>未完成 TODO</span></div>
            <div className="kb-stat"><b>{kb.decisions.length}</b><span>决策</span></div>
            <div className="kb-stat"><b>{kb.top_commands.length}</b><span>常用命令</span></div>
            <div className="kb-stat"><b>{kb.top_files.length}</b><span>高频文件</span></div>
          </div>
        )}
      </div>

      {kb && kb.extracted > 0 && (
        <>
          <div className="ops-card">
            <div className="scope-bar" style={{ alignItems: "center" }}>
              {([["todos", `TODO（${filtered.todos.length}）`], ["decisions", `决策（${filtered.decisions.length}）`], ["prompts", `我的提问（收藏 ${favPrompts.length}）`]] as const).map(([k, label]) => (
                <button key={k} className={`scope-chip ${tab === k ? "active" : ""}`} onClick={() => setTab(k)}>{label}</button>
              ))}
              <input
                className="settings-confirm-input"
                style={{ marginLeft: "auto", width: 180, fontSize: 12 }}
                placeholder="🔍 搜索知识条目…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            {tab === "todos" && (
              <div className="kb-list">
                {todoPager.slice.length === 0 && <div className="ops-table-empty">{search ? "无匹配条目" : "无 TODO 记录"}</div>}
                {todoPager.slice.map((t, i) => {
                  const done = doneTodos.has(t.text);
                  return (
                    <div key={i} className={`kb-item ${done ? "done" : ""}`}>
                      <span className="todo-check" title={done ? "标记未完成" : "标记已完成"}
                        onClick={() => setDoneTodos(toggleDoneTodo(t.text))}>{done ? "☑" : "☐"}</span>
                      <span className="kb-text">{t.text}</span>
                      <span className="kb-src" onClick={() => onJump(t.conversation_id)} title="跳转到会话">{t.title || "(无标题)"}</span>
                    </div>
                  );
                })}
                {pagerBar(todoPager)}
              </div>
            )}
            {tab === "decisions" && (
              <div className="kb-list">
                {decisionPager.slice.length === 0 && <div className="ops-table-empty">{search ? "无匹配条目" : "无决策记录"}</div>}
                {decisionPager.slice.map((d, i) => (
                  <div key={i} className="kb-item">
                    <span className="kb-text">🎯 {d.text}</span>
                    <span className="kb-src" onClick={() => onJump(d.conversation_id)} title="跳转到会话">{d.title || "(无标题)"}</span>
                  </div>
                ))}
                {pagerBar(decisionPager)}
              </div>
            )}
            {tab === "prompts" && (
              <div className="kb-list">
                {favPrompts.length === 0 && <div className="ops-table-empty">还没有收藏的提问——在下方点 ☆ 收藏好用的 prompt</div>}
                {favPrompts.map((p) => (
                  <div key={p.message_id} className="kb-item">
                    <span className="fav-toggle on" onClick={() => setFavs(togglePromptFavorite(p.message_id))}>★</span>
                    <span className="kb-text prompt">{p.text}</span>
                    <span className="kb-src">{p.title}</span>
                  </div>
                ))}
                {promptPager.slice.length > 0 && <div className="automation-sub">最近提问（点 ☆ 收藏）</div>}
                {promptPager.slice.map((p) => (
                  <div key={p.message_id} className="kb-item">
                    <span className={`fav-toggle ${favs.includes(p.message_id) ? "on" : ""}`}
                      onClick={() => setFavs(togglePromptFavorite(p.message_id))}>{favs.includes(p.message_id) ? "★" : "☆"}</span>
                    <span className="kb-text prompt">{p.text}</span>
                    <span className="kb-src">{p.title}</span>
                  </div>
                ))}
                {pagerBar(promptPager)}
              </div>
            )}
          </div>

          <div className="ops-card">
            <div className="ops-card-title">⚙ 常用命令 Top 20</div>
            {kb.top_commands.length === 0 ? <div className="ops-table-empty">无数据</div> : (
              <div className="kb-list">
                {kb.top_commands.map((c, i) => (
                  <div key={i} className="kb-item">
                    <span className="kb-count">{c.count}×</span>
                    <span className="kb-text mono">{c.cmd}</span>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="ops-card">
            <div className="ops-card-title">📄 高频文件 Top 20</div>
            {kb.top_files.length === 0 ? <div className="ops-table-empty">无数据</div> : (
              <div className="kb-list">
                {kb.top_files.map((f, i) => (
                  <div key={i} className="kb-item">
                    <span className="kb-count">{f.count}×</span>
                    <span className="kb-text mono">{f.path}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
