// 轻量通知（toast）系统：模块级 store + useSyncExternalStore，免 prop 透传。
import { render } from "./renderSchedule";

export type ToastKind = "info" | "warn" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();
/** 测试钩子：重置全局状态。 */
export const _resetToasts = () => {
  toasts = [];
  nextId = 1;
  listeners.forEach((l) => l());
};

const emit = () => listeners.forEach((l) => l());

export function subscribeToasts(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function getToasts(): Toast[] {
  return toasts;
}

/** 弹出一条通知（默认 5 秒自动消失）。 */
export function showToast(text: string, kind: ToastKind = "info", ttlMs = 5000): number {
  const id = nextId++;
  toasts = [...toasts, { id, kind, text }];
  emit();
  render(() => {
    window.setTimeout(() => dismissToast(id), ttlMs);
  });
  return id;
}

export function dismissToast(id: number): void {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

/** React 绑定（App 里一次 useSyncExternalStore）。 */
export function toastSnapshot(): Toast[] {
  return toasts;
}
