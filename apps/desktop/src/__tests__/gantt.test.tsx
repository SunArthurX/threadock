// 会话甘特图：纯布局函数矩阵 + 组件渲染/交互（自带时间范围选择，默认 30 天）
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import GanttConversations, { buildGanttRows, ganttSpanText, ganttTimeText } from "../GanttConversations";
import type { Conversation } from "../types";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  // 默认：30 天窗口内有 2 条会话（时间相对 now 生成，避免写死日期）
  invoke: vi.fn(async (cmd: string): Promise<unknown> => {
    if (cmd !== "list_conversations_by_date") return [];
    const now = Date.now();
    const mk = (id: string, provider: string, title: string, startedAgoMs: number, durMs: number): Conversation => ({
      id, provider, source_conversation_id: id, title, user_title: null, status: null,
      model: null, completeness_score: null, workspace_id: null,
      started_at_ms: now - startedAgoMs,
      updated_at_ms: now - startedAgoMs + durMs,
      source_parent_id: null, child_count: 0,
    });
    return [
      mk("g1", "zcode", "重构搜索", 5 * 86_400_000, 2 * 3_600_000),
      mk("g2", "codex", "修图标", 12 * 86_400_000, 3_600_000),
    ];
  }),
}));

// 本地时区无关的固定时刻（用本地构造器换算 ms）
const T = (y: number, mo: number, d: number, h = 0, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime();
const DAY = 86_400_000;

describe("buildGanttRows（纯布局）", () => {
  const from = T(2026, 8, 15);
  const to = T(2026, 8, 21);

  it("窗口内会话：left/width 按窗口百分比，按开始时间倒序", () => {
    const r = buildGanttRows(
      [
        { id: "a", started_at_ms: from + DAY, updated_at_ms: from + DAY + 3_600_000 },
        { id: "b", started_at_ms: from + 2 * DAY, updated_at_ms: from + 2 * DAY + 7_200_000 },
      ].map((o) => ({ ...baseConv, ...o })),
      from,
      to,
    );
    expect(r.total).toBe(2);
    expect(r.rows[0].conv.id).toBe("b"); // 晚开始的在前
    expect(r.rows[1].leftPct).toBeCloseTo((1 * DAY / (6 * DAY)) * 100, 5);
    expect(r.rows[1].widthPct).toBeCloseTo((3_600_000 / (6 * DAY)) * 100, 5);
  });

  it("跨窗口边界裁剪：起点在窗前 → left=0；终点在窗后 → 铺到 100%", () => {
    const r = buildGanttRows(
      [{ ...baseConv, id: "x", started_at_ms: from - DAY, updated_at_ms: to + DAY }],
      from,
      to,
    );
    expect(r.rows[0].leftPct).toBe(0);
    expect(r.rows[0].widthPct).toBeCloseTo(100, 5);
  });

  it("零跨度会话保底可见（widthPct ≥ 0.6）", () => {
    const r = buildGanttRows([{ ...baseConv, id: "z", started_at_ms: from + DAY, updated_at_ms: null }], from, to);
    expect(r.rows[0].widthPct).toBeGreaterThanOrEqual(0.6);
  });

  it("过滤：无 started_at 或整段在窗口外的会话不出现", () => {
    const r = buildGanttRows(
      [
        { ...baseConv, id: "n", started_at_ms: null }, // 无时间
        { ...baseConv, id: "early", started_at_ms: from - 10 * DAY, updated_at_ms: from - 5 * DAY },
        { ...baseConv, id: "ok", started_at_ms: from + DAY },
      ],
      from,
      to,
    );
    expect(r.total).toBe(1);
    expect(r.rows[0].conv.id).toBe("ok");
  });

  it("maxRows 截断但 total 保留全量", () => {
    const convs = Array.from({ length: 10 }, (_, i) => ({ ...baseConv, id: `c${i}`, started_at_ms: from + i * 1_000 }));
    const r = buildGanttRows(convs, from, to, { maxRows: 3 });
    expect(r.rows).toHaveLength(3);
    expect(r.total).toBe(10);
  });

  it("updated 早于 started 时钳制为正跨度", () => {
    const r = buildGanttRows(
      [{ ...baseConv, id: "s", started_at_ms: from + DAY, updated_at_ms: from + DAY - 5_000 }],
      from,
      to,
    );
    expect(r.rows[0].widthPct).toBeGreaterThanOrEqual(0.6);
  });

  it("轴刻度：5 个（首尾含空标签收尾）", () => {
    const r = buildGanttRows([], from, to);
    expect(r.ticks).toHaveLength(5);
    expect(r.ticks[0].pct).toBe(0);
    expect(r.ticks[4].pct).toBe(100);
  });
});

const baseConv: Conversation = {
  id: "base", provider: "zcode", source_conversation_id: "base", title: "t", user_title: null,
  status: null, model: null, completeness_score: null, workspace_id: null,
  started_at_ms: null, updated_at_ms: null, source_parent_id: null, child_count: 0,
};

describe("ganttSpanText / ganttTimeText", () => {
  it("跨度人话矩阵", () => {
    expect(ganttSpanText(500)).toBe("≤1 秒");
    expect(ganttSpanText(30_000)).toBe("30 秒");
    expect(ganttSpanText(5 * 60_000)).toBe("5 分钟");
    expect(ganttSpanText(3 * 3_600_000 + 25 * 60_000)).toBe("3 小时 25 分");
    expect(ganttSpanText(2 * DAY + 4 * 3_600_000)).toBe("2 天 4 小时");
  });
  it("时间文本 MM-DD HH:mm（本地构造换算，时区无关）", () => {
    expect(ganttTimeText(T(2026, 8, 21, 9, 5))).toBe("08-21 09:05");
  });
});

describe("GanttConversations（组件，自带范围选择）", () => {
  it("默认近 30 天：chip 激活，异步加载后渲染行/条/legend", async () => {
    render(<GanttConversations />);
    expect(screen.getByTestId("gantt-range-30").className).toContain("active");
    expect(await screen.findAllByTestId("gantt-row")).toHaveLength(2);
    expect(screen.getAllByTestId("gantt-bar")).toHaveLength(2);
    expect(screen.getByText("ZCode")).toBeTruthy();
    expect(screen.getByText("Codex")).toBeTruthy();
  });

  it("点击行触发 onJumpToConversation（最新在前）", async () => {
    const onJump = vi.fn();
    render(<GanttConversations onJumpToConversation={onJump} />);
    fireEvent.click((await screen.findAllByTestId("gantt-row"))[0]);
    expect(onJump).toHaveBeenCalledWith("g1"); // 5 天前 > 12 天前，倒序在前
  });

  it("悬停条显示浮动详情（标题 + 跨度）", async () => {
    render(<GanttConversations />);
    fireEvent.mouseMove((await screen.findAllByTestId("gantt-bar"))[0]);
    const tip = screen.getByTestId("gantt-tooltip");
    expect(tip.textContent).toContain("重构搜索");
    expect(tip.textContent).toContain("跨度 2 小时");
  });

  it("切换时间范围：以新窗口重新调用 invoke", async () => {
    render(<GanttConversations />);
    await screen.findAllByTestId("gantt-row");
    fireEvent.click(screen.getByTestId("gantt-range-7"));
    await waitFor(() => {
      const calls = vi.mocked(invoke).mock.calls;
      const last = calls[calls.length - 1];
      expect(last?.[0]).toBe("list_conversations_by_date");
      const args = last?.[1] as { fromMs: number; toMs: number };
      const now = Date.now();
      expect(Math.abs(args.fromMs - (now - 7 * DAY))).toBeLessThan(5_000);
      expect(Math.abs(args.toMs - now)).toBeLessThan(5_000);
    });
    expect(screen.getByTestId("gantt-range-7").className).toContain("active");
    expect(screen.getByTestId("gantt-range-30").className).not.toContain("active");
  });

  it("空数据显示空态", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    render(<GanttConversations />);
    await waitFor(() => expect(screen.getByText(/近 30 天没有会话/)).toBeTruthy());
  });
});
