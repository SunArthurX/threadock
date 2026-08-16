// 第 19 轮测试：ScrollArea 自定义滚动条组件
// - 内容溢出时显示 thumb，content 不溢出时不显示
// - thumb 高度 = clientHeight / scrollHeight * clientHeight（minThumbHeight 兜底）
// - 滚动后 thumb top 同步
// - thumb mousedown 拖动 → scrollTop 同步
// - hover 显示 thumb，1s 后自动隐藏
// - ref 转发暴露 inner div（保留原生 scrollTop / scrollTo API）
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { fireEvent, render, act } from "@testing-library/react";
import { useRef } from "react";
import ScrollArea, { type ScrollAreaRef } from "../ScrollArea";

function makeContent(n: number) {
  return Array.from({ length: n }, (_, i) => (
    <div key={i} style={{ height: 50 }}>row {i}</div>
  ));
}

describe("ScrollArea 基础渲染", () => {
  it("渲染 .scroll-area 外层 + .scroll-area-inner 内层", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 300 }}>{makeContent(2)}</ScrollArea>,
    );
    expect(container.querySelector(".scroll-area")).toBeTruthy();
    expect(container.querySelector(".scroll-area-inner")).toBeTruthy();
  });

  it("内容不溢出时不显示 thumb", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 300 }}>{makeContent(2)}</ScrollArea>,
    );
    // jsdom 不算 layout，clientHeight=0 视为不溢出
    expect(container.querySelector('[data-testid="scroll-area-thumb"]')).toBeNull();
  });

  it("设置 clientHeight 后溢出时显示 thumb", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(10)}</ScrollArea>,
    );
    // 模拟 jsdom 中 clientHeight（jsdom 默认 0，需要手动设）
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 500, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    // 初始 scrollTop=0，thumb top 也 = 0，thumbHeight = 100/500*100 = 20 < 30 → 30
    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb).toBeTruthy();
    expect(thumb.style.height).toBe("30px"); // minThumbHeight 兜底
  });
});

describe("ScrollArea ref 转发", () => {
  it("ref.inner 暴露内部 div，保留原生 scrollTop / scrollHeight", () => {
    let captured: ScrollAreaRef | null = null;
    function Wrap() {
      const ref = useRef<ScrollAreaRef>(null);
      return (
        <>
          <ScrollArea ref={ref} style={{ width: 200, height: 200 }}>{makeContent(5)}</ScrollArea>
          <button onClick={() => { captured = ref.current; }}>capture</button>
        </>
      );
    }
    const { container, getByText } = render(<Wrap />);
    fireEvent.click(getByText("capture"));
    expect(captured).toBeTruthy();
    expect(captured?.inner).toBeTruthy();
    expect(captured?.inner).toBe(container.querySelector(".scroll-area-inner"));
  });
});

describe("ScrollArea thumb 位置同步", () => {
  it("scrollTop 变化 → thumb top 同步", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(inner, "scrollTop", { value: 0, configurable: true, writable: true });
    // 触发初始 update
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    // 模拟 scroll 到 50%
    Object.defineProperty(inner, "scrollTop", { value: 450, configurable: true, writable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb).toBeTruthy();
    // scrollable=900, maxTop=100-30=70, newTop=70*450/900=35
    expect(parseFloat(thumb.style.top)).toBeCloseTo(35, 0);
  });
});

describe("ScrollArea thumb 拖动", () => {
  it("mousedown thumb → mousemove 改 thumb top + inner.scrollTop", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(inner, "scrollTop", { value: 0, configurable: true, writable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });

    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb).toBeTruthy();

    // mousedown on thumb
    act(() => {
      fireEvent.mouseDown(thumb, { clientY: 50 });
      document.dispatchEvent(new MouseEvent("mousemove", { clientY: 80, bubbles: true }));
      document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    });
    // dy = 30, startTop = 0, newTop = 0+30 = 30
    // scrollTop = 30 / 70 * 900 = 385.71
    expect(parseFloat(thumb.style.top)).toBeCloseTo(30, 0);
    expect((inner as any).scrollTop).toBeCloseTo(385.71, -1);
  });

  it("mousedown thumb → mouseup 解除拖动", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(inner, "scrollTop", { value: 0, configurable: true, writable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });

    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    act(() => {
      fireEvent.mouseDown(thumb, { clientY: 10 });
      document.dispatchEvent(new MouseEvent("mousemove", { clientY: 50, bubbles: true }));
    });
    const topAfterDown = parseFloat(thumb.style.top);
    expect(topAfterDown).toBeGreaterThan(0);
    // mouseup 后再 mousemove 不应该改变
    act(() => {
      document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      document.dispatchEvent(new MouseEvent("mousemove", { clientY: 999, bubbles: true }));
    });
    expect(parseFloat(thumb.style.top)).toBe(topAfterDown);
  });
});

describe("ScrollArea thumb 颜色 + 边框", () => {
  it("默认 thumb 颜色 = rgba(148, 163, 199, 0.28)", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb.style.background).toBe("rgba(148, 163, 199, 0.28)");
    expect(thumb.style.borderRadius).toBe("4px"); // thumbWidth=8 / 2
    expect(thumb.style.right).toBe("2px");
  });

  it("hover thumb 颜色变深", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    const scrollArea = container.querySelector(".scroll-area") as HTMLElement;
    fireEvent.mouseEnter(scrollArea);
    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb.style.background).toBe("rgba(148, 163, 199, 0.55)");
    expect(thumb.style.opacity).toBe("1");
  });

  it("thumbWidth 自定义", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }} thumbWidth={12}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb.style.width).toBe("12px");
    expect(thumb.style.borderRadius).toBe("6px");
  });
});

describe("ScrollArea 隐藏原生滚动条", () => {
  it(".scroll-area-inner 设置 scrollbar-width: none", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    expect(inner.style.scrollbarWidth).toBe("none");
  });

  it("注入 style 标签隐藏 webkit 滚动条", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const style = container.querySelector("style");
    expect(style?.textContent).toContain("::-webkit-scrollbar");
    expect(style?.textContent).toContain("display: none");
  });
});
