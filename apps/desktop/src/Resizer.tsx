// 通用拖拽条：在两栏之间竖插（axis="x"，默认），或上下区域之间横插（axis="y"）。
// mousedown 后 mousemove 把偏移量通过 onDrag 回调给父组件。
// 用法：<Resizer onDrag={(dx) => setWidth(w => clamp(w + dx, 240, 540))} />
//      <Resizer axis="y" onDrag={(dy) => setHeight(h => clamp(h - dy, 160, 640))} />
// 默认 6px 宽，hover 时变蓝色，拖拽时 body 加 select-none 避免选中文本。
import { useEffect, useRef, useState } from "react";

export default function Resizer({
  onDrag,
  title = "拖拽调整宽度",
  className = "",
  axis = "x",
}: {
  /** 拖拽时回调，dx/dy = 鼠标沿拖拽轴的偏移（正数 = 向右 / 向下）。 */
  onDrag: (d: number) => void;
  title?: string;
  className?: string;
  /** 拖拽轴：x = 竖条左右拖（默认），y = 横条上下拖。 */
  axis?: "x" | "y";
}) {
  const [dragging, setDragging] = useState(false);
  const lastX = useRef(0);
  const lastY = useRef(0);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      if (axis === "y") {
        const dy = e.clientY - lastY.current;
        lastY.current = e.clientY;
        onDrag(dy);
      } else {
        const dx = e.clientX - lastX.current;
        lastX.current = e.clientX;
        onDrag(dx);
      }
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
  }, [dragging, onDrag, axis]);

  return (
    <div
      role="separator"
      aria-orientation={axis === "y" ? "horizontal" : "vertical"}
      className={`resizer ${axis === "y" ? "resizer-h" : ""} ${dragging ? "active" : ""} ${className}`}
      title={title}
      onMouseDown={(e) => {
        e.preventDefault();
        lastX.current = e.clientX;
        lastY.current = e.clientY;
        setDragging(true);
        document.body.style.userSelect = "none";
        document.body.style.cursor = axis === "y" ? "row-resize" : "col-resize";
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
