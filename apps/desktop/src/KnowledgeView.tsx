// 知识库页：跨会话知识聚合（TODO/决策/命令/文件）+ 我的提问（提示词库）
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTime } from "./types";
import { MiniProgressShim } from "./progress";

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

export default function KnowledgeView({ onJump }: { onJump: (conversationId: string) => void }) {
  const [kb, setKb] = useState<KnowledgeBase | null>(null);
  const [prompts, setPrompts] = useState<PromptRow[]>([]);
  const [favs, setFavs] = useState<string[]>(loadPromptFavorites);
  const [extracting, setExtracting] = useState(false);
  const [tab, setTab] = useState<"todos" | "decisions" | "prompts">("todos");

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

  const empty = kb && kb.extracted === 0;
  const favPrompts = prompts.filter((p) => favs.includes(p.message_id));

  return (
    <div className="knowledge-page">
      <div className="ops-card">
        <div className="ops-card-title">
          📚 知识库
          <span className="ops-card-sub">
            {kb ? `已提取 ${kb.extracted}/${kb.total_conversations} 会话 · 上次 ${formatTime(kb.last_extract_ms) || "从未"}` : "加载中…"}
          </span>
          <button className="action-btn" style={{ marginLeft: "auto", fontSize: 11 }} disabled={extracting}
            onClick={() => runExtract(false)}>
            {extracting ? "提取中…" : kb && kb.extracted > 0 ? "↻ 提取新会话" : "▶ 首次提取全部"}
          </button>
          {kb && kb.extracted > 0 && kb.extracted < kb.total_conversations && (
            <button className="action-btn" style={{ fontSize: 11 }} disabled={extracting}
              onClick={() => runExtract(true)} title="重新提取全部（含已提取）">重提全部</button>
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
            <div className="kb-stat"><b>{kb.todos.length}</b><span>TODO</span></div>
            <div className="kb-stat"><b>{kb.decisions.length}</b><span>决策</span></div>
            <div className="kb-stat"><b>{kb.top_commands.length}</b><span>常用命令</span></div>
            <div className="kb-stat"><b>{kb.top_files.length}</b><span>高频文件</span></div>
          </div>
        )}
      </div>

      {kb && kb.extracted > 0 && (
        <>
          <div className="ops-card">
            <div className="scope-bar">
              {([["todos", `TODO（${kb.todos.length}）`], ["decisions", `决策（${kb.decisions.length}）`], ["prompts", `我的提问（收藏 ${favPrompts.length}）`]] as const).map(([k, label]) => (
                <button key={k} className={`scope-chip ${tab === k ? "active" : ""}`} onClick={() => setTab(k)}>{label}</button>
              ))}
            </div>
            {tab === "todos" && (
              <div className="kb-list">
                {kb.todos.length === 0 && <div className="ops-table-empty">无 TODO 记录</div>}
                {kb.todos.map((t, i) => (
                  <div key={i} className="kb-item">
                    <span className="kb-text">☐ {t.text}</span>
                    <span className="kb-src" onClick={() => onJump(t.conversation_id)} title="跳转到会话">{t.title || "(无标题)"}</span>
                  </div>
                ))}
              </div>
            )}
            {tab === "decisions" && (
              <div className="kb-list">
                {kb.decisions.length === 0 && <div className="ops-table-empty">无决策记录</div>}
                {kb.decisions.map((d, i) => (
                  <div key={i} className="kb-item">
                    <span className="kb-text">🎯 {d.text}</span>
                    <span className="kb-src" onClick={() => onJump(d.conversation_id)} title="跳转到会话">{d.title || "(无标题)"}</span>
                  </div>
                ))}
              </div>
            )}
            {tab === "prompts" && (
              <div className="kb-list">
                {favPrompts.length === 0 && <div className="ops-table-empty">还没有收藏的提问——在下方最近提问里点 ☆ 收藏好用的 prompt</div>}
                {favPrompts.map((p) => (
                  <div key={p.message_id} className="kb-item">
                    <span className="fav-toggle on" onClick={() => setFavs(togglePromptFavorite(p.message_id))}>★</span>
                    <span className="kb-text prompt">{p.text}</span>
                    <span className="kb-src">{p.title}</span>
                  </div>
                ))}
                {prompts.length > 0 && <div className="automation-sub">最近提问（点 ☆ 收藏）</div>}
                {prompts.map((p) => (
                  <div key={p.message_id} className="kb-item">
                    <span className={`fav-toggle ${favs.includes(p.message_id) ? "on" : ""}`}
                      onClick={() => setFavs(togglePromptFavorite(p.message_id))}>{favs.includes(p.message_id) ? "★" : "☆"}</span>
                    <span className="kb-text prompt">{p.text}</span>
                    <span className="kb-src">{p.title}</span>
                  </div>
                ))}
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
