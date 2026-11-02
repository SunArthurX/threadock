// 实时任务监控（Live Board）前端类型与展示工具。
// 后端对应 apps/desktop/src-tauri/src/commands/live.rs（serde camelCase）。

export type LiveSource = "claude-code" | "codex" | "zcode" | "minimax-code";
export type LiveState = "running" | "waiting" | "idle" | "recent" | "today";

export interface LiveActivity {
  kind: string; // message / tool / thinking / system / status
  role: string | null; // user / assistant（kind = message 时）
  snippet: string;
  atMs: number;
}

export interface LiveProgress {
  todosDone: number | null;
  todosTotal: number | null;
  messageCount: number | null;
  totalTokens: number | null;
}

export interface LiveTask {
  source: LiveSource;
  sessionId: string;
  title: string | null;
  workspace: string | null;
  /** Codex 专用：codex_work_desktop（桌面）/ codex_cli_rs（CLI）。 */
  originator: string | null;
  startedAtMs: number | null;
  lastActivityMs: number;
  state: LiveState;
  activity: LiveActivity | null;
  progress: LiveProgress | null;
}

/** 单任务 + 深链回填。 */
export interface LiveTaskDto extends LiveTask {
  conversationId: string | null;
}

export interface LiveTasksDto {
  tasks: LiveTaskDto[];
  errors: { source: LiveSource; message: string }[];
  scannedAtMs: number;
}

import { t } from "./i18n";

/** 源展示名。 */
export function liveSourceLabel(source: LiveSource): string {
  switch (source) {
    case "claude-code": return "Claude Code";
    case "codex": return "Codex";
    case "zcode": return "ZCode";
    case "minimax-code": return "MiniMax Code";
  }
}

/** 状态元数据：标签 + 主色 + 浅底色。 */
/* today：今天零点后有过活动、已超 30 分钟（当天已结束仍可回看） */
export function liveStateMeta(state: LiveState): { label: string; color: string; bg: string } {
  switch (state) {
    case "running": return { label: t("运行中"), color: "#16a34a", bg: "rgba(34, 197, 94, 0.12)" };
    case "waiting": return { label: t("等待输入"), color: "#d97706", bg: "rgba(245, 158, 11, 0.14)" };
    case "idle": return { label: t("空闲"), color: "#64748b", bg: "rgba(100, 116, 139, 0.12)" };
    case "recent": return { label: t("刚活跃"), color: "#2563eb", bg: "rgba(59, 130, 246, 0.12)" };
    case "today": return { label: t("今日"), color: "#7c3aed", bg: "rgba(139, 92, 246, 0.12)" };
  }
}

/** Codex 端别徽标（桌面 / CLI），非 codex 返回 null。 */
export function originatorBadge(source: LiveSource, originator: string | null): string | null {
  if (source !== "codex" || !originator) return null;
  const s = originator.toLowerCase();
  if (s.includes("desktop") || s.includes("work") || s.includes("vscode") || s.includes("ide")) {
    return t("桌面端");
  }
  if (s.includes("cli")) return "CLI";
  return null;
}

/** 相对时间：「x 秒前 / x 分钟前 / x 小时前」。 */
export function relTime(ms: number, now: number): string {
  const sec = Math.max(0, Math.round((now - ms) / 1000));
  if (sec < 60) return t("{__0__} 秒前", { __0__: sec });
  const min = Math.floor(sec / 60);
  if (min < 60) return t("{__0__} 分钟前", { __0__: min });
  return t("{__0__} 小时前", { __0__: Math.floor(min / 60) });
}

/** 已运行时长：「x 分 / x 小时 y 分」。 */
export function elapsedText(startedMs: number, now: number): string {
  const min = Math.max(1, Math.round((now - startedMs) / 60_000));
  if (min < 60) return t("{__0__} 分", { __0__: min });
  return t("{__0__} 小时 {__1__} 分", { __0__: Math.floor(min / 60), __1__: min % 60 });
}

/** 工作区展示名：取路径末段。 */
export function workspaceLabel(workspace: string | null): string | null {
  if (!workspace) return null;
  const seg = workspace.replace(/\/+$/, "").split("/").pop();
  return seg || workspace;
}

/** token 数缩写：1.2k / 34.5k / 1.2M。 */
export function tokenShort(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}
