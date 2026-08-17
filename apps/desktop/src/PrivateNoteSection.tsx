// 私有笔记 section：折叠默认开，autosave on blur（不参与搜索/导出/统计）。
// 父级应给此组件传 `key={conv.id}`：切换会话时 React 会卸载旧实例并重建，
// 避免 useEffect [note] 覆盖用户当前正在编辑的本地 text。
import { useEffect, useState } from "react";

export interface PrivateNoteSectionProps {
  note: string;
  onChange: (n: string | null) => void;
}

export default function PrivateNoteSection({ note, onChange }: PrivateNoteSectionProps) {
  const [text, setText] = useState(note);
  // 受控但允许本地编辑（保存前不写回父级，autosave 触发）
  // note prop 变化（保存回写/父级刷新）时同步到本地编辑态；父级已用 key={conv.id}
  // 处理会话切换，此处仅同步同一会话内的 prop 更新，effect 同步是有意的。
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- note prop 变化时同步本地编辑态
    setText(note);
  }, [note]);
  const [saved, setSaved] = useState<"idle" | "saving" | "saved">("idle");

  const save = async (next: string) => {
    const trimmed = next.trim();
    if (trimmed === (note ?? "").trim()) { setSaved("idle"); return; }
    setSaved("saving");
    try { await onChange(trimmed || null); setSaved("saved"); window.setTimeout(() => setSaved("idle"), 1500); }
    catch { setSaved("idle"); }
  };

  // 切走 / 关窗前自动保存：在 blur/失焦前若仍有未提交内容，立即落库。
  useEffect(() => {
    const flush = () => {
      // 仅当本地 text 与 prop note 不一致时才需要保存
      if (text.trim() !== (note ?? "").trim()) {
        // fire-and-forget；onChange 是 Promise，我们不 await，避免同步路径阻塞
        void onChange(text.trim() || null);
      }
    };
    const onVisibility = () => { if (document.visibilityState === "hidden") flush(); };
    const onPageHide = () => flush();
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", onPageHide);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("pagehide", onPageHide);
    };
  }, [text, note, onChange]);

  const placeholder = "📝 私人笔记（不参与搜索/导出/统计）";
  return (
    <details className="private-note" open={!!note}>
      <summary>
        📝 私人笔记 {saved === "saving" && <span className="private-note-status">保存中…</span>}
        {saved === "saved" && <span className="private-note-status saved">✓ 已保存</span>}
      </summary>
      <textarea
        className="private-note-text"
        value={text}
        placeholder={placeholder}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => save(e.target.value)}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            save(text);
            (e.target as HTMLTextAreaElement).blur();
          }
        }}
        rows={3}
      />
      <div className="private-note-hint">
        ⌘+Enter 保存 · 失焦自动保存 · 切走/关窗前自动保存 · 清空内容后失焦 = 删除笔记
      </div>
    </details>
  );
}
