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
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // 唤起时聚焦 + 拉取最近 30 条会话
  useEffect(() => {
    if (!open) return;
    setQ("");
    setActive(0);
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

  const commitActive = () => {
    const all = [...items.matchedPages, ...items.matchedConvs];
    const pick = all[active];
    if (!pick) return;
    if (pick.kind === "page") {
      onJumpPage(pick.page.key);
      onClose();
    } else if (pick.kind === "conv" && onJumpConversation) {
      onJumpConversation(pick.conv.id);
      onClose();
    }
  };

  if (!open) return null;
  const all = [...items.matchedPages, ...items.matchedConvs];
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
            placeholder="跳到页面 / 搜会话标题…  (↑↓ 选择 · Enter 跳转 · Esc 关闭)"
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
