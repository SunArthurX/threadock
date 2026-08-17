// 全站 Command Palette：⌘K / Ctrl+K 唤起
// 支持：页面跳转、跳到指定会话、跳到知识条目、跳到活动页指定日期、内置动作
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

// 内置动作：触发外部副作用（开关弹窗 / 跑同步 / 切主题等），由 App 层通过 onAction 实际处理。
// P1-E3：⌘K 现在不只跳页，也能执行动作。
export type CommandActionId =
  | "open_settings"
  | "trigger_sync"
  | "toggle_theme"
  | "show_shortcuts"
  | "open_reports"
  | "show_changelog";

const ACTIONS: { id: CommandActionId; icon: string; label: string; hint: string }[] = [
  { id: "open_settings", icon: "⚙", label: "打开设置", hint: "主题 / 同步 / 预算 / 重置" },
  { id: "trigger_sync", icon: "⟳", label: "触发同步", hint: "立即从来源拉取最新会话" },
  { id: "toggle_theme", icon: "🌓", label: "切换主题 深/浅", hint: "深色 ⇄ 浅色" },
  { id: "show_shortcuts", icon: "?", label: "显示快捷键", hint: "列出所有全局快捷键" },
  { id: "open_reports", icon: "📄", label: "打开周报中心", hint: "历史周报列表" },
  { id: "show_changelog", icon: "📝", label: "查看更新日志", hint: "本版本的变更说明" },
];

// 列表项的判别联合（P1-E3）
type Command =
  | { kind: "page"; page: typeof PAGES[number] }
  | { kind: "action"; action: typeof ACTIONS[number] }
  | { kind: "conv"; conv: Conversation }
  | { kind: "reuse"; hit: {
      message_id: string;
      conversation_id: string;
      title: string | null;
      user_title: string | null;
      model: string | null;
      provider_name: string;
      snippet: string;
      body: string;
      cost_usd: number;
    } };

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  onJumpPage: (p: Page) => void;
  onJumpConversation?: (cid: string, mid?: string) => void;
  /** 触发内置动作（设置 / 同步 / 切主题 等）。由 App 层实现副作用。 */
  onAction?: (action: CommandActionId) => void;
}

const RECENT_KEY = "ch-cmd-recent";
const RECENT_MAX = 10;

function loadRecent(): string[] {
  try { return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]") as string[]; }
  catch { return []; }
}
function pushRecent(q: string) {
  const trimmed = q.trim();
  if (!trimmed) return;
  const next = [trimmed, ...loadRecent().filter((x) => x !== trimmed)].slice(0, RECENT_MAX);
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(next)); } catch { /* 静默 */ }
}

export function CommandPalette({ open, onClose, onJumpPage, onJumpConversation, onAction }: CommandPaletteProps) {
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
  // （open 由父组件控制，无法在本地事件处理器里重置输入状态，只能在 effect 中同步）
  useEffect(() => {
    if (!open) return;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 打开时重置输入/选中状态（受控 open prop）
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
  // 短关键词不显示复用推荐：在渲染层推导（而非 effect 里 setState 清空），
  // 顺带规避「在途请求返回后覆盖清空结果」的竞态。
  useEffect(() => {
    const query = q.trim();
    if (query.length < 2) return;
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

  // 候选列表：page / action / conv / reuse（P1-E3 命令判别联合）
  const items = useMemo<Command[]>(() => {
    const lower = q.trim().toLowerCase();
    const ql = lower;
    // 短关键词（<2 字符）时不展示复用推荐：渲染层推导（替代 effect 里 setState 清空），
    // 顺带规避「在途请求返回后覆盖清空结果」的竞态。
    const shownReuse = q.trim().length >= 2 ? promptReuse : [];
    const matchedPages: Command[] = PAGES.filter(
      (p) => !ql || p.label.toLowerCase().includes(ql) || p.hint.toLowerCase().includes(ql) || p.key.toLowerCase().includes(ql),
    ).map((p) => ({ kind: "page", page: p }));
    const matchedActions: Command[] = ql
      ? ACTIONS.filter(
          (a) => a.label.toLowerCase().includes(ql) || a.hint.toLowerCase().includes(ql),
        ).map((a) => ({ kind: "action", action: a }))
      : [];
    const matchedConvs: Command[] = ql
      ? convs.filter((c) =>
          (c.title ?? "").toLowerCase().includes(ql) ||
          (c.user_title ?? "").toLowerCase().includes(ql) ||
          (c.provider ?? "").toLowerCase().includes(ql),
        ).slice(0, 15).map((c) => ({ kind: "conv", conv: c }))
      : convs.slice(0, 8).map((c) => ({ kind: "conv", conv: c }));
    const matchedReuse: Command[] = shownReuse.map((h) => ({ kind: "reuse", hit: h }));
    return [...matchedPages, ...matchedActions, ...matchedConvs, ...matchedReuse];
  }, [q, convs, promptReuse]);

  // commit 顺序：page → action → conv → reuse（与 items 顺序保持一致）
  const commitActive = () => {
    const pick = items[active];
    if (!pick) return;
    // 记一次最近查询（哪怕只是 Enter 触发的也记下来）
    if (q.trim()) pushRecent(q);
    switch (pick.kind) {
      case "page":
        onJumpPage(pick.page.key);
        onClose();
        return;
      case "action":
        if (onAction) onAction(pick.action.id);
        onClose();
        return;
      case "conv":
        if (onJumpConversation) {
          onJumpConversation(pick.conv.id);
          onClose();
        } else {
          onJumpPage("chat");
          onClose();
        }
        return;
      case "reuse":
        if (onJumpConversation) {
          onJumpConversation(pick.hit.conversation_id, pick.hit.message_id);
          onClose();
        }
        return;
    }
  };

  // 键盘上下选择
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActive((a) => Math.min(a + 1, items.length - 1));
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

  if (!open) return null;
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
          {/* 页面组 */}
          {items.some((i) => i.kind === "page") && (
            <div className="cmd-group">
              <div className="cmd-group-title">页面</div>
              {items.map((it, i) => it.kind === "page" ? (
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
              ) : null)}
            </div>
          )}
          {/* 动作组（P1-E3） */}
          {items.some((i) => i.kind === "action") && (
            <div className="cmd-group">
              <div className="cmd-group-title">动作</div>
              {items.map((it, i) => it.kind === "action" ? (
                <div
                  key={`a-${it.action.id}`}
                  className={`cmd-row ${active === i ? "active" : ""}`}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => { if (onAction) onAction(it.action.id); onClose(); }}
                  data-testid="cmd-action"
                >
                  <span className="cmd-row-icon">{it.action.icon}</span>
                  <span className="cmd-row-label">{it.action.label}</span>
                  <span className="cmd-row-hint">{it.action.hint}</span>
                </div>
              ) : null)}
            </div>
          )}
          {/* 会话组 */}
          {items.some((i) => i.kind === "conv") && (
            <div className="cmd-group">
              <div className="cmd-group-title">最近会话（{items.filter((i) => i.kind === "conv").length}）</div>
              {items.map((it, i) => it.kind === "conv" ? (
                <div
                  key={`c-${it.conv.id}`}
                  className={`cmd-row ${active === i ? "active" : ""}`}
                  onMouseEnter={() => setActive(i)}
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
              ) : null)}
            </div>
          )}
          {/* Prompt 复用组 */}
          {items.some((i) => i.kind === "reuse") && (
            <div className="cmd-group">
              <div className="cmd-group-title">💡 你之前问过类似问题（Prompt 复用）</div>
              {items.map((it, i) => it.kind === "reuse" ? (
                <div
                  key={`r-${it.hit.message_id}`}
                  className={`cmd-row ${active === i ? "active" : ""}`}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => {
                    if (onJumpConversation) {
                      onJumpConversation(it.hit.conversation_id, it.hit.message_id);
                      onClose();
                    }
                  }}
                  data-testid="cmd-prompt-reuse"
                >
                  <span className="cmd-row-icon">🔁</span>
                  <span className="cmd-row-label" dangerouslySetInnerHTML={{ __html: it.hit.snippet }} />
                  <span className="cmd-row-hint">
                    {it.hit.provider_name}{it.hit.model ? ` · ${it.hit.model}` : ""}{it.hit.cost_usd > 0 ? ` · $${it.hit.cost_usd.toFixed(2)}` : ""}
                  </span>
                </div>
              ) : null)}
            </div>
          )}
          {items.length === 0 && (
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
