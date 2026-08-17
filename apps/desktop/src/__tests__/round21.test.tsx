// 第 21 轮测试：概览图表 hover 增强
// - BarChart 不传 renderTooltip 也有默认 tooltip（label + value + 占比%）
// - DonutChart 切到 React hover tooltip（不再用 SVG <title>）
// - ops-tool-row 加 data-tooltip 属性 + CSS 伪元素显示
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { fireEvent, render, act } from "@testing-library/react";
import { BarChart, DonutChart } from "../charts";

describe("BarChart 默认 tooltip（不传 renderTooltip 也有）", () => {
  beforeEach(() => {
    // 让 inner div 有 clientHeight（jsdom 默认 0）
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      get() { return 100; },
    });
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get() { return 400; },
    });
  });
  afterEach(() => {
    delete (HTMLElement.prototype as unknown as { clientHeight?: number }).clientHeight;
    delete (HTMLElement.prototype as unknown as { clientWidth?: number }).clientWidth;
  });

  it("hover bar 出现默认 tooltip，含 label + value + 占比%", () => {
    const data = [
      { label: "2025-01-01", value: 100 },
      { label: "2025-01-02", value: 200 },
      { label: "2025-01-03", value: 400 },
    ];
    const { container } = render(<BarChart data={data} height={100} />);
    // 设置 scrollHeight 让 thumb 算出 max
    const inner = container.querySelector(".barchart") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 400, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    act(() => { inner.dispatchEvent(new Event("scroll")); });

    const bars = container.querySelectorAll(".barchart-bar");
    expect(bars.length).toBe(3);
    // hover 第 2 根（value=200，max=400，占比 50%）
    fireEvent.mouseMove(bars[1] as HTMLElement, { clientX: 200, clientY: 50 });
    const tooltip = container.querySelector(".barchart-tooltip") as HTMLElement;
    expect(tooltip).toBeTruthy();
    expect(tooltip.textContent).toContain("2025-01-02");
    // value 用 formatTokens 渲染：200 → "200"
    expect(tooltip.textContent).toContain("200");
    // 占比：200/400 = 50%
    expect(tooltip.textContent).toContain("50.0%");
  });

  it("传了 renderTooltip 用自定义，忽略默认", () => {
    const data = [{ label: "2025-01-01", value: 100 }];
    const { container } = render(
      <BarChart
        data={data}
        height={100}
        renderTooltip={(d) => <div>custom-{d.value}</div>}
      />,
    );
    const inner = container.querySelector(".barchart") as HTMLElement;
    Object.defineProperty(inner, "scrollHeight", { value: 200, configurable: true });
    Object.defineProperty(inner.parentElement!, "clientHeight", { value: 100, configurable: true });
    act(() => { inner.dispatchEvent(new Event("scroll")); });
    const bar = container.querySelector(".barchart-bar") as HTMLElement;
    fireEvent.mouseMove(bar, { clientX: 100, clientY: 50 });
    const tooltip = container.querySelector(".barchart-tooltip") as HTMLElement;
    expect(tooltip.textContent).toContain("custom-100");
    expect(tooltip.textContent).not.toContain("50.0%");
  });
});

describe("DonutChart React hover tooltip", () => {
  it("hover 扇区出现自定义 tooltip，移除 SVG <title>", () => {
    const slices = [
      { label: "Codex", value: 700, color: "#2da44e" },
      { label: "ZCode", value: 300, color: "#4da3ff" },
    ];
    const { container } = render(<DonutChart slices={slices} size={160} />);
    // 不再用 SVG <title>
    const titles = container.querySelectorAll("title");
    expect(titles.length).toBe(0);
    // 初始无 tooltip
    expect(container.querySelector(".barchart-tooltip")).toBeNull();
  });

  it("mouseEnter 扇区显示 tooltip（label + value + 占比）", () => {
    const slices = [
      { label: "Codex", value: 700, color: "#2da44e" },
      { label: "ZCode", value: 300, color: "#4da3ff" },
    ];
    const { container } = render(<DonutChart slices={slices} size={160} />);
    const circles = container.querySelectorAll("circle");
    expect(circles.length).toBe(2);
    fireEvent.mouseEnter(circles[0] as unknown as HTMLElement, { clientX: 80, clientY: 80 });
    const tooltip = container.querySelector(".barchart-tooltip") as HTMLElement;
    expect(tooltip).toBeTruthy();
    expect(tooltip.textContent).toContain("Codex");
    // 700 → "700"
    expect(tooltip.textContent).toContain("700");
    // 占比：700/1000 = 70%
    expect(tooltip.textContent).toContain("70.0%");
  });

  it("mouseLeave tooltip 消失", () => {
    const slices = [
      { label: "Codex", value: 700, color: "#2da44e" },
    ];
    const { container } = render(<DonutChart slices={slices} size={160} />);
    const circle = container.querySelector("circle") as unknown as HTMLElement;
    fireEvent.mouseEnter(circle, { clientX: 80, clientY: 80 });
    expect(container.querySelector(".barchart-tooltip")).toBeTruthy();
    fireEvent.mouseLeave(circle);
    expect(container.querySelector(".barchart-tooltip")).toBeNull();
  });
});
