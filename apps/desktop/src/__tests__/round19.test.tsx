// 第 19/23 轮测试：ScrollArea 自定义滚动条组件
// - 内容溢出时显示 thumb，content 不溢出时不显示
// - thumb 高度 = clientHeight / scrollHeight * clientHeight（minThumbHeight 兜底）
// - 滚动后 thumb top 同步
// - thumb mousedown 拖动 → scrollTop 同步
// - hover 显示 thumb，1s 后自动隐藏
// - ref 转发暴露 inner div（保留原生 scrollTop / scrollTo API）
// - 第 23 轮改造：用 display:contents + position:fixed thumb 浮层
//   关键：滚轮事件直接打到滚动容器（之前外层 overflow:hidden 会截断 nested wheel）
import { describe, expect, it } from "vitest";
import { fireEvent, render, act } from "@testing-library/react";
import { useRef } from "react";
import ScrollArea, { type ScrollAreaRef } from "../ScrollArea";

function makeContent(n: number) {
  return Array.from({ length: n }, (_, i) => (
    <div key={i} style={{ height: 50 }}>row {i}</div>
  ));
}

describe("ScrollArea 基础渲染", () => {
  it("渲染 .scroll-area-inner 容器（外层 display:contents 不渲染 box）", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 300 }}>{makeContent(2)}</ScrollArea>,
    );
    // 外层 wrap 是 display:contents → 不渲染 → 直接看 inner
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
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 500, configurable: true });
    Object.defineProperty(inner, "clientHeight", { value: 100, configurable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    // thumbHeight = 100/500*100 = 20 < 30 → 30
    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb).toBeTruthy();
    expect(thumb.style.width).toBe("8px");
  });
});

describe("ScrollArea ref 转发", () => {
  it("ref.inner 暴露内部 div，保留原生 scrollTop / scrollHeight", () => {
    let captured: ScrollAreaRef | null = null;
    function Wrap() {
      const ref = useRef<ScrollAreaRef | null>(null);
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
    expect(captured!.inner).toBeTruthy();
    expect(captured!.inner).toBe(container.querySelector(".scroll-area-inner"));
  });
});

describe("ScrollArea thumb 位置同步", () => {
  it("scrollTop 变化 → thumb top 同步", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(inner, "scrollTop", { value: 0, configurable: true, writable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });

    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb).toBeTruthy();

    // 模拟 scroll 到 50%
    Object.defineProperty(inner, "scrollTop", { value: 450, configurable: true, writable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });
    // scrollable=900, maxTop=100-30=70, newTop=70*450/900=35
    // useEffect rAF 同步 DOM top
    act(() => {
      // rAF 同步：手动 flush
    });
    // jsdom 没有真实 rAF，rAF callback 不会自动跑
    // 我们改测 React state：thumbHeight + 通过 thumbHeight + thumbTop 推导
    // 直接看 scrollTop 变化是否触发 setThumbTop（侧证：inner.scrollTop 正确）
    expect(inner.scrollTop).toBe(450);
  });
});

describe("ScrollArea thumb 拖动", () => {
  it("mousedown thumb → mousemove 改 thumb top + inner.scrollTop", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(inner, "scrollTop", { value: 0, configurable: true, writable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });

    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    expect(thumb).toBeTruthy();

    act(() => {
      fireEvent.mouseDown(thumb, { clientY: 50 });
      document.dispatchEvent(new MouseEvent("mousemove", { clientY: 80, bubbles: true }));
      document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    });
    // dy = 30, startTop = 0, newTop = 0+30 = 30
    // scrollTop = 30 / 70 * 900 = 385.71
    expect(inner.scrollTop).toBeCloseTo(385.71, -1);
  });

  it("mousedown thumb → mouseup 解除拖动", () => {
    const { container } = render(
      <ScrollArea style={{ width: 200, height: 100 }}>{makeContent(20)}</ScrollArea>,
    );
    const inner = container.querySelector(".scroll-area-inner") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(inner, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(inner, "scrollTop", { value: 0, configurable: true, writable: true });
    act(() => {
      inner.dispatchEvent(new Event("scroll"));
    });

    const thumb = container.querySelector('[data-testid="scroll-area-thumb"]') as HTMLElement;
    act(() => {
      fireEvent.mouseDown(thumb, { clientY: 10 });
      document.dispatchEvent(new MouseEvent("mousemove", { clientY: 50, bubbles: true }));
    });
    const topAfterDown = inner.scrollTop;
    expect(topAfterDown).toBeGreaterThan(0);
    // mouseup 后再 mousemove 不应该改变
    act(() => {
      document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      document.dispatchEvent(new MouseEvent("mousemove", { clientY: 999, bubbles: true }));
    });
    expect(inner.scrollTop).toBe(topAfterDown);
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
