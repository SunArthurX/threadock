// 通知堆叠容器（右下角，自动消失由 toast store 管理）
// 支持 undo：若 toast 携带 undo 回调，渲染一个撤销按钮
import type { Toast } from "./toast";

export function Toasts({ toasts, onDismiss }: { toasts: Toast[]; onDismiss: (id: number) => void }) {
  if (toasts.length === 0) return null;
  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast-${t.kind}`}>
          <span className="toast-text" onClick={() => onDismiss(t.id)}>{t.text}</span>
          {t.undo && (
            <button
              className="toast-undo"
              onClick={(e) => { e.stopPropagation(); t.undo?.(); onDismiss(t.id); }}
            >↩ {t.undoLabel ?? "撤销"}</button>
          )}
        </div>
      ))}
    </div>
  );
}
