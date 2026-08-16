// 通用自定义滚动区域：保留原生滚动行为（wheel/键盘/触摸板），用 React 绘制 thumb
// 解决 WKWebView overlay 滚动条颜色不可控问题（深色主题下原生 thumb 显示为亮白）
//
// 设计：
// - 单一 div 同时承担容器（flex: 1 / min-height: 0）和内部滚动（overflow-y: auto）
// - 用 CSS `::-webkit-scrollbar { display: none }` + `scrollbar-width: none` 隐藏原生滚动条
// - 自定义 thumb 用 position: sticky 浮在容器内右上角，跟随 scrollTop 更新 top
// - thumb 支持 mousedown 拖动（不阻塞原生滚动，hover 时显形，1s 后渐隐）
// - 横向滚动暂不支持（项目内 .heatmap-scroll / .ops-table-wrap 等横向区域走原生即可）
//
// 关键坑：早期版本用外层 position:relative + overflow:hidden 包裹 inner overflow:auto，
// macOS WKWebView 下 nested scroll 链断裂 → wheel 事件被截断，**完全不能滚**。
// 现在改为单一 div：滚轮直接打到滚动容器，无中间层截断。
//
// ref 转发：forwardRef 指向滚动 div（保留原生 scrollTop / scrollTo API）
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
    // thumbHoverColor 保留 prop API；当前 hover 状态由 useEffect 直接设到 DOM
    thumbHoverColor: _thumbHoverColor,
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

    const update = () => {
      const ch = inner.scrollHeight;
      const sh = inner.clientHeight;
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
    const sh = inner.clientHeight;
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
      const sh2 = inner.clientHeight;
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

  // thumb 位置：position fixed + 跟着 inner 位置 + scrollTop 同步
  // innerRef 在每次 render 时可能不同，但 thumb 渲染是 state-driven → 每次 state 变都 re-render
  // 页面整体滚动时 inner.getBoundingClientRect().top 会变 → 用 ref + rAF 直接更新 DOM 避免 re-render 风暴
  const thumbRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const inner = innerRef.current;
    const thumb = thumbRef.current;
    if (!inner || !thumb) return;
    let raf = 0;
    const update = () => {
      const r = inner.getBoundingClientRect();
      const visible = thumbHeight > 0;
      if (visible) {
        thumb.style.top = `${r.top + thumbTop + thumbOffset}px`;
        thumb.style.right = `${Math.max(0, window.innerWidth - r.right + thumbOffset)}px`;
        thumb.style.opacity = showThumb || hover || isDragging ? "1" : "0";
      } else {
        thumb.style.opacity = "0";
      }
    };
    const onScroll = () => { if (raf) return; raf = requestAnimationFrame(() => { raf = 0; update(); }); };
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    inner.addEventListener("scroll", onScroll);
    update();
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
      inner.removeEventListener("scroll", onScroll);
      if (raf) cancelAnimationFrame(raf);
    };
  }, [thumbTop, thumbHeight, thumbOffset, showThumb, hover, isDragging]);

  const hasScroll = thumbHeight > 0;

  return (
    <div style={{ display: "contents" }}>
      {/* 滚动容器：直接放在父级 grid/flex 布局里。
          关键修复：滚轮事件直接打到此 div（不再被外层 overflow:hidden 截断）。 */}
      <div
        ref={innerRef}
        data-testid="scroll-area-inner"
        className={`scroll-area-inner ${className}`}
        onMouseDown={onMouseDown}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
        style={{
          display: "block",
          // flex 容器里用 min-height:0 才能让 overflow:auto 触发滚动
          minHeight: 0,
          // grid item 默认 stretch → 高度 = 父级 grid track 高度
          overflowY: "auto",
          overflowX: "hidden",
          scrollbarWidth: "none",
          // 强制隐藏 webkit 原生滚动条
          ...style,
        }}
      >
        {children}
        <style>{`.scroll-area-inner::-webkit-scrollbar { display: none; }`}</style>
      </div>
      {/* thumb：position fixed 浮层，display:contents 不占 grid cell。
          位置由 useEffect rAF 节流同步（页面/inner/resize 滚动时更新）。 */}
      {hasScroll && (
        <div
          ref={thumbRef}
          data-testid="scroll-area-thumb"
          onMouseDown={onThumbMouseDown}
          style={{
            position: "fixed",
            top: 0,
            right: 0,
            width: thumbWidth,
            height: thumbHeight,
            borderRadius: thumbWidth / 2,
            background: thumbColor,
            opacity: 0,
            transition: "opacity 0.2s, background 0.15s",
            cursor: "grab",
            zIndex: 10,
            pointerEvents: "auto",
          }}
        />
      )}
    </div>
  );
});

export default ScrollArea;

