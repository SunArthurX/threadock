// 通用自定义滚动区域：保留原生滚动行为（wheel/键盘/触摸板），用 React 绘制 thumb
// 解决 WKWebView overlay 滚动条颜色不可控问题（深色主题下原生 thumb 显示为亮白）
//
// 设计：
// - 内部 div 走原生 overflow-y: auto，wheel/键盘/触摸板全部走浏览器
// - 用 CSS `::-webkit-scrollbar { display: none }` + `scrollbar-width: none` 隐藏原生滚动条
// - 自定义 thumb 用绝对定位 + 监听原生 scroll 事件 + ResizeObserver 同步
// - thumb 支持 mousedown 拖动（不阻塞原生滚动，hover 时显形，1s 后渐隐）
// - 横向滚动暂不支持（项目内 .heatmap-scroll / .ops-table-wrap 等横向区域走原生即可）
//
// ref 转发：forwardRef 指向内部滚动容器（保留原生 scrollTop / scrollTo API）
import {
  forwardRef, useEffect, useImperativeHandle, useRef, useState,
  type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode,
} from "react";

export interface ScrollAreaRef {
  /** 内部原生滚动 div（保留 scrollTop / scrollTo / scrollHeight / clientHeight） */
  inner: HTMLDivElement | null;
}

export interface ScrollAreaProps {
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  /** thumb 宽度（默认 8px） */
  thumbWidth?: number;
  /** thumb 颜色（默认深灰蓝 0.28 alpha） */
  thumbColor?: string;
  /** hover 颜色（默认 0.55 alpha） */
  thumbHoverColor?: string;
  /** thumb 距离右边距离（默认 2px） */
  thumbOffset?: number;
  /** thumb 最小高度（默认 30px，避免太短） */
  minThumbHeight?: number;
  /** 透传到内部 div 的 mousedown（如 .tag-suggest 阻止 input blur） */
  onMouseDown?: (e: ReactMouseEvent<HTMLDivElement>) => void;
}

const HIDE_DELAY_MS = 1000;

const ScrollArea = forwardRef<ScrollAreaRef, ScrollAreaProps>(function ScrollArea(
  {
    children,
    className = "",
    style,
    thumbWidth = 8,
    thumbColor = "rgba(148, 163, 199, 0.28)",
    thumbHoverColor = "rgba(148, 163, 199, 0.55)",
    thumbOffset = 2,
    minThumbHeight = 30,
    onMouseDown,
  },
  ref,
) {
  const innerRef = useRef<HTMLDivElement>(null);
  const [thumbHeight, setThumbHeight] = useState(0);
  const [thumbTop, setThumbTop] = useState(0);
  const [showThumb, setShowThumb] = useState(false);
  const [hover, setHover] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const hideTimerRef = useRef<number | null>(null);

  // forwardRef：暴露 inner div 给父组件（每次重渲染更新 ref.inner）
  useImperativeHandle(ref, () => ({ inner: innerRef.current }), []);

  // 监听内容 + 容器尺寸 + 滚动位置 → 更新 thumb
  useEffect(() => {
    const inner = innerRef.current;
    if (!inner) return;
    const parent = inner.parentElement;
    if (!parent) return;

    const update = () => {
      const ch = inner.scrollHeight;
      const sh = parent.clientHeight;
      if (ch <= sh || sh <= 0) {
        setThumbHeight(0);
        return;
      }
      const ratio = sh / ch;
      const th = Math.max(minThumbHeight, sh * ratio);
      setThumbHeight(th);
      const maxTop = Math.max(0, sh - th);
      const scrollable = ch - sh;
      setThumbTop(scrollable > 0 ? (inner.scrollTop / scrollable) * maxTop : 0);
    };

    update();
    const ro = new ResizeObserver(update);
    ro.observe(inner);
    ro.observe(parent);
    inner.addEventListener("scroll", update, { passive: true });
    return () => {
      ro.disconnect();
      inner.removeEventListener("scroll", update);
    };
  }, [minThumbHeight]);

  // thumb 自动隐藏计时器
  useEffect(() => {
    if (hideTimerRef.current) {
      clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    if (showThumb && !hover && !isDragging) {
      hideTimerRef.current = window.setTimeout(() => setShowThumb(false), HIDE_DELAY_MS);
    }
    return () => {
      if (hideTimerRef.current) clearTimeout(hideTimerRef.current);
    };
  }, [showThumb, hover, isDragging]);

  // 滚动时显示 thumb
  useEffect(() => {
    setShowThumb(true);
  }, [thumbTop]);

  // thumb 拖动
  const dragStateRef = useRef<{ startY: number; startTop: number; maxTop: number } | null>(null);
  const onThumbMouseDown = (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const inner = innerRef.current;
    if (!inner) return;
    const parent = inner.parentElement;
    if (!parent) return;
    const sh = parent.clientHeight;
    const maxTop = Math.max(0, sh - thumbHeight);
    dragStateRef.current = { startY: e.clientY, startTop: thumbTop, maxTop };
    setIsDragging(true);
    setShowThumb(true);

    const onMove = (ev: globalThis.MouseEvent) => {
      const state = dragStateRef.current;
      if (!state) return;
      const dy = ev.clientY - state.startY;
      const newTop = Math.max(0, Math.min(state.maxTop, state.startTop + dy));
      setThumbTop(newTop);
      const ch2 = inner.scrollHeight;
      const sh2 = parent.clientHeight;
      const scrollable = ch2 - sh2;
      const maxTop2 = Math.max(0, sh2 - thumbHeight);
      if (maxTop2 > 0) inner.scrollTop = (newTop / maxTop2) * scrollable;
    };
    const onUp = () => {
      dragStateRef.current = null;
      setIsDragging(false);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const hasScroll = thumbHeight > 0;

  return (
    <div
      className={`scroll-area ${className}`}
      style={{
        position: "relative",
        overflow: "hidden",
        ...style,
      }}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      <div
        ref={innerRef}
        className="scroll-area-inner"
        onMouseDown={onMouseDown}
        style={{
          height: "100%",
          width: "100%",
          overflowY: "auto",
          overflowX: "hidden",
          scrollbarWidth: "none",
        }}
      >
        {children}
      </div>
      {hasScroll && (
        <div
          data-testid="scroll-area-thumb"
          onMouseDown={onThumbMouseDown}
          style={{
            position: "absolute",
            top: thumbTop,
            right: thumbOffset,
            width: thumbWidth,
            height: thumbHeight,
            borderRadius: thumbWidth / 2,
            background: hover || isDragging ? thumbHoverColor : thumbColor,
            opacity: showThumb || hover || isDragging ? 1 : 0,
            transition: "opacity 0.2s, background 0.15s",
            cursor: isDragging ? "grabbing" : "grab",
            zIndex: 10,
          }}
        />
      )}
      <style>{`.scroll-area-inner::-webkit-scrollbar { display: none; }`}</style>
    </div>
  );
});

export default ScrollArea;

