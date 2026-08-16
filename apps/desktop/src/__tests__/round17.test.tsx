// 第 17 轮测试：HeatmapGitHub hover 自定义 tooltip
// - 初始无 tooltip
// - mouseEnter cell 后显示 tooltip，含日期/调用/会话/强度
// - "今天" tag 仅 hover today cell 时显示
// - mouseLeave 后 tooltip 消失
// - 浮层 position: fixed + transform(12,12) 偏移
import { describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import HeatmapGitHub from "../HeatmapGitHub";

function buildGrid() {
  const cols: { cells: ({ day: string; calls: number; sessions: number } | null)[] }[] = [];
  for (let i = 0; i < 3; i++) {
    const cells: ({ day: string; calls: number; sessions: number } | null)[] = [];
    for (let r = 0; r < 7; r++) {
      const day = `2025-01-${String(i * 7 + r + 1).padStart(2, "0")}`;
      // 5 个 cell 有数据 + 1 个空（让 0 档也走 empty testid 分支）
      if (i === 1 && r === 3) {
        cells.push(null);
      } else {
        cells.push({ day, calls: 100 + r * 10, sessions: r + 1 });
      }
    }
    cols.push({ cells });
  }
  return cols;
}

function renderGrid(opts: { todayKey?: string | null } = {}) {
  return render(
    <HeatmapGitHub
      cols={buildGrid()}
      max={200}
      todayKey={opts.todayKey ?? "2025-01-10"}
    />,
  );
}

describe("HeatmapGitHub 容器 + data-testid", () => {
  it("渲染 heatmap-github 容器", () => {
    const { container } = renderGrid();
    const root = container.querySelector('[data-testid="heatmap-github"]');
    expect(root).toBeTruthy();
  });

  it("7 个 weekday label（Mon-Sun）", () => {
    const { container } = renderGrid();
    const dows = container.querySelectorAll('[data-testid="heatmap-dow"]');
    expect(dows.length).toBe(7);
    expect(dows[0].textContent).toBe("Mon");
    expect(dows[6].textContent).toBe("Sun");
  });

  it("N 个 month label（与 cols 同数）", () => {
    const { container } = renderGrid();
    const months = container.querySelectorAll('[data-testid="heatmap-month"]');
    expect(months.length).toBe(3);
  });

  it("数据 cell + 空 cell 数量正确", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    const empty = container.querySelectorAll('[data-testid="heatmap-cell-empty"]');
    // 3 cols × 7 cells = 21；其中 1 个空 → 20 数据 + 1 空
    expect(cells.length).toBe(20);
    expect(empty.length).toBe(1);
  });
});

describe("hover tooltip 显示/隐藏", () => {
  it("初始无 tooltip", () => {
    const { container } = renderGrid();
    expect(container.querySelector('[data-testid="heatmap-tooltip"]')).toBeNull();
  });

  it("mouseEnter cell 后 tooltip 出现", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 100, clientY: 200 });
    const tip = container.querySelector('[data-testid="heatmap-tooltip"]');
    expect(tip).toBeTruthy();
  });

  it("tooltip 含日期 + 星期中文（周X）", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 100, clientY: 200 });
    const dayLabel = container.querySelector('[data-testid="heatmap-tooltip-day"]');
    expect(dayLabel).toBeTruthy();
    // 2025-01-01 是周三
    expect(dayLabel?.textContent).toContain("2025-01-01");
    expect(dayLabel?.textContent).toContain("周三");
  });

  it("tooltip 含调用数 + 会话数", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 100, clientY: 200 });
    const tip = container.querySelector('[data-testid="heatmap-tooltip"]');
    expect(tip?.textContent).toContain("100");
    expect(tip?.textContent).toContain("次调用");
    expect(tip?.textContent).toContain("活跃会话");
  });

  it("tooltip 含强度 1-4", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 100, clientY: 200 });
    const tip = container.querySelector('[data-testid="heatmap-tooltip"]');
    expect(tip?.textContent).toMatch(/强度 [1-4] \/ 4/);
  });

  it("mouseLeave 后 tooltip 消失", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 100, clientY: 200 });
    expect(container.querySelector('[data-testid="heatmap-tooltip"]')).toBeTruthy();
    fireEvent.mouseLeave(cells[0]);
    expect(container.querySelector('[data-testid="heatmap-tooltip"]')).toBeNull();
  });

  it("切换 cell 时 tooltip 内容切换", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 100, clientY: 200 });
    expect(container.querySelector('[data-testid="heatmap-tooltip-day"]')?.textContent).toContain("2025-01-01");
    fireEvent.mouseLeave(cells[0]);
    fireEvent.mouseEnter(cells[5], { clientX: 100, clientY: 200 });
    // cells[5] = 2025-01-06（row=5 of col=0，call=150）
    expect(container.querySelector('[data-testid="heatmap-tooltip-day"]')?.textContent).toContain("2025-01-06");
    expect(container.querySelector('[data-testid="heatmap-tooltip"]')?.textContent).toContain("150");
  });
});

describe("今天 tag", () => {
  it("hover today cell 时显示「今天」", () => {
    const { container } = renderGrid({ todayKey: "2025-01-10" });
    // 2025-01-10 是 col=1, row=2 (i=1, r=2) → cells[1*7-1+2] = cells[9]? 实际 cols[1] 第 3 个
    // 算下：col 0 (1-7) + col 1 (8-14)，2025-01-10 在 col 1, r=2，所以 cells[1*7+2] = cells[9]
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[9], { clientX: 100, clientY: 200 });
    const today = container.querySelector('[data-testid="heatmap-tooltip-today"]');
    expect(today).toBeTruthy();
    expect(today?.textContent).toBe("今天");
  });

  it("hover 非 today cell 时不显示「今天」", () => {
    const { container } = renderGrid({ todayKey: "2025-01-10" });
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    // cells[0] = 2025-01-01，不是 today
    fireEvent.mouseEnter(cells[0], { clientX: 100, clientY: 200 });
    expect(container.querySelector('[data-testid="heatmap-tooltip-today"]')).toBeNull();
  });
});

describe("tooltip position fixed + 跟随鼠标", () => {
  it("tooltip style.position = fixed", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 123, clientY: 456 });
    const tip = container.querySelector('[data-testid="heatmap-tooltip"]') as HTMLElement;
    expect(tip.style.position).toBe("fixed");
    // mouseMove 更新 x/y
    fireEvent.mouseMove(cells[0], { clientX: 999, clientY: 888 });
    expect(tip.style.left).toBe("999px");
    expect(tip.style.top).toBe("888px");
  });

  it("tooltip 偏移 12px（transform）", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 50, clientY: 50 });
    const tip = container.querySelector('[data-testid="heatmap-tooltip"]') as HTMLElement;
    expect(tip.style.transform).toBe("translate(12px, 12px)");
  });
});

describe("weekdayCN 准确（星期中文）", () => {
  it("2025-01-01 周三", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    fireEvent.mouseEnter(cells[0], { clientX: 50, clientY: 50 });
    expect(container.querySelector('[data-testid="heatmap-tooltip-day"]')?.textContent).toContain("周三");
  });

  it("2025-01-04 周六", () => {
    const { container } = renderGrid();
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"]');
    // cells[3] = 2025-01-04
    fireEvent.mouseEnter(cells[3], { clientX: 50, clientY: 50 });
    expect(container.querySelector('[data-testid="heatmap-tooltip-day"]')?.textContent).toContain("周六");
  });
});
