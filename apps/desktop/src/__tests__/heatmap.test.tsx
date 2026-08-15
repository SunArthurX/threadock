// 热力图渲染端到端：真实形态数据 → 组件必须画出格子
import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ActivityView from "../ActivityView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => ({
    heatmap: [
      { day: "2026-08-01", calls: 10, sessions: 2 },
      { day: "2026-08-02", calls: 40, sessions: 3 },
      { day: "2026-08-05", calls: 5, sessions: 1 },
    ],
    hourly: [{ hour: 9, calls: 30 }, { hour: 14, calls: 20 }],
    tools_trend: [{ month: "2026-08", tool: "Bash", calls: 50 }],
  })),
}));

describe("活动页热力图渲染", () => {
  it("真实形态数据下必须渲染出热力格（含月份标签）", async () => {
    const { container, findByText } = render(<ActivityView />);
    expect(await findByText("每日协作热力图")).toBeTruthy();
    // 8月1日(周六)~5日(周三)：首列补 6 个 null + 数据天 + 尾列补齐 → 至少 14 格
    const cells = container.querySelectorAll(".heat-cell");
    expect(cells.length).toBeGreaterThanOrEqual(14);
    // 有数据格子带 title 明细
    const titled = container.querySelectorAll('.heat-cell[title*="次调用"]');
    expect(titled.length).toBeGreaterThanOrEqual(3);
    // 月份标签
    expect(container.querySelector(".heat-month-label")?.textContent).toContain("8月");
  });

  it("空数据显示空态而非崩溃", async () => {
    vi.mocked(await import("@tauri-apps/api/core")).invoke.mockResolvedValueOnce({
      heatmap: [], hourly: [], tools_trend: [],
    });
    const { findByText } = render(<ActivityView />);
    // 第 6-10 轮优化后空态文案带「活动数据」字样 + 引导
    expect(await findByText(/暂无活动数据/)).toBeTruthy();
  });
});
