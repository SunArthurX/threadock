// 热力图渲染端到端：真实形态数据 → 组件必须画出格子
import { fireEvent, render, waitFor } from "@testing-library/react";
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
    tools_trend: [
      { month: "2026-07", tool: "Bash", calls: 5 },
      { month: "2026-08", tool: "Bash", calls: 30 },
      { month: "2026-08", tool: "Read", calls: 20 },
    ],
  })),
}));

describe("活动页热力图渲染", () => {
  it("真实形态数据下必须渲染出热力格（含月份标签 + day-of-week）", async () => {
    const { container, findByText } = render(<ActivityView />);
    expect(await findByText("每日协作热力图")).toBeTruthy();
    // 8月1日(周六)~5日(周三)：首列补 6 个 null + 数据天 + 尾列补齐 → 至少 14 格
    const cells = container.querySelectorAll(".heat-cell");
    expect(cells.length).toBeGreaterThanOrEqual(14);
    // 有数据格子带 title 明细（含「次调用」+「会话」）
    const titled = container.querySelectorAll('.heat-cell[title*="次调用"]');
    expect(titled.length).toBeGreaterThanOrEqual(3);
    // 月份标签
    expect(container.querySelector(".heat-month-label")?.textContent).toContain("8月");
    // day-of-week 标签
    const dowLabels = container.querySelectorAll(".heat-dow-label");
    expect(dowLabels.length).toBeGreaterThanOrEqual(7);
  });

  it("点击格子后显示 day detail panel", async () => {
    const { container, findByText } = render(<ActivityView />);
    await findByText("每日协作热力图");
    // 找有 title 的格子点一下
    const dataCell = container.querySelector('.heat-cell[title*="次调用"]');
    expect(dataCell).toBeTruthy();
    fireEvent.click(dataCell!);
    // day detail 出现
    await waitFor(() => {
      expect(container.querySelector(".day-detail")).toBeTruthy();
    });
    // 详情里有「当月工具分布」
    expect(container.querySelector(".day-detail")?.textContent).toContain("工具调用");
  });

  it("空数据显示空态而非崩溃", async () => {
    vi.mocked(await import("@tauri-apps/api/core")).invoke.mockResolvedValueOnce({
      heatmap: [], hourly: [], tools_trend: [],
    });
    const { findByText } = render(<ActivityView />);
    // 第 6-10 轮优化后空态文案带「活动数据」字样 + 引导
    expect(await findByText(/暂无活动数据/)).toBeTruthy();
  });

  it("Top 10 工具列表带环比 delta", async () => {
    const { container } = render(<ActivityView />);
    // 等数据加载 + 渲染完成（找任意 tool-rank-row）
    await waitFor(() => {
      expect(container.querySelector(".tool-rank-row")).toBeTruthy();
    });
    const rows = container.querySelectorAll(".tool-rank-row");
    expect(rows.length).toBeGreaterThan(0);
    // 至少 1 行带 delta 标签
    const deltas = container.querySelectorAll(".tool-rank-delta");
    expect(deltas.length).toBe(rows.length);
    // Top 3 排名号带高亮 class
    expect(container.querySelectorAll(".tool-rank-num.top").length).toBeGreaterThanOrEqual(1);
  });
});
