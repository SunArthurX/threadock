// 会话甘特图：纯布局函数矩阵 + 组件渲染/交互
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import GanttConversations, { buildGanttRows, ganttSpanText, ganttTimeText } from "../GanttConversations";
import type { Conversation } from "../types";

const mk = (o: Partial<Conversation> & Pick<Conversation, "id" | "provider">): Conversation => ({
  source_conversation_id: o.id,
  title: `t-${o.id}`,
  user_title: null,
  status: null,
  model: null,
  completeness_score: null,
  workspace_id: null,
  started_at_ms: null,
  updated_at_ms: null,
  source_parent_id: null,
  child_count: 0,
  ...o,
});

// 本地时区无关的固定时刻（用本地构造器换算 ms）
const T = (y: number, mo: number, d: number, h = 0, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime();
const DAY = 86_400_000;

describe("buildGanttRows（纯布局）", () => {
  const from = T(2026, 8, 15);
  const to = T(2026, 8, 21);

  it("窗口内会话：left/width 按窗口百分比，按开始时间倒序", () => {
    const r = buildGanttRows(
      [
        mk({ id: "a", provider: "zcode", started_at_ms: from + DAY, updated_at_ms: from + DAY + 3_600_000 }),
        mk({ id: "b", provider: "codex", started_at_ms: from + 2 * DAY, updated_at_ms: from + 2 * DAY + 7_200_000 }),
      ],
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
      [mk({ id: "x", provider: "zcode", started_at_ms: from - DAY, updated_at_ms: to + DAY })],
      from,
      to,
    );
    expect(r.rows[0].leftPct).toBe(0);
    expect(r.rows[0].widthPct).toBeCloseTo(100, 5);
  });

  it("零跨度会话保底可见（widthPct ≥ 0.6）", () => {
    const r = buildGanttRows(
      [mk({ id: "z", provider: "zcode", started_at_ms: from + DAY, updated_at_ms: null })],
      from,
      to,
    );
    expect(r.rows[0].widthPct).toBeGreaterThanOrEqual(0.6);
  });

  it("过滤：无 started_at 或整段在窗口外的会话不出现", () => {
    const r = buildGanttRows(
      [
        mk({ id: "n", provider: "zcode" }), // started_at_ms null
        mk({ id: "early", provider: "zcode", started_at_ms: from - 10 * DAY, updated_at_ms: from - 5 * DAY }),
        mk({ id: "ok", provider: "zcode", started_at_ms: from + DAY }),
      ],
      from,
      to,
    );
    expect(r.total).toBe(1);
    expect(r.rows[0].conv.id).toBe("ok");
  });

  it("maxRows 截断但 total 保留全量", () => {
    const convs = Array.from({ length: 10 }, (_, i) =>
      mk({ id: `c${i}`, provider: "zcode", started_at_ms: from + i * 1_000 }),
    );
    const r = buildGanttRows(convs, from, to, { maxRows: 3 });
    expect(r.rows).toHaveLength(3);
    expect(r.total).toBe(10);
  });

  it("updated 早于 started 时钳制为正跨度", () => {
    const r = buildGanttRows(
      [mk({ id: "s", provider: "zcode", started_at_ms: from + DAY, updated_at_ms: from + DAY - 5_000 })],
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

describe("ganttSpanText / ganttTimeText", () => {
  it("跨度人话矩阵", () => {
    expect(ganttSpanText(500)).toBe("≤1 秒");
    expect(ganttSpanText(30_000)).toBe("30 秒");
    expect(ganttSpanText(5 * 60_000)).toBe("5 分钟");
    expect(ganttSpanText(3 * 3_600_000 + 25 * 60_000)).toBe("3 小时 25 分");
    expect(ganttSpanText(2 * DAY + 4 * 3_600_000)).toBe("2 天 4 小时");
  });
  it("时间文本 MM-DD HH:mm（本地构造换算，时区无关）", () => {
    const ms = T(2026, 8, 21, 9, 5);
    expect(ganttTimeText(ms)).toBe("08-21 09:05");
  });
});

describe("GanttConversations（组件）", () => {
  const from = T(2026, 8, 15);
  const to = T(2026, 8, 21);
  const convs: Conversation[] = [
    mk({ id: "g1", provider: "zcode", title: "重构搜索", started_at_ms: from + DAY, updated_at_ms: from + DAY + 2 * 3_600_000 }),
    mk({ id: "g2", provider: "codex", title: "修图标", started_at_ms: from + 3 * DAY, updated_at_ms: from + 3 * DAY + 3_600_000 }),
  ];

  it("渲染行与条，按 Agent 着色，legend 列出来源", () => {
    render(<GanttConversations convs={convs} loading={false} fromMs={from} toMs={to} />);
    expect(screen.getAllByTestId("gantt-row")).toHaveLength(2);
    expect(screen.getAllByTestId("gantt-bar")).toHaveLength(2);
    expect(screen.getByText("ZCode")).toBeTruthy();
    expect(screen.getByText("Codex")).toBeTruthy();
  });

  it("点击行触发 onJumpToConversation", () => {
    const onJump = vi.fn();
    render(<GanttConversations convs={convs} loading={false} fromMs={from} toMs={to} onJumpToConversation={onJump} />);
    fireEvent.click(screen.getAllByTestId("gantt-row")[0]);
    expect(onJump).toHaveBeenCalledWith("g2"); // 倒序：最新在前
  });

  it("悬停条显示浮动详情（标题 + 跨度）", () => {
    render(<GanttConversations convs={convs} loading={false} fromMs={from} toMs={to} />);
    fireEvent.mouseMove(screen.getAllByTestId("gantt-bar")[0]);
    const tip = screen.getByTestId("gantt-tooltip");
    expect(tip.textContent).toContain("修图标");
    expect(tip.textContent).toContain("跨度 1 小时");
  });

  it("空数据显示空态，loading 不渲染行", () => {
    const { rerender } = render(<GanttConversations convs={[]} loading={false} fromMs={from} toMs={to} />);
    expect(screen.getByText("时间范围内没有会话")).toBeTruthy();
    rerender(<GanttConversations convs={null} loading fromMs={from} toMs={to} />);
    expect(screen.queryAllByTestId("gantt-row")).toHaveLength(0);
  });
});
