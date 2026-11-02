// LiveBoard 测试：卡片渲染 / 状态徽标 / 深链回调 / 空态
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import LiveBoard from "../LiveBoard";
import type { LiveTasksDto } from "../live";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

const now = Date.now();

function dto(tasks: Partial<LiveTasksDto["tasks"][number]>[], extra: Partial<LiveTasksDto> = {}): LiveTasksDto {
  return {
    tasks: tasks.map((t) => ({
      source: "zcode",
      sessionId: "s1",
      title: "示例任务",
      workspace: "/tmp/proj",
      originator: null,
      startedAtMs: now - 120_000,
      lastActivityMs: now - 5_000,
      state: "running",
      activity: { kind: "tool", role: "assistant", snippet: "▶ cargo test", atMs: now - 5_000 },
      progress: { todosDone: 1, todosTotal: 3, messageCount: 10, totalTokens: 1234 },
      conversationId: null,
      ...t,
    })),
    errors: [],
    scannedAtMs: now,
    ...extra,
  };
}

describe("LiveBoard", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(dto([]));
  });

  it("渲染任务卡片：源徽标 / 标题 / 运行中状态 / 活动摘要", async () => {
    invoke.mockResolvedValue(dto([{ source: "claude-code", title: "修 bug", state: "running" }]));
    render(<LiveBoard onJumpToConversation={vi.fn()} />);
    await waitFor(() => expect(screen.getByText("修 bug")).toBeTruthy());
    expect(screen.getByText("Claude Code")).toBeTruthy();
    expect(screen.getByText("运行中")).toBeTruthy();
    expect(screen.getByText("▶ cargo test")).toBeTruthy();
    expect(screen.getByText("TODO 1/3")).toBeTruthy();
  });

  it("运行中的任务排前 + 深链按钮触发回调", async () => {
    const onJump = vi.fn();
    invoke.mockResolvedValue(dto([
      { sessionId: "idle-1", title: "空闲任务", state: "idle", conversationId: "conv-idle" },
      { sessionId: "run-1", title: "运行任务", state: "running", conversationId: "conv-run" },
    ]));
    render(<LiveBoard onJumpToConversation={onJump} />);
    await waitFor(() => expect(screen.getByText("运行任务")).toBeTruthy());
    // 网格顺序：running 卡片 DOM 序在 idle 前
    const cards = Array.from(document.querySelectorAll("[title='点击查看实时流']")) as HTMLElement[];
    expect(cards.length).toBe(2);
    expect(cards[0].textContent).toContain("运行任务");
    // 打开会话按钮
    fireEvent.click(screen.getAllByText("打开会话")[0]);
    expect(onJump).toHaveBeenCalledWith("conv-run");
  });

  it("空态 + probe 错误提示", async () => {
    invoke.mockResolvedValue(dto([], {
      errors: [{ source: "codex", message: "boom" }],
    }));
    render(<LiveBoard onJumpToConversation={vi.fn()} />);
    await waitFor(() => expect(screen.getByText(/今天还没有活跃的 Agent 会话/)).toBeTruthy());
    expect(screen.getByText(/Codex: boom/)).toBeTruthy();
  });

  it("今日会话排在运行中之后 + 只看运行中过滤生效", async () => {
    invoke.mockResolvedValue(dto([
      { sessionId: "today-1", title: "早上任务", state: "today", conversationId: "conv-t" },
      { sessionId: "run-1", title: "运行任务", state: "running", conversationId: "conv-r" },
    ]));
    const { rerender } = render(<LiveBoard onJumpToConversation={vi.fn()} />);
    await waitFor(() => expect(screen.getByText("早上任务")).toBeTruthy());
    // DOM 序：running 在 today 前
    const cards = Array.from(document.querySelectorAll("[aria-label^='点击查看实时流']")) as HTMLElement[];
    expect(cards.length).toBe(2);
    expect(cards[0].textContent).toContain("运行任务");
    // 「今日」徽标渲染
    expect(screen.getByText("今日")).toBeTruthy();
    // 打开只看运行中 → today 卡片隐藏
    fireEvent.click(screen.getByText("只看运行中"));
    rerender(<LiveBoard onJumpToConversation={vi.fn()} />);
    expect(screen.queryByText("早上任务")).toBeNull();
    expect(screen.getByText("运行任务")).toBeTruthy();
  });

  it("点击卡片打开抽屉并轮询 live_transcript", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "live_tasks") return dto([{ title: "抽屉任务" }]);
      if (cmd === "live_transcript") {
        return [
          { kind: "message", role: "user", snippet: "你好", atMs: now - 60_000 },
          { kind: "message", role: "assistant", snippet: "在干", atMs: now - 30_000 },
        ];
      }
      return {};
    });
    render(<LiveBoard onJumpToConversation={vi.fn()} />);
    await waitFor(() => expect(screen.getByText("抽屉任务")).toBeTruthy());
    fireEvent.click(screen.getByText("抽屉任务"));
    await waitFor(() => expect(screen.getByText("你好")).toBeTruthy());
    expect(screen.getByText("在干")).toBeTruthy();
  });
});
