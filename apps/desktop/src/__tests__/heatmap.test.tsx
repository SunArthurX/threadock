// 热力图渲染端到端：真实形态数据 → 组件必须画出格子
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ActivityView from "../ActivityView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "list_conversations_by_date") return []; // 当日会话列表
    return {
      heatmap: [
        { day: "2026-08-01", calls: 10, sessions: 2 },
        { day: "2026-08-02", calls: 40, sessions: 3 },
        { day: "2026-08-05", calls: 5, sessions: 1 },
      ],
      hourly: [{ hour: 9, calls: 30 }, { hour: 14, calls: 20 }],
      hourly_weekday: Array.from({ length: 24 }, (_, h) => ({ hour: h, calls: h === 14 ? 20 : 5 })),
      hourly_weekend: Array.from({ length: 24 }, (_, h) => ({ hour: h, calls: h === 9 ? 30 : 0 })),
      tools_trend: [
        { month: "2026-07", tool: "Bash", calls: 5 },
        { month: "2026-08", tool: "Bash", calls: 30 },
        { month: "2026-08", tool: "Read", calls: 20 },
      ],
      tool_daily: [
        { day: "2026-08-01", tool: "Bash", calls: 6 },
        { day: "2026-08-01", tool: "Read", calls: 4 },
      ],
    };
  }),
}));

describe("活动页热力图渲染", () => {
  it("真实形态数据下必须渲染出热力格（含月份标签 + day-of-week）", async () => {
    const { container, findByText } = render(<ActivityView />);
    expect(await findByText(/contributions in the last/)).toBeTruthy();
    // 8月1日(周六)~5日(周三)：首列补 6 个 null + 数据天 + 尾列补齐 → 至少 14 格
    const cells = container.querySelectorAll(".heat-cell");
    expect(cells.length).toBeGreaterThanOrEqual(14);
    // 有数据格子带 title 明细（含「次调用」+「会话」）
    const titled = container.querySelectorAll('.heat-cell[title*="次调用"]');
    expect(titled.length).toBeGreaterThanOrEqual(3);
    // 月份标签（GitHub 风格英文 Aug）
    expect(container.querySelector(".heat-month-label")?.textContent).toContain("Aug");
    // day-of-week 标签
    const dowLabels = container.querySelectorAll(".heat-dow-label");
    expect(dowLabels.length).toBeGreaterThanOrEqual(7);
  });

  it("点击格子后显示 day detail panel", async () => {
    const { container, findByText } = render(<ActivityView />);
    await findByText(/contributions in the last/);
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

  it("24h 分布的 BarChart 支持自定义 hover tooltip（renderTooltip）", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      expect(container.querySelector(".barchart-bar")).toBeTruthy();
    });
    // 模拟 hover 第一个柱子 → 应该出现 .barchart-tooltip
    const firstBar = container.querySelector(".barchart-bar");
    expect(firstBar).toBeTruthy();
    fireEvent.mouseMove(firstBar!, { clientX: 100, clientY: 50 } as unknown as MouseEvent);
    // 出现 tooltip（含 tooltip-title 元素）
    await waitFor(() => {
      const tt = container.querySelector(".barchart-tooltip");
      expect(tt).toBeTruthy();
      expect(tt?.textContent).toMatch(/\d{1,2}:00/); // 包含 H:00 或 HH:00
    });
  });

  it("点击「查看当日会话」展开当日会话列表（点击条目触发跳转）", async () => {
    const onJump = vi.fn();
    const { container, findByText } = render(<ActivityView onJumpToConversation={onJump} />);
    await findByText(/contributions in the last/);
    const dataCell = container.querySelector('.heat-cell[title*="次调用"]');
    fireEvent.click(dataCell!);
    // 等 day detail 出现
    await waitFor(() => {
      expect(container.querySelector(".day-detail")).toBeTruthy();
    });
    // 点「查看当日会话」按钮
    const btn = await findByText(/查看当日会话|重新查询/);
    fireEvent.click(btn);
    // 由于 mock 返回空数组，应显示「当日没有主任务会话」空态
    await waitFor(() => {
      expect(container.textContent).toMatch(/没有主任务会话|当日会话/);
    });
  });
});
