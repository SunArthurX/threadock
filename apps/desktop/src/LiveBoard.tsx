// LiveBoard：多 Agent 实时任务监控面板。
// 一个窗口看到所有正在运行的 coding agent 会话（Claude Code / Codex 桌面+CLI / ZCode / MiniMax Code）
// 及其实时进度；数据由后端直接读源侧存储（被动只读），前端 3s 轮询，点卡片开实时流抽屉（2s 轮询）。
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { Icon } from "./Icon";
import { CardTitle } from "./CardTitle";
import ScrollArea from "./ScrollArea";
import { InlineEmpty } from "./EmptyState";
import type { LiveActivity, LiveTaskDto, LiveTasksDto } from "./live";
import {
  elapsedText, liveSourceLabel, liveStateMeta, originatorBadge, relTime, tokenShort,
} from "./live";

const POLL_MS = 3_000;
const DRAWER_POLL_MS = 2_000;

interface Props {
  onJumpToConversation: (conversationId: string) => void | Promise<void>;
}

/** 活动条目（卡片摘要与抽屉通用）。 */
function ActivityRow({ act, compact = false }: { act: LiveActivity; compact?: boolean }) {
  const kindMeta = (): { icon: Parameters<typeof Icon>[0]["name"]; color: string } => {
    switch (act.kind) {
      case "tool": return { icon: "terminal", color: "var(--text-secondary)" };
      case "thinking": return { icon: "lightbulb", color: "var(--text-muted)" };
      case "system": case "status": return { icon: "info", color: "var(--text-muted)" };
      default: return act.role === "user" ? { icon: "chat", color: "var(--accent)" } : { icon: "sparkle", color: "var(--text-secondary)" };
    }
  };
  const meta = kindMeta();
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 6, minWidth: 0 }}>
      <Icon name={meta.icon} size={12} style={{ flexShrink: 0, marginTop: 2, color: meta.color }} />
      <span
        className="mono"
        style={{
          fontSize: compact ? 11 : 11.5, lineHeight: 1.45, color: "var(--text-secondary)",
          overflow: "hidden", display: "-webkit-box", WebkitLineClamp: compact ? 2 : 1, WebkitBoxOrient: "vertical",
          wordBreak: "break-all",
        }}
        title={act.snippet}
      >
        {act.snippet}
      </span>
    </div>
  );
}

/** 进度徽标行：todo / 消息数 / token。 */
function ProgressRow({ task }: { task: LiveTaskDto }) {
  const p = task.progress;
  const items: string[] = [];
  if (p?.todosTotal) items.push(t("TODO {__0__}/{__1__}", { __0__: p.todosDone ?? 0, __1__: p.todosTotal }));
  if (p?.messageCount != null) items.push(t("{__0__} 条消息", { __0__: p.messageCount }));
  if (p?.totalTokens) items.push(t("~{__0__} tokens", { __0__: tokenShort(p.totalTokens) }));
  if (!items.length) return null;
  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
      {items.map((it) => (
        <span key={it} style={{
          fontSize: 10.5, padding: "1px 7px", borderRadius: 999,
          background: "var(--bg-grouped)", border: "1px solid var(--border-subtle)", color: "var(--text-muted)",
        }}>
          {it}
        </span>
      ))}
    </div>
  );
}

/** 任务卡片。 */
function TaskCard({ task, now, onOpen, onJump }: {
  task: LiveTaskDto; now: number;
  onOpen: () => void;
  onJump: () => void;
}) {
  const meta = liveStateMeta(task.state);
  const badge = originatorBadge(task.source, task.originator);
  const running = task.state === "running";
  return (
    <div
      onClick={onOpen}
      style={{
        background: "var(--bg-panel)", border: "1px solid var(--border-default)",
        borderLeft: `3px solid ${meta.color}`, borderRadius: 10, padding: "10px 12px",
        display: "flex", flexDirection: "column", gap: 7, cursor: "pointer", minWidth: 0,
      }}
      title={t("点击查看实时流")}
    >
      {/* 头行：源徽标 + 端别 + 状态 */}
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span style={{
          fontSize: 10.5, fontWeight: 600, padding: "1px 7px", borderRadius: 5,
          background: "var(--accent-bg)", color: "var(--accent)",
        }}>
          {liveSourceLabel(task.source)}
        </span>
        {badge && (
          <span style={{
            fontSize: 10, padding: "1px 6px", borderRadius: 999,
            background: "var(--bg-grouped)", color: "var(--text-muted)", border: "1px solid var(--border-subtle)",
          }}>{badge}</span>
        )}
        <span style={{ flex: 1 }} />
        <span style={{
          display: "inline-flex", alignItems: "center", gap: 4, fontSize: 11,
          color: meta.color, fontWeight: 600,
        }}>
          <span
            className={running ? "live-dot-pulse" : undefined}
            style={{ width: 7, height: 7, borderRadius: 999, background: meta.color, display: "inline-block" }}
          />
          {meta.label}
        </span>
      </div>
      {/* 标题 + 工作区 */}
      <div style={{ minWidth: 0 }}>
        <div style={{
          fontSize: 13, fontWeight: 600, color: "var(--text-primary)",
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
        }} title={task.title ?? undefined}>
          {task.title ?? t("（未命名任务）")}
        </div>
        {task.workspace && (
          <div style={{ fontSize: 11, color: "var(--text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {task.workspace}
          </div>
        )}
      </div>
      {/* 最近活动 */}
      {task.activity && <ActivityRow act={task.activity} compact />}
      <ProgressRow task={task} />
      {/* 底行：时间 + 操作 */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 1 }}>
        <span style={{ fontSize: 10.5, color: "var(--text-faint)" }}>
          {task.startedAtMs ? t("已运行 {__0__}", { __0__: elapsedText(task.startedAtMs, now) }) : ""}
          {" · "}{t("活跃于 {__0__}", { __0__: relTime(task.lastActivityMs, now) })}
        </span>
        <span style={{ flex: 1 }} />
        {task.conversationId && (
          <button
            className="action-btn"
            style={{ fontSize: 10.5, padding: "2px 8px" }}
            onClick={(e) => { e.stopPropagation(); onJump(); }}
            title={t("在 Threadock 中打开已导入的会话")}
          >
            {t("打开会话")}
          </button>
        )}
      </div>
    </div>
  );
}

/** 实时流抽屉：单任务最近活动（2s 轮询）。 */
function TranscriptDrawer({ task, onClose }: { task: LiveTaskDto; onClose: () => void }) {
  const [items, setItems] = useState<LiveActivity[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const rows = await invoke<LiveActivity[]>("live_transcript", {
          source: task.source,
          sessionId: task.sessionId,
          limit: 80,
        });
        if (alive) { setItems(rows); setLoading(false); }
      } catch { if (alive) setLoading(false); }
    };
    load();
    const timer = setInterval(() => { if (!document.hidden) load(); }, DRAWER_POLL_MS);
    return () => { alive = false; clearInterval(timer); };
  }, [task.sessionId, task.source]);

  const meta = liveStateMeta(task.state);
  return (
    <div
      style={{
        position: "fixed", top: 0, right: 0, bottom: 0, width: 460, zIndex: 90,
        background: "var(--bg-panel)", borderLeft: "1px solid var(--border-default)",
        boxShadow: "-12px 0 32px rgba(0,0,0,0.12)", display: "flex", flexDirection: "column",
      }}
      onClick={(e) => e.stopPropagation()}
    >
      {/* 头 */}
      <div style={{
        padding: "12px 14px", borderBottom: "1px solid var(--border-subtle)",
        display: "flex", alignItems: "flex-start", gap: 8,
      }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span style={{
              fontSize: 10.5, fontWeight: 600, padding: "1px 7px", borderRadius: 5,
              background: "var(--accent-bg)", color: "var(--accent)",
            }}>{liveSourceLabel(task.source)}</span>
            <span style={{ fontSize: 11, color: meta.color, fontWeight: 600 }}>{liveStateMeta(task.state).label}</span>
          </div>
          <div style={{
            fontSize: 13, fontWeight: 600, color: "var(--text-primary)", marginTop: 4,
            overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
          }}>{task.title ?? t("（未命名任务）")}</div>
          {task.workspace && (
            <div style={{ fontSize: 11, color: "var(--text-muted)" }}>{task.workspace}</div>
          )}
        </div>
        <button className="action-btn" style={{ padding: "4px 8px" }} onClick={onClose} title={t("关闭")}>
          <Icon name="close" size={14} />
        </button>
      </div>
      {/* 流 */}
      <ScrollArea style={{ flex: 1 }}>
        <div style={{ padding: "10px 14px", display: "flex", flexDirection: "column", gap: 8 }}>
          {loading && <div style={{ fontSize: 12, color: "var(--text-muted)" }}>{t("加载中…")}</div>}
          {!loading && !items.length && (
            <div style={{ fontSize: 12, color: "var(--text-muted)" }}>{t("暂无可展示的活动")}</div>
          )}
          {items.map((it, i) => (
            <div key={i} style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
              <span className="mono" style={{ fontSize: 10, color: "var(--text-faint)", flexShrink: 0, paddingTop: 2, minWidth: 44 }}>
                {it.atMs ? new Date(it.atMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }) : "--:--"}
              </span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <ActivityRow act={it} />
              </div>
            </div>
          ))}
          <div style={{ fontSize: 10.5, color: "var(--text-faint)", paddingBottom: 8 }}>
            {t("每 {__0__} 秒自动刷新", { __0__: DRAWER_POLL_MS / 1000 })}
          </div>
        </div>
      </ScrollArea>
    </div>
  );
}

/** 页面：实时任务。 */
export default function LiveBoard({ onJumpToConversation }: Props) {
  const [dto, setDto] = useState<LiveTasksDto | null>(null);
  const [openTask, setOpenTask] = useState<LiveTaskDto | null>(null);
  // 展示时钟：跟随每次成功扫描更新（0 = 尚未扫描成功，回退 scannedAtMs）
  const [nowState, setNow] = useState(0);
  const [, forceTick] = useState(0);

  const load = useCallback(async () => {
    try {
      const d = await invoke<LiveTasksDto>("live_tasks");
      setNow(Date.now());
      setDto(d);
      // 打开中的任务以最新数据续期（状态徽标随之更新）
      setOpenTask((prev) => (prev ? d.tasks.find((x) => x.source === prev.source && x.sessionId === prev.sessionId) ?? prev : prev));
    } catch { /* 后端不可达时保留上一次结果 */ }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- load 为 async，setState 均发生在 await 之后
    load();
    const timer = setInterval(() => { if (!document.hidden) { load(); forceTick((n) => n + 1); } }, POLL_MS);
    return () => clearInterval(timer);
  }, [load]);

  const now = nowState || dto?.scannedAtMs || 0;

  // 展示层自守排序：Running 最前，同级按最近活跃降序（后端 scan_all 已排，这里兜底）
  const stateRank: Record<string, number> = { running: 0, waiting: 1, idle: 2, recent: 3 };
  const tasks = [...(dto?.tasks ?? [])].sort(
    (a, b) => (stateRank[a.state] ?? 9) - (stateRank[b.state] ?? 9) || b.lastActivityMs - a.lastActivityMs,
  );
  const runningCount = tasks.filter((x) => x.state === "running").length;

  return (
    <div style={{ position: "relative", height: "100%", minWidth: 0 }} onClick={() => { if (openTask) setOpenTask(null); }}>
      <div style={{ padding: "16px 18px", height: "100%", display: "flex", flexDirection: "column", gap: 12, boxSizing: "border-box" }}>
        <CardTitle
          icon="pulse"
          sub={dto ? t("扫描于 {__0__}", { __0__: new Date(dto.scannedAtMs).toLocaleTimeString() }) : undefined}
          trailing={
            <span style={{ fontSize: 11.5, color: "var(--text-muted)" }}>
              {t("{__0__} 个任务 · {__1__} 个运行中", { __0__: tasks.length, __1__: runningCount })}
            </span>
          }
        >
          {t("实时任务")}
        </CardTitle>
        {/* probe 级错误提示（单源失败不影响其他源） */}
        {!!dto?.errors.length && (
          <div style={{
            fontSize: 11.5, color: "#d97706", background: "rgba(245, 158, 11, 0.1)",
            border: "1px solid rgba(245, 158, 11, 0.25)", borderRadius: 8, padding: "6px 10px",
          }}>
            {dto.errors.map((e) => `${liveSourceLabel(e.source)}: ${e.message}`).join("；")}
          </div>
        )}
        {!tasks.length ? (
          <InlineEmpty message={t("近 30 分钟没有活跃的 Agent 会话")}
            hint={t("去任何一个 Agent 里开工，这里会实时亮起来")} />
        ) : (
          <ScrollArea style={{ flex: 1 }}>
            <div style={{
              display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(340px, 1fr))",
              gap: 10, paddingBottom: 12,
            }}>
              {tasks.map((task) => (
                <TaskCard
                  key={`${task.source}:${task.sessionId}`}
                  task={task}
                  now={now}
                  onOpen={() => setOpenTask(task)}
                  onJump={() => task.conversationId && onJumpToConversation(task.conversationId)}
                />
              ))}
            </div>
          </ScrollArea>
        )}
      </div>
      {openTask && <TranscriptDrawer task={openTask} onClose={() => setOpenTask(null)} />}
    </div>
  );
}
