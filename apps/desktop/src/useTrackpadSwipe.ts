// 触控板双指横滑检测（对话视图左右栏：左滑 → 下一会话，右滑 → 上一会话）。
// 纯决策函数 swipeStep 独立导出便于矩阵测试；只在横向意图明显
// （|deltaX| > |deltaY| 且单次 |deltaX| ≥ 6）时累积，过阈值触发一次导航，
// 带冷却与手势停顿重置；目标处于可横向滚动的内容（如 payload JSON 块）
// 时让位给原生滚动，不劫持。
import { useCallback, useEffect, useRef } from "react";
import type { WheelEvent as ReactWheelEvent } from "react";

export interface SwipeState {
  /** 累积的横向位移（负 = 向左滑）。 */
  acc: number;
  /** 最近一次 wheel 的时间戳（ms）。 */
  lastT: number;
  /** 最近一次触发导航的时间戳（ms），用于冷却。 */
  firedAt: number;
}

export const SWIPE_THRESHOLD = 110;
export const SWIPE_COOLDOWN_MS = 700;
export const SWIPE_IDLE_RESET_MS = 300;
/** 单次事件计入累积的最小横向位移（过滤抖动）。 */
export const SWIPE_MIN_DELTA = 6;

export function swipeInit(now = 0): SwipeState {
  return { acc: 0, lastT: now, firedAt: 0 };
}

/**
 * 纯决策：一次 wheel 的 delta + 当前状态 → 新状态与是否触发。
 * fire: 1 = 左滑（下一会话）、-1 = 右滑（上一会话）、0 = 不触发。
 */
export function swipeStep(
  s: SwipeState,
  deltaX: number,
  deltaY: number,
  now: number,
): { state: SwipeState; fire: 0 | 1 | -1 } {
  if (now - s.lastT > SWIPE_IDLE_RESET_MS) s = swipeInit(now);
  if (Math.abs(deltaX) > Math.abs(deltaY) && Math.abs(deltaX) >= SWIPE_MIN_DELTA) {
    s = { ...s, acc: s.acc + deltaX, lastT: now };
  } else {
    s = { ...s, lastT: now };
  }
  if (s.firedAt > 0 && now - s.firedAt < SWIPE_COOLDOWN_MS) return { state: { ...s, acc: 0 }, fire: 0 };
  if (s.acc <= -SWIPE_THRESHOLD) return { state: { acc: 0, lastT: now, firedAt: now }, fire: 1 };
  if (s.acc >= SWIPE_THRESHOLD) return { state: { acc: 0, lastT: now, firedAt: now }, fire: -1 };
  return { state: s, fire: 0 };
}

/** 事件目标到容器之间是否存在可横向滚动的内容（让位给原生滚动）。 */
export function hasHorizontalOverflow(target: Element | null, bound: Element | null): boolean {
  for (let el = target; el && el !== bound; el = el.parentElement) {
    if (el.scrollWidth > el.clientWidth + 2) return true;
  }
  return false;
}

/** 返回稳定的 onWheel 处理器；回调经 ref 保鲜，不随渲染重建。 */
export function useTrackpadSwipe(onNext: () => void, onPrev: () => void) {
  const state = useRef<SwipeState>(swipeInit());
  const cbs = useRef({ onNext, onPrev });
  useEffect(() => {
    cbs.current = { onNext, onPrev };
  });
  return useCallback((e: ReactWheelEvent) => {
    if (hasHorizontalOverflow(e.target as Element | null, e.currentTarget as Element | null)) return;
    const { state: next, fire } = swipeStep(state.current, e.deltaX, e.deltaY, Date.now());
    state.current = next;
    if (fire === 1) cbs.current.onNext();
    else if (fire === -1) cbs.current.onPrev();
  }, []);
}
