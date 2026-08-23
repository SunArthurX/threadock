// 通知堆叠容器（右下角，自动消失由 toast store 管理）
// 支持 undo：若 toast 携带 undo 回调，渲染一个撤销按钮
import type { Toast } from "./toast";
import { t } from "./i18n";
import { Icon, type IconName } from "./Icon";

const TOAST_ICON: Record<Toast["kind"], IconName> = {
  info: "info",
  warn: "warning",
  error: "alert",
};

export function Toasts({ toasts, onDismiss }: { toasts: Toast[]; onDismiss: (id: number) => void }) {
  if (toasts.length === 0) return null;
  return (
    <div className="toasts">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.kind}`} role="status">
          <span className="toast-icon"><Icon name={TOAST_ICON[toast.kind] ?? "info"} size={14} /></span>
          <span className="toast-text" onClick={() => onDismiss(toast.id)}>{toast.text}</span>
          {toast.undo && (
            <button
              className="toast-undo"
              onClick={(e) => { e.stopPropagation(); toast.undo?.(); onDismiss(toast.id); }}
            >{toast.undoLabel ?? t("撤销")}</button>
          )}
        </div>
      ))}
    </div>
  );
}
