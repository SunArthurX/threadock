// 触控板横滑检测：纯决策矩阵 + 横向溢出让位判定
import { describe, expect, it } from "vitest";
import {
  SWIPE_COOLDOWN_MS,
  SWIPE_IDLE_RESET_MS,
  SWIPE_THRESHOLD,
  hasHorizontalOverflow,
  swipeInit,
  swipeStep,
} from "../useTrackpadSwipe";

describe("swipeStep（纯决策）", () => {
  it("未过阈值只累积不触发", () => {
    let s = swipeInit(0);
    for (let i = 0; i < 5; i++) {
      const r = swipeStep(s, -20, 0, 100 + i * 50);
      expect(r.fire).toBe(0);
      s = r.state;
    }
    expect(s.acc).toBe(-100); // 5 × -20
  });

  it("左滑过阈值触发下一会话（fire=1），触发后累积清零", () => {
    let s = swipeInit(0);
    let fired = 0;
    for (let i = 0; i < 8; i++) {
      const r = swipeStep(s, -20, 0, 100 + i * 50);
      if (r.fire !== 0) fired = r.fire;
      s = r.state;
    }
    expect(fired).toBe(1);
    expect(s.acc).toBe(0);
    expect(s.firedAt).toBeGreaterThan(0);
  });

  it("右滑过阈值触发上一会话（fire=-1）", () => {
    let s = swipeInit(0);
    let fired = 0;
    for (let i = 0; i < 8; i++) {
      const r = swipeStep(s, 18, 0, 100 + i * 50);
      if (r.fire !== 0) fired = r.fire;
      s = r.state;
    }
    expect(fired).toBe(-1);
  });

  it("冷却期内不重复触发", () => {
    let s = swipeInit(0);
    let fires = 0;
    let t = 100;
    for (let burst = 0; burst < 3; burst++) {
      for (let i = 0; i < 8; i++) {
        const r = swipeStep(s, -20, 0, t);
        if (r.fire !== 0) fires += 1;
        s = r.state;
        t += 30; // 冷却期内连续猛滑
      }
    }
    expect(fires).toBe(1); // 3 轮全在 700ms 冷却内 → 只触发一次
  });

  it("冷却结束后可再次触发", () => {
    let s = swipeInit(0);
    let fires = 0;
    let t = 100;
    for (let round = 0; round < 2; round++) {
      for (let i = 0; i < 8; i++) {
        const r = swipeStep(s, -20, 0, t);
        if (r.fire !== 0) fires += 1;
        s = r.state;
        t += 40;
      }
      t += SWIPE_COOLDOWN_MS; // 等 cooldown 过去
    }
    expect(fires).toBe(2);
  });

  it("手势停顿超过 IDLE_RESET 后累积重置", () => {
    const s = swipeInit(0);
    const r1 = swipeStep(s, -80, 0, 100);
    expect(r1.fire).toBe(0);
    const r2 = swipeStep(r1.state, -80, 0, 100 + SWIPE_IDLE_RESET_MS + 50);
    expect(r2.fire).toBe(0); // 旧累积被重置，新一轮只有 -80
    expect(r2.state.acc).toBe(-80);
  });

  it("纵向意图主导（|dy| ≥ |dx|）不计入累积", () => {
    let s = swipeInit(0);
    for (let i = 0; i < 20; i++) {
      s = swipeStep(s, -30, 40, 100 + i * 50).state;
    }
    expect(s.acc).toBe(0);
  });

  it("微小抖动（|dx| < 6）不计入累积", () => {
    let s = swipeInit(0);
    for (let i = 0; i < 50; i++) {
      s = swipeStep(s, -5, 0, 100 + i * 10).state;
    }
    expect(s.acc).toBe(0);
  });

  it("阈值常量合理性", () => {
    expect(SWIPE_THRESHOLD).toBeGreaterThan(0);
    expect(SWIPE_COOLDOWN_MS).toBeGreaterThan(0);
  });
});

describe("hasHorizontalOverflow（让位原生横向滚动）", () => {
  const setGeom = (el: HTMLElement, scrollWidth: number, clientWidth: number) => {
    Object.defineProperty(el, "scrollWidth", { configurable: true, get: () => scrollWidth });
    Object.defineProperty(el, "clientWidth", { configurable: true, get: () => clientWidth });
  };

  it("链路上存在横向溢出元素 → true", () => {
    const bound = document.createElement("div");
    const wide = document.createElement("div");
    const target = document.createElement("span");
    bound.appendChild(wide);
    wide.appendChild(target);
    setGeom(wide, 800, 300);
    expect(hasHorizontalOverflow(target, bound)).toBe(true);
  });

  it("无横向溢出 → false", () => {
    const bound = document.createElement("div");
    const row = document.createElement("div");
    const target = document.createElement("span");
    bound.appendChild(row);
    row.appendChild(target);
    setGeom(row, 300, 300);
    expect(hasHorizontalOverflow(target, bound)).toBe(false);
  });

  it("容器自身的溢出不算（只查 target 与 bound 之间）", () => {
    const bound = document.createElement("div");
    setGeom(bound, 999, 100);
    const target = document.createElement("span");
    bound.appendChild(target);
    expect(hasHorizontalOverflow(target, bound)).toBe(false);
  });
});
