// 轻量通知（toast）系统：模块级 store + useSyncExternalStore，免 prop 透传。
// 支持 undo 按钮：showToast(text, kind, ttlMs, undo?)

export type ToastKind = "info" | "warn" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
  /** 撤销回调：点「撤销」时触发，3 秒后失效。 */
  undo?: () => void;
  /** undo 按钮文本，默认「撤销」。 */
  undoLabel?: string;
}

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

const emit = () => listeners.forEach((l) => l());

/** 测试钩子：重置全局状态。 */
export const _resetToasts = () => {
  toasts = [];
  nextId = 1;
  emit();
};

export function subscribeToasts(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function getToasts(): Toast[] {
  return toasts;
}

/** React useSyncExternalStore 快照（返回稳定引用直到变更）。 */
export const toastSnapshot = getToasts;

/** 弹出一条通知（默认 5 秒自动消失）。 */
const MAX_TOASTS = 4;
const DEFAULT_UNDO_TTL_MS = 6000;

export function showToast(
  text: string,
  kind: ToastKind = "info",
  ttlMs: number = DEFAULT_UNDO_TTL_MS,
  undo?: () => void,
  undoLabel: string = "撤销",
): number {
  const id = nextId++;
  toasts = [
    ...toasts,
    { id, kind, text, undo, undoLabel },
  ].slice(-MAX_TOASTS); // 溢出丢最旧，避免堆叠刷屏
  emit();
  window.setTimeout(() => dismissToast(id), ttlMs);
  return id;
}

export function dismissToast(id: number): void {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}
