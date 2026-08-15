// 知识提取弹窗：分区展示会话纪要（摘要/决策/TODO/错误/命令/文件），
// 一键复制为 Markdown 交接文档，支持重新提取与 Esc 关闭。
import { useEffect, useState } from "react";
import type { ExtractionResult } from "./types";

interface Props {
  knowledge: ExtractionResult;
  /** 所属会话标题（弹窗副标题展示）。 */
  convTitle?: string | null;
  onClose: () => void;
  onReextract: () => void;
}

/** 知识提取结果 → Markdown 纪要（复制/交接用）。 */
export function knowledgeToMarkdown(k: {
  summary?: string;
  decisions?: { decision: string }[];
  todos?: { text: string }[];
  errors?: { error: string }[];
  commands?: string[];
  files?: { path: string }[];
  extractor?: string;
}): string {
  const lines: string[] = ["# 会话纪要", ""];
  if (k.summary) lines.push("## 摘要", k.summary, "");
  if ((k.decisions ?? []).length > 0) {
    lines.push("## 决策", ...(k.decisions ?? []).map((d) => `- ${d.decision}`), "");
  }
  if ((k.todos ?? []).length > 0) {
    lines.push("## TODO", ...(k.todos ?? []).map((t) => `- [ ] ${t.text}`), "");
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

export default function KnowledgeModal({ knowledge, convTitle, onClose, onReextract }: Props) {
  const [copied, setCopied] = useState(false);

  // Esc 关闭（与设置弹窗一致的交互习惯）
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const isEmpty =
    (knowledge.summary ?? "").length === 0
    && (knowledge.decisions ?? []).length === 0
    && (knowledge.todos ?? []).length === 0
    && (knowledge.errors ?? []).length === 0
    && (knowledge.commands ?? []).length === 0
    && (knowledge.files ?? []).length === 0;

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings-modal knowledge-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>
            ✨ 知识提取结果
            {convTitle && <span className="knowledge-modal-sub">{convTitle}</span>}
          </h2>
          <div className="knowledge-modal-actions">
            <button className="action-btn" onClick={onReextract}>↻ 重新提取</button>
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
            <button className="settings-close" onClick={onClose}>✕</button>
          </div>
        </div>
        <div className="settings-body">
          {isEmpty && (
            <div className="knowledge-empty">
              本会话未提取到知识要点（决策/TODO/命令/文件等）——常见于短问答类会话；
              代码/工程类会话提取效果更明显。
            </div>
          )}
          {knowledge.summary && (
            <div className="knowledge-block summary">
              <div className="knowledge-label">📖 摘要</div>
              <div className="knowledge-text">{knowledge.summary}</div>
            </div>
          )}
          {(knowledge.decisions ?? []).length > 0 && (
            <div className="knowledge-block decisions">
              <div className="knowledge-label">🎯 决策（{(knowledge.decisions ?? []).length}）</div>
              {(knowledge.decisions ?? []).map((d, i) => (
                <div key={i} className="knowledge-item">• {d.decision}</div>
              ))}
            </div>
          )}
          {(knowledge.todos ?? []).length > 0 && (
            <div className="knowledge-block todos">
              <div className="knowledge-label">📋 TODO（{(knowledge.todos ?? []).length}）</div>
              {(knowledge.todos ?? []).map((t, i) => (
                <div key={i} className="knowledge-item">• {t.text}</div>
              ))}
            </div>
          )}
          {(knowledge.errors ?? []).length > 0 && (
            <div className="knowledge-block errors">
              <div className="knowledge-label">❌ 错误（{(knowledge.errors ?? []).length}）</div>
              {(knowledge.errors ?? []).map((e, i) => (
                <div key={i} className="knowledge-item">• {e.error}</div>
              ))}
            </div>
          )}
          {(knowledge.commands ?? []).length > 0 && (
            <div className="knowledge-block commands">
              <div className="knowledge-label">⚙️ 命令（{(knowledge.commands ?? []).length}）</div>
              {(knowledge.commands ?? []).map((c, i) => (
                <div key={i} className="knowledge-item mono">• {c}</div>
              ))}
            </div>
          )}
          {(knowledge.files ?? []).length > 0 && (
            <div className="knowledge-block files">
              <div className="knowledge-label">📄 涉及文件（{(knowledge.files ?? []).length}）</div>
              {(knowledge.files ?? []).map((f, i) => (
                <div key={i} className="knowledge-item mono">• {f.path}</div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
