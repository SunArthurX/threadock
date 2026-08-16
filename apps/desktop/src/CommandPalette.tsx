// 全站 Command Palette：⌘K / Ctrl+K 唤起
// 支持：页面跳转、跳到指定会话、跳到知识条目、跳到活动页指定日期
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Conversation } from "./types";
import { formatTime } from "./types";
import ScrollArea from "./ScrollArea";

export type Page = "chat" | "overview" | "cost" | "security" | "assets" | "knowledge" | "activity" | "projects";

const PAGES: { key: Page; icon: string; label: string; hint: string }[] = [
  { key: "chat", icon: "💬", label: "对话", hint: "会话列表 / 搜索" },
  { key: "overview", icon: "📊", label: "概览", hint: "治理总览" },
  { key: "cost", icon: "💰", label: "成本", hint: "成本 / 预算" },
  { key: "security", icon: "🛡", label: "安全", hint: "审计 / 风险" },
  { key: "assets", icon: "🧩", label: "资产", hint: "技能 / 插件 / MCP" },
  { key: "knowledge", icon: "📚", label: "知识库", hint: "决策 / TODO / 提示词" },
  { key: "activity", icon: "📆", label: "活动", hint: "热力图 / 时段 / 工具" },
  { key: "projects", icon: "📁", label: "项目", hint: "按 source_dir 归并" },
];

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  onJumpPage: (p: Page) => void;
  onJumpConversation?: (cid: string) => void;
}

export function CommandPalette({ open, onClose, onJumpPage, onJumpConversation }: CommandPaletteProps) {
  const [q, setQ] = useState("");
  const [convs, setConvs] = useState<Conversation[]>([]);
  // Prompt 复用推荐：相似历史 user 消息 + 当时 cost
  // round 25：用户开始输入 2+ 字符时实时拉取，「你之前 3 个会话问过类似问题」一键跳
  const [promptReuse, setPromptReuse] = useState<{
    message_id: string;
    conversation_id: string;
    title: string | null;
    user_title: string | null;
    model: string | null;
    provider_name: string;
    snippet: string;
    body: string;
    cost_usd: number;
  }[]>([]);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // 唤起时聚焦 + 拉取最近 30 条会话
  useEffect(() => {
    if (!open) return;
    setQ("");
    setActive(0);
    setPromptReuse([]);
    setTimeout(() => inputRef.current?.focus(), 30);
    (async () => {
      try {
        const list = await invoke<Conversation[]>("list_conversations", {
          workspaceId: null, provider: null, favorite: null, archived: null, includeDeleted: false,
        });
        setConvs(list.slice(0, 30));
      } catch { /* 静默：拉取失败时只显示页面跳转 */ }
    })();
  }, [open]);

  // 关闭时 esc
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Prompt 复用推荐：q 长度 ≥ 2 时 debounce 拉取（FTS5 + cost JOIN）
  useEffect(() => {
    const query = q.trim();
    if (query.length < 2) {
      setPromptReuse([]);
      return;
    }
    const t = window.setTimeout(() => {
      (async () => {
        try {
          const hits = await invoke<
            {
              message_id: string;
              conversation_id: string;
              title: string | null;
              user_title: string | null;
              model: string | null;
              provider_name: string;
              snippet: string;
              body: string;
              cost_usd: number;
            }[]
          >("prompt_reuse_search", { query, limit: 5 });
          setPromptReuse(hits);
          setActive(0);
        } catch {
          setPromptReuse([]);
        }
      })();
    }, 250);
    return () => window.clearTimeout(t);
  }, [q]);

  const items = useMemo(() => {
    const lower = q.trim().toLowerCase();
    const ql = lower;
    const matchedPages = PAGES.filter(
      (p) => !ql || p.label.toLowerCase().includes(ql) || p.hint.toLowerCase().includes(ql) || p.key.toLowerCase().includes(ql),
    ).map((p) => ({ kind: "page" as const, page: p }));
    const matchedConvs = ql
      ? convs.filter((c) =>
          (c.title ?? "").toLowerCase().includes(ql) ||
          (c.user_title ?? "").toLowerCase().includes(ql) ||
          (c.provider ?? "").toLowerCase().includes(ql),
        ).slice(0, 15).map((c) => ({ kind: "conv" as const, conv: c }))
      : convs.slice(0, 8).map((c) => ({ kind: "conv" as const, conv: c }));
    return { matchedPages, matchedConvs, total: matchedPages.length + matchedConvs.length };
  }, [q, convs]);

  // 键盘上下选择
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActive((a) => Math.min(a + 1, items.total - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => Math.max(0, a - 1));
      } else if (e.key === "Enter") {
        e.preventDefault();
        commitActive();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, items, active, onClose]); // eslint-disable-line react-hooks/exhaustive-deps

  // Prompt 复用 item 在列表里按 (page, conv, reuse) 顺序排，索引分段计算
  const commitActive = () => {
    const pageCount = items.matchedPages.length;
    const convCount = items.matchedConvs.length;
    if (active < pageCount) {
      const pick = items.matchedPages[active];
      onJumpPage(pick.page.key);
      onClose();
    } else if (active < pageCount + convCount) {
      const pick = items.matchedConvs[active - pageCount];
      if (onJumpConversation) {
        onJumpConversation(pick.conv.id);
        onClose();
      } else {
        onJumpPage("chat");
        onClose();
      }
    } else {
      const idx = active - pageCount - convCount;
      const pick = promptReuse[idx];
      if (pick && onJumpConversation) {
        onJumpConversation(pick.conversation_id);
        onClose();
      }
    }
  };

  if (!open) return null;
  const all = [...items.matchedPages, ...items.matchedConvs, ...promptReuse];
  return (
    <div className="cmd-overlay" onClick={onClose}>
      <div className="cmd-modal" onClick={(e) => e.stopPropagation()}>
        <div className="cmd-input-wrap">
          <span className="cmd-input-icon">⌘K</span>
          <input
            ref={inputRef}
            className="cmd-input"
            value={q}
            onChange={(e) => { setQ(e.target.value); setActive(0); }}
            placeholder="跳到页面 / 搜会话标题 / 找历史相似 prompt…  (↑↓ 选择 · Enter 跳转 · Esc 关闭)"
          />
        </div>
        <ScrollArea className="cmd-list">
          {items.matchedPages.length > 0 && (
            <div className="cmd-group">
              <div className="cmd-group-title">页面</div>
              {items.matchedPages.map((it, i) => (
                <div
                  key={`p-${it.page.key}`}
                  className={`cmd-row ${active === i ? "active" : ""}`}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => { onJumpPage(it.page.key); onClose(); }}
                >
                  <span className="cmd-row-icon">{it.page.icon}</span>
                  <span className="cmd-row-label">{it.page.label}</span>
                  <span className="cmd-row-hint">{it.page.hint}</span>
                </div>
              ))}
            </div>
          )}
          {items.matchedConvs.length > 0 && (
            <div className="cmd-group">
              <div className="cmd-group-title">最近会话（{items.matchedConvs.length}）</div>
              {items.matchedConvs.map((it, i) => {
                const idx = items.matchedPages.length + i;
                return (
                  <div
                    key={`c-${it.conv.id}`}
                    className={`cmd-row ${active === idx ? "active" : ""}`}
                    onMouseEnter={() => setActive(idx)}
                    onClick={() => {
                      if (onJumpConversation) {
                        onJumpConversation(it.conv.id);
                        onClose();
                      } else {
                        onJumpPage("chat");
                        onClose();
                      }
                    }}
                  >
                    <span className="cmd-row-icon">💬</span>
                    <span className="cmd-row-label">
                      {it.conv.user_title ?? it.conv.title ?? "(无标题)"}
                    </span>
                    <span className="cmd-row-hint">
                      {it.conv.provider}{formatTime(it.conv.started_at_ms ?? null)}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
          {promptReuse.length > 0 && (
            <div className="cmd-group">
              <div className="cmd-group-title">💡 你之前问过类似问题（Prompt 复用）</div>
              {promptReuse.map((h, i) => {
                const idx = items.matchedPages.length + items.matchedConvs.length + i;
                return (
                  <div
                    key={`r-${h.message_id}`}
                    className={`cmd-row ${active === idx ? "active" : ""}`}
                    onMouseEnter={() => setActive(idx)}
                    onClick={() => {
                      if (onJumpConversation) {
                        onJumpConversation(h.conversation_id);
                        onClose();
                      }
                    }}
                    data-testid="cmd-prompt-reuse"
                  >
                    <span className="cmd-row-icon">🔁</span>
                    <span className="cmd-row-label" dangerouslySetInnerHTML={{ __html: h.snippet }} />
                    <span className="cmd-row-hint">
                      {h.provider_name}{h.model ? ` · ${h.model}` : ""}{h.cost_usd > 0 ? ` · $${h.cost_usd.toFixed(2)}` : ""}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
          {all.length === 0 && (
            <div className="cmd-empty">没有匹配项（试试「活动」「成本」或会话标题关键词）</div>
          )}
        </ScrollArea>
        <div className="cmd-footer">
          <span>↑↓ 移动</span>
          <span>⏎ 跳转</span>
          <span>esc 关闭</span>
          <span style={{ marginLeft: "auto", opacity: 0.55 }}>Threadock · Command Palette</span>
        </div>
      </div>
    </div>
  );
}
