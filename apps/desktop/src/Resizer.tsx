// 通用竖向拖拽条：在两栏之间插入，mousedown 后 mousemove 把 x 偏移通过 onDrag 回调给父组件。
// 用法：<Resizer onDrag={(dx) => setWidth(w => clamp(w + dx, 240, 540))} />
// 默认 6px 宽，hover 时变蓝色，拖拽时 body 加 select-none 避免选中文本。
import { useEffect, useRef, useState } from "react";

export default function Resizer({
  onDrag,
  title = "拖拽调整宽度",
  className = "",
}: {
  /** 拖拽时回调，dx = 鼠标 X 偏移（正数 = 向右）。 */
  onDrag: (dx: number) => void;
  title?: string;
  className?: string;
}) {
  const [dragging, setDragging] = useState(false);
  const lastX = useRef(0);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      const dx = e.clientX - lastX.current;
      lastX.current = e.clientX;
      onDrag(dx);
    };
    const onUp = () => {
      setDragging(false);
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    return () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
  }, [dragging, onDrag]);

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      className={`resizer ${dragging ? "active" : ""} ${className}`}
      title={title}
      onMouseDown={(e) => {
        e.preventDefault();
        lastX.current = e.clientX;
        setDragging(true);
        document.body.style.userSelect = "none";
        document.body.style.cursor = "col-resize";
      }}
    />
  );
}

/** 读 localStorage 数字（带 fallback + 范围 clamp）。 */
export function loadClampedNumber(key: string, fallback: number, min: number, max: number): number {
  try {
    const v = Number(localStorage.getItem(key));
    if (Number.isFinite(v) && v >= min && v <= max) return v;
  } catch { /* 静默 */ }
  return fallback;
}
export function saveNumber(key: string, v: number) {
  try { localStorage.setItem(key, String(v)); } catch { /* 静默 */ }
}
