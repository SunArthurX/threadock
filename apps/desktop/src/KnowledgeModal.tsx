// 知识提取弹窗：分区展示会话纪要（摘要/决策/TODO/错误/命令/文件），
// 一键复制为 Markdown 交接文档，支持重新提取与 Esc 关闭。
// 增强：类型筛选 tabs（点击只看一类）+ JSON/Markdown 文件下载导出 +
//       跨会话引用（同文件/同命令还出现在哪些会话里）
import { useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import type { ExtractionResult, KnowledgeEngine, LlmRunRecord } from "./types";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";
import { Icon } from "./Icon";
interface Props {
  /** null = 数据尚未就绪（弹窗已开，渲染提取骨架） */
  knowledge: ExtractionResult | null;
  /** 所属会话 ID（用于跨会话引用排除自身）。 */
  conversationId?: string;
  /** 所属会话标题（弹窗副标题展示）。 */
  convTitle?: string | null;
  /** 当前提取引擎（默认规则；AI 需在设置中启用）。 */
  engine?: KnowledgeEngine;
  onClose: () => void;
  /** 以指定引擎重新提取（可异步；无论成败，弹窗都会解除按钮禁用态） */
  onReextract: (engine: KnowledgeEngine) => void | Promise<void>;
  /** 跳转到其他会话（跨会话引用点击）。 */
  onJumpToConversation?: (conversationId: string) => void;
  /** 本会话 AI 提取运行历史（最近在前；成功与失败都含） */
  llmRuns?: LlmRunRecord[];
  /** 当次提取的事件日志时间线（开始 → 调用模型 → 结果） */
  extractLog?: string[];
  /** 最近一次成功 AI 提取的知识内容（独立数据：主视图恒为规则结果） */
  aiKnowledge?: ExtractionResult | null;
}

type Tab = "all" | "summary" | "decisions" | "todos" | "errors" | "commands" | "files" | "airuns";
interface XrefConv { id: string; title: string | null; provider: string; updated_at_ms: number | null }
interface XrefEntry { keyword: string; kind: "file" | "command"; other_count: number; other_conversations: XrefConv[] }
const TABS: { value: Tab; label: string; icon: string }[] = [
  { value: "all", label: "全部", icon: "📚" },
  { value: "summary", label: "摘要", icon: "📖" },
  { value: "decisions", label: "决策", icon: "🎯" },
  { value: "todos", label: "TODO", icon: "📋" },
  { value: "errors", label: "错误", icon: "❌" },
  { value: "commands", label: "命令", icon: "⚙️" },
  { value: "files", label: "文件", icon: "📄" },
  { value: "airuns", label: "AI 知识", icon: "✨" },
];

/** 单 section 复制（仅复制某块的内容）。 */
async function copyOne(label: string, text: string) {
  try {
    await navigator.clipboard.writeText(text);
    showToast(`✓ 已复制 ${label}`, "info");
  } catch { showToast("剪贴板不可用", "error"); }
}

/** 知识提取结果 → Markdown 纪要（复制/交接用）。 */
export function knowledgeToMarkdown(k: {
  summary?: string;
  decisions?: { decision: string }[];
  todos?: { text: string; status?: string }[];
  errors?: { error: string }[];
  commands?: string[];
  files?: { path: string }[];
  extractor?: string;
}): string {
  const todoMark = (t: { status?: string }) => (t.status === "done" || t.status === "stale" ? "x" : " ");
  const lines: string[] = ["# 会话纪要", ""];
  if (k.summary) lines.push("## 摘要", k.summary, "");
  if ((k.decisions ?? []).length > 0) {
    lines.push("## 决策", ...(k.decisions ?? []).map((d) => `- ${d.decision}`), "");
  }
  if ((k.todos ?? []).length > 0) {
    lines.push("## TODO", ...(k.todos ?? []).map((t) => `- [${todoMark(t)}] ${t.text}`), "");
  }
  if ((k.errors ?? []).length > 0) {
    lines.push("## 错误", ...(k.errors ?? []).map((e) => `- ${e.error}`), "");
  }
  if ((k.commands ?? []).length > 0) {
    lines.push("## 命令", ...(k.commands ?? []).map((c) => "- `" + c + "`"), "");
  }
  if ((k.files ?? []).length > 0) {
    lines.push("## 涉及文件", ...(k.files ?? []).map((f) => `- ${f.path}`), "");
  }
  return lines.join("\n");
}

/** 知识提取结果 → JSON 字符串（用于程序化处理 / 二次提取）。 */
export function knowledgeToJson(k: ExtractionResult): string {
  return JSON.stringify({
    summary: k.summary ?? "",
    decisions: k.decisions ?? [],
    todos: k.todos ?? [],
    errors: k.errors ?? [],
    commands: k.commands ?? [],
    files: k.files ?? [],
    extractor: k.extractor ?? "unknown",
    exported_at: new Date().toISOString(),
  }, null, 2);
}

/** knowledge=null 时的空结构（渲染兜底；正文区会换成提取骨架） */
const EMPTY_KNOWLEDGE: ExtractionResult = {
  summary: "", decisions: [], todos: [], errors: [], commands: [], files: [], extractor: "",
};

export default function KnowledgeModal({ knowledge: knowledgeProp, conversationId, convTitle, engine = "rule", onClose, onReextract, onJumpToConversation, llmRuns = [], extractLog = [], aiKnowledge = null }: Props) {
  const [copied, setCopied] = useState(false);
  const [tab, setTab] = useState<Tab>("all");
  /** 已成功提取过后再点 ✨AI：先确认再重跑（避免重复花 token） */
  const [confirmingAi, setConfirmingAi] = useState(false);
  /** 数据未就绪（弹窗已开）：用空结构兜底渲染，正文区换成提取骨架 */
  const knowledge: ExtractionResult = knowledgeProp ?? EMPTY_KNOWLEDGE;
  const loading = knowledgeProp === null;
  /** AI 提取完成且结果就绪 → 自动切回「全部」展示知识（用户不用找） */
  const lastAiTarget = useRef(false);
  /** 引擎切换瞬时的请求中标记（AI 引擎有网络延迟）；
   * 无论成功失败都要清除——失败路径（未启用/网络错）不会有新结果到达，
   * 若只依赖「结果引用变化」清除，按钮会永久禁用（功能测试轮发现的回归） */
  const [switching, setSwitching] = useState<KnowledgeEngine | null>(null);
  const runExtract = (target: KnowledgeEngine) => {
    setSwitching(target);
    if (target === "llm") lastAiTarget.current = true;
    void Promise.resolve(onReextract(target)).finally(() => setSwitching(null));
  };
  useEffect(() => {
    if (lastAiTarget.current && switching === null && (knowledgeProp?.extractor ?? "").startsWith("llm:")) {
      lastAiTarget.current = false;
      // AI 知识 tab 自身展示知识内容（摘要+经验），不再强制跳走
    }
  }, [knowledgeProp, switching]);
  const isLlmResult = (knowledge.extractor ?? "").startsWith("llm:");
  const llmModel = isLlmResult ? knowledge.extractor.slice(4).split("@")[0] : null;
  /** 导出 dropdown 开关（合并 MD/JSON 后的单按钮） */
  const [downloadOpen, setDownloadOpen] = useState(false);
  const downloadRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!downloadOpen) return;
    const onClick = (e: MouseEvent) => {
      if (downloadRef.current && !downloadRef.current.contains(e.target as Node)) setDownloadOpen(false);
    };
    window.addEventListener("mousedown", onClick);
    return () => window.removeEventListener("mousedown", onClick);
  }, [downloadOpen]);

  // Esc 关闭（与设置弹窗一致的交互习惯）
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  // 会话切换（跨会话引用跳转）由父组件以 key={conversationId} 重挂载本组件，
  // tab/确认条随之重置——无需 effect 同步（React 正统：状态随 key 重建）

  // 跨会话引用：取 files + commands 中 Top 5（按出现频次）调用后端 xref
  const [xref, setXref] = useState<XrefEntry[]>([]);
  const [xrefLoading, setXrefLoading] = useState(false);
  const xrefKeywords = useMemo(() => {
    const files = (knowledge.files ?? []).map((f) => ({ text: f.path, kind: "file" as const }));
    const cmds = (knowledge.commands ?? []).map((c) => ({ text: c, kind: "command" as const }));
    return [...files, ...cmds].slice(0, 12); // 限 12 关键词避免请求爆
  }, [knowledge.files, knowledge.commands]);
  // 跨会话引用数据加载：无会话/关键词时同步清空（effect 数据加载模式，有意保留）
  useEffect(() => {
    if (!conversationId || xrefKeywords.length === 0) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- 无关键字时清空跨会话引用
      setXref([]);
      return;
    }
    setXrefLoading(true);
    invoke<XrefEntry[]>("knowledge_xref", { conversationId, keywords: xrefKeywords })
      .then((r) => setXref(r.filter((e) => e.other_count > 0)))
      .catch(() => setXref([]))
      .finally(() => setXrefLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId, xrefKeywords.map((k) => k.text).join("|")]);

  // 计数（用于 tab 徽标）
  const counts: Record<Tab, number> = {
    all: 0,
    summary: (knowledge.summary ?? "").trim() ? 1 : 0,
    decisions: (knowledge.decisions ?? []).length,
    todos: (knowledge.todos ?? []).length,
    errors: (knowledge.errors ?? []).length,
    commands: (knowledge.commands ?? []).length,
    files: (knowledge.files ?? []).length,
    airuns: llmRuns.length,
  };
  counts.all = counts.summary + counts.decisions + counts.todos + counts.errors + counts.commands + counts.files;

  const isEmpty = counts.all === 0;

  // 复制单 section 的小函数（仅生成纯文本）
  const summaryText = (knowledge.summary ?? "").trim();
  const decisionsMd = (knowledge.decisions ?? []).map((d) => `- ${d.decision}`).join("\n");
  const todosMd = (knowledge.todos ?? []).map((t) => `- [${t.status === "done" || t.status === "stale" ? "x" : " "}] ${t.text}`).join("\n");
  const errorsMd = (knowledge.errors ?? []).map((e) => `- ${e.error}`).join("\n");
  const commandsMd = (knowledge.commands ?? []).map((c) => `- \`${c}\``).join("\n");
  const filesMd = (knowledge.files ?? []).map((f) => `- ${f.path}`).join("\n");

  /** 导出文件（弹保存对话框，写入磁盘）。 */
  const exportFile = async (fmt: "md" | "json") => {
    try {
      const content = fmt === "md" ? knowledgeToMarkdown(knowledge) : knowledgeToJson(knowledge);
      const ext = fmt;
      const stamp = new Date().toISOString().slice(0, 10);
      const safeTitle = (convTitle ?? "knowledge").replace(/[\\/:*?"<>|]/g, "_").slice(0, 40);
      const path = await save({
        defaultPath: `${safeTitle}-${stamp}.${ext}`,
        filters: [{ name: fmt.toUpperCase(), extensions: [ext] }],
      });
      if (typeof path !== "string") return;
      await invoke("save_text_file", { path, content });
      showToast(`✓ 已导出 ${ext.toUpperCase()}（${(content.length / 1024).toFixed(1)} KB）`, "info");
    } catch (e) {
      showToast(`导出失败：${typeof e === "string" ? e : String(e)}`, "error");
    }
  };

  const showBlock = (k: Tab) => tab === "all" || tab === k;

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings-modal knowledge-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>
            <Icon name="sparkle" size={15} /> 知识提取结果
            {convTitle && <span className="knowledge-modal-sub">{convTitle}</span>}
            {llmModel && (
              <span className="badge" style={{ marginLeft: 8 }} title={`由 ${llmModel} 提取`}><Icon name="cpu" size={10} /> {llmModel}</span>
            )}
          </h2>
          <div className="knowledge-modal-actions">
            {/* 引擎切换：规则（默认，离线确定性）/ AI（需在设置中启用大模型） */}
            <div className="settings-segment" title="切换提取引擎并重新提取">
              <button
                className={engine === "rule" ? "active" : ""}
                disabled={switching !== null}
                onClick={() => runExtract("rule")}
              ><Icon name="settings" size={11} /> 规则</button>
              <button
                className={engine === "llm" ? "active" : ""}
                disabled={switching !== null}
                onClick={() => {
                  // 已成功提取过 → 先确认（避免重复花 token）；失败/没跑过 → 直接提取
                  if (llmRuns[0]?.status === "success") {
                    setConfirmingAi(true);
                    setTab("airuns");
                  } else {
                    runExtract("llm");
                  }
                }}
              >{switching === "llm" ? "AI 提取中…" : <><Icon name="sparkle" size={11} /> AI 引擎</>}</button>
            </div>
            {confirmingAi && llmRuns[0]?.status === "success" && (
              <div className="knowledge-ai-confirm">
                <span>
                  已成功 AI 提取过（{(llmRuns[0].extractor ?? "").slice(4).split("@")[0] || "AI"} ·{" "}
                  {new Date(llmRuns[0].created_at_ms).toLocaleString()}），再次提取将重新消耗 token
                </span>
                <button className="action-btn" onClick={() => { setConfirmingAi(false); runExtract("llm"); }}>再次提取</button>
                <button className="action-btn" onClick={() => setConfirmingAi(false)}>取消</button>
              </div>
            )}
            <button className="action-btn" onClick={() => runExtract(engine)} disabled={switching !== null}><Icon name="sync" size={11} /> 重新提取</button>
            {/* MD/JSON 下载合并为单一 dropdown 按钮：节省顶栏空间 */}
            <div className={`list-dropdown ${downloadOpen ? "open" : ""}`} ref={downloadRef}>
              <button
                className={`action-btn list-dropdown-btn ${downloadOpen ? "active" : ""}`}
                onClick={() => setDownloadOpen((o) => !o)}
                title="导出为 Markdown / JSON 文件"
              >⤓ 导出 <span className="list-dropdown-caret">▾</span></button>
              {downloadOpen && (
                <div className="list-dropdown-panel right">
                  <button className="list-dropdown-item" onClick={() => { setDownloadOpen(false); exportFile("md"); }}>
                    <span className="list-dropdown-icon">📄</span><span>Markdown（.md）</span>
                  </button>
                  <button className="list-dropdown-item" onClick={() => { setDownloadOpen(false); exportFile("json"); }}>
                    <span className="list-dropdown-icon">🧾</span><span>JSON（.json）</span>
                  </button>
                </div>
              )}
            </div>
            <button
              className="action-btn"
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(knowledgeToMarkdown(knowledge));
                  setCopied(true);
                  window.setTimeout(() => setCopied(false), 2000);
                } catch { /* 剪贴板不可用时忽略 */ }
              }}
              title="把提取结果复制为 Markdown，可直接粘贴做会话纪要 / 交接文档"
            >
              {copied ? "✓ 已复制" : "⧉ 复制为纪要"}
            </button>
            <button className="settings-close" onClick={onClose} aria-label="关闭"><Icon name="close" size={14} /></button>
          </div>
        </div>
        {/* 类型筛选 tabs（带计数徽标） */}
        {!isEmpty && (
          <div className="knowledge-tabs">
            {TABS.map((t) => (
              <button
                key={t.value}
                className={`filter-chip ${tab === t.value ? "active" : ""} ${counts[t.value] === 0 && t.value !== "all" && t.value !== "airuns" ? "disabled" : ""}`}
                onClick={() => setTab(t.value)}
                disabled={counts[t.value] === 0 && t.value !== "all" && t.value !== "airuns"}
                title={counts[t.value] === 0 ? `${t.label}（无内容）` : `只看${t.label}`}
              >
                <span className="tab-icon">{t.icon}</span>
                {t.label}
                <span className="tab-count">{counts[t.value]}</span>
              </button>
            ))}
          </div>
        )}
        <ScrollArea className="settings-body">
          {/* 跨会话引用：同文件/同命令 还出现在哪些会话里 */}
          {conversationId && (xrefLoading || xref.length > 0) && (
            <div className="knowledge-xref">
              <div className="knowledge-label">
                🔗 跨会话引用（{xrefLoading ? "查询中…" : `${xref.length} 个文件/命令还在其他会话里出现`}）
              </div>
              {xrefLoading ? (
                <div className="sk-line" style={{ margin: 12 }} />
              ) : (
                <ScrollArea className="knowledge-xref-list">
                  {xref.map((e) => (
                    <details key={`${e.kind}-${e.keyword}`} className="knowledge-xref-item">
                      <summary>
                        <span className={`xref-kind ${e.kind}`}>{e.kind === "file" ? "📄" : "⚙️"}</span>
                        <span className="xref-keyword mono" title={e.keyword}>{e.keyword.length > 40 ? e.keyword.slice(0, 40) + "…" : e.keyword}</span>
                        <span className="xref-count">{e.other_count} 个会话</span>
                      </summary>
                      <div className="knowledge-xref-convs">
                        {e.other_conversations.map((c) => (
                          <button
                            key={c.id}
                            className="xref-conv-row"
                            onClick={() => onJumpToConversation?.(c.id)}
                            title="点击跳转到该会话"
                          >
                            <span className={`badge source ${c.provider}`}>{c.provider}</span>
                            <span className="xref-conv-title">{c.title ?? "(无标题)"}</span>
                          </button>
                        ))}
                      </div>
                    </details>
                  ))}
                </ScrollArea>
              )}
            </div>
          )}
          {loading && (
            <div className="knowledge-empty">
              ⏳ 正在读取/提取知识…（大会话规则提取 &lt; 1 秒；AI 提取请看「AI 记录」日志）
            </div>
          )}
          {!loading && isEmpty && (
            <div className="knowledge-empty">
              本会话未提取到知识要点（决策/TODO/命令/文件等）——常见于短问答类会话；
              代码/工程类会话提取效果更明显。
            </div>
          )}
          {showBlock("summary") && summaryText && (
            <div className="knowledge-block summary">
              <div className="knowledge-label">
                📖 摘要
                <button className="kb-copy" onClick={() => copyOne("摘要", summaryText)}>📋</button>
              </div>
              <div className="knowledge-text">{summaryText}</div>
            </div>
          )}
          {showBlock("decisions") && (knowledge.decisions ?? []).length > 0 && (
            <div className="knowledge-block decisions">
              <div className="knowledge-label">
                🎯 决策（{(knowledge.decisions ?? []).length}）
                <button className="kb-copy" onClick={() => copyOne("决策列表", decisionsMd)}>📋</button>
              </div>
              {(knowledge.decisions ?? []).map((d, i) => (
                <div key={i} className="knowledge-item">• {d.decision}</div>
              ))}
            </div>
          )}
          {showBlock("todos") && (knowledge.todos ?? []).length > 0 && (
            <div className="knowledge-block todos">
              <div className="knowledge-label">
                📋 TODO（{(knowledge.todos ?? []).length}）
                <button className="kb-copy" onClick={() => copyOne("TODO 列表", todosMd)}>📋</button>
              </div>
              {(knowledge.todos ?? []).map((t, i) => (
                <div
                  key={i}
                  className="knowledge-item"
                  title={t.status === "stale" ? "过期：会话早期的计划，后续已被覆盖" : t.status === "done" ? "已完成：后文有完成证据" : "待办"}
                >
                  {t.status === "done" ? "☑" : t.status === "stale" ? "⊘" : "☐"} {t.text}
                </div>
              ))}
            </div>
          )}
          {showBlock("errors") && (knowledge.errors ?? []).length > 0 && (
            <div className="knowledge-block errors">
              <div className="knowledge-label">
                ❌ 错误（{(knowledge.errors ?? []).length}）
                <button className="kb-copy" onClick={() => copyOne("错误列表", errorsMd)}>📋</button>
              </div>
              {(knowledge.errors ?? []).map((e, i) => (
                <div key={i} className="knowledge-item">• {e.error}</div>
              ))}
            </div>
          )}
          {showBlock("commands") && (knowledge.commands ?? []).length > 0 && (
            <div className="knowledge-block commands">
              <div className="knowledge-label">
                ⚙️ 命令（{(knowledge.commands ?? []).length}）
                <button className="kb-copy" onClick={() => copyOne("命令列表", commandsMd)}>📋</button>
              </div>
              {(knowledge.commands ?? []).map((c, i) => (
                <div key={i} className="knowledge-item mono">• {c}</div>
              ))}
            </div>
          )}
          {showBlock("files") && (knowledge.files ?? []).length > 0 && (
            <div className="knowledge-block files">
              <div className="knowledge-label">
                📄 涉及文件（{(knowledge.files ?? []).length}）
                <button className="kb-copy" onClick={() => copyOne("文件列表", filesMd)}>📋</button>
              </div>
              {(knowledge.files ?? []).map((f, i) => (
                <div key={i} className="knowledge-item mono">• {f.path}</div>
              ))}
            </div>
          )}
          {tab === "airuns" && (
            <div className="knowledge-block airuns">
              {/* AI 提取的知识（独立数据源）：本次对话可复用的经验 */}
              {aiKnowledge ? (
                <>
                  <div className="knowledge-label">
                    ✨ 本次对话的 AI 经验（{(aiKnowledge.extractor ?? "").slice(4).split("@")[0] || "AI"} 提取）
                  </div>
                  {(aiKnowledge.summary ?? "").trim() && (
                    <div style={{ whiteSpace: "pre-wrap", fontSize: 12.5, lineHeight: 1.7, padding: "4px 0 8px" }}>
                      {aiKnowledge.summary}
                    </div>
                  )}
                  {(aiKnowledge.decisions ?? []).slice(0, 5).map((d, i) => (
                    <div key={`d${i}`} className="knowledge-item">🎯 {d.decision}</div>
                  ))}
                  {(aiKnowledge.todos ?? []).filter((t) => t.status !== "done" && t.status !== "stale").slice(0, 5).map((t, i) => (
                    <div key={`t${i}`} className="knowledge-item">📋 待办：{t.text}</div>
                  ))}
                  {(aiKnowledge.errors ?? []).slice(0, 5).map((e, i) => (
                    <div key={`e${i}`} className="knowledge-item">⚠️ {e.error}{e.solution ? ` → ${e.solution}` : ""}</div>
                  ))}
                  {((aiKnowledge.commands ?? []).length > 0 || (aiKnowledge.files ?? []).length > 0) && (
                    <div className="knowledge-item" style={{ opacity: 0.7 }}>
                      另有命令 {aiKnowledge.commands?.length ?? 0} 条 / 文件 {aiKnowledge.files?.length ?? 0} 个
                    </div>
                  )}
                </>
              ) : (
                <div className="knowledge-item" style={{ opacity: 0.7 }}>
                  还没有 AI 提取的经验——点顶部「✨AI 引擎」提取（约 30~120 秒），prompt 心得、踩坑与解法会沉淀在这里
                </div>
              )}
              <div className="knowledge-label" style={{ marginTop: 10 }}>运行日志与历史（{llmRuns.length}）</div>
              {extractLog.length > 0 && (
                <div className="ai-run-log">
                  <div className="automation-sub">本次提取日志</div>
                  {extractLog.map((line, i) => (
                    <div key={i} className="knowledge-item mono" style={{ whiteSpace: "pre-wrap" }}>{line}</div>
                  ))}
                </div>
              )}
              {llmRuns.length === 0 ? (
                <div className="knowledge-item">还没有 AI 提取记录</div>
              ) : (
                llmRuns.map((r) => (
                  <div key={r.id} className={`knowledge-item ${r.status === "failed" ? "ai-run-failed" : ""}`}>
                    <b>{r.status === "success" ? "✓ 成功" : "✗ 失败"}</b>
                    {" "}{r.extractor.startsWith("llm:") ? r.extractor.slice(4).split("@")[0] : "AI"} · {r.duration_ms >= 1000 ? `${Math.round(r.duration_ms / 1000)}s` : `${r.duration_ms}ms`} ·{" "}
                    输入 {r.input_messages} 条消息/{r.input_chars >= 1000 ? `${Math.round(r.input_chars / 1000)}k` : r.input_chars} 字符
                    {r.status === "success" ? ` · ${r.items_total} 条` : ""}
                    <span style={{ opacity: 0.55, marginLeft: 8 }}>{new Date(r.created_at_ms).toLocaleString()}</span>
                    {r.status === "failed" && r.error && (
                      <div style={{ color: "var(--danger, #e5484d)", fontSize: 12, marginTop: 4, whiteSpace: "pre-wrap" }}>原因：{r.error}</div>
                    )}
                  </div>
                ))
              )}
            </div>
          )}
        </ScrollArea>
      </div>
    </div>
  );
}
