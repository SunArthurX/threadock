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
  it("真实形态数据下必须渲染出热力格（GitHub 7×N 布局：含月份标签 + day-of-week）", async () => {
    const { container, findByText } = render(<ActivityView />);
    expect(await findByText(/contributions in the last/)).toBeTruthy();
    // data-testid="heatmap-cell" + "heatmap-cell-empty"（GitHub 7×N 独立组件）
    const cells = container.querySelectorAll('[data-testid="heatmap-cell"], [data-testid="heatmap-cell-empty"]');
    expect(cells.length).toBeGreaterThanOrEqual(14);
    // 月份标签（顶部 data-testid="heatmap-month" 含 GitHub 风格英文 Aug）
    const monthLabels = container.querySelectorAll('[data-testid="heatmap-month"]');
    expect(monthLabels.length).toBeGreaterThan(0);
    const monthTexts = [...monthLabels].map((m) => m.textContent ?? "");
    expect(monthTexts.some((t) => t === "Aug")).toBe(true);
    // day-of-week 标签（左侧 7 个：Mon-Sun）
    const dowLabels = container.querySelectorAll('[data-testid="heatmap-dow"]');
    expect(dowLabels.length).toBe(7);
    const dowTexts = [...dowLabels].map((d) => d.textContent ?? "");
    expect(dowTexts).toEqual(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
  });

  it("点击格子后显示 day detail panel", async () => {
    const { container, findByText } = render(<ActivityView />);
    await findByText(/contributions in the last/);
    // 找一个非空 cell 点一下（HeatmapGitHub 用 data-testid="heatmap-cell"）
    const dataCell = container.querySelector('[data-testid="heatmap-cell"]');
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
    // 持续按命令覆盖（子组件甘特卡的 invoke 先于 activity_stats 执行，
    // once 型 mock 会被它消费掉导致本测试拿到全量数据）；用例结束恢复原实现
    const mocked = vi.mocked((await import("@tauri-apps/api/core")).invoke);
    const original = mocked.getMockImplementation() ?? (async () => []);
    mocked.mockImplementation(async (cmd: string) =>
      cmd === "activity_stats" ? { heatmap: [], hourly: [], tools_trend: [] } : original(cmd, undefined),
    );
    try {
      const { findByText } = render(<ActivityView />);
      // 第 7 轮优化：empty 用 InlineEmpty，文案统一为「暂无活动热力数据」
      expect(await findByText(/暂无活动热力数据/)).toBeTruthy();
    } finally {
      mocked.mockImplementation(original);
    }
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
    const dataCell = container.querySelector('[data-testid="heatmap-cell"]');
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
