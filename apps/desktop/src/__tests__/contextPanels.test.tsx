// 会话摘要 / 上下文窗口面板：后端 DTO 字段缺失时的降级守卫（回归测试）。
//
// 起因：详情页挂载即 invoke 这两个命令。若面板直接读 digest.top_files.length
// 之类字段，而 IPC 返回的是残缺结构（宽松 mock、后端结构变更），会抛
// TypeError 把整个详情页打挂。这里锁定「缺字段 → 降级不崩」的行为。
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConversationDigestPanel from "../ConversationDigestPanel";
import ContextWindowPanel from "../ContextWindowPanel";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
vi.mock("../clipboard", () => ({ copyToClipboard: vi.fn(async () => ({ ok: true })) }));
vi.mock("../toast", () => ({ showToast: vi.fn() }));

/** 每个用例自定命令处理器；未命中的命令一律 resolve null。 */
let handlers: Record<string, (args: unknown) => unknown> = {};

beforeEach(() => {
  handlers = {};
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    const h = handlers[cmd as string];
    return h ? h(args) : null;
  });
});

describe("会话摘要面板（DTO 缺字段降级）", () => {
  it("缺 top_files/milestones/recent_errors → 渲染空摘要，不崩", async () => {
    handlers["conversation_digest"] = () => ({ title: "标题", provider: "zcode" });
    render(<ConversationDigestPanel convId="c1" />);
    await waitFor(() => expect(screen.getByText("会话摘要")).toBeTruthy());
    // 数组缺失 → 该行不渲染，而不是崩在 top_files.length
    expect(screen.queryByText("主要文件")).toBeNull();
    expect(screen.queryByText("里程碑")).toBeNull();
  });

  it("字段齐全 → 渲染主要文件与里程碑", async () => {
    handlers["conversation_digest"] = () => ({
      title: "标题", provider: "zcode", model: null,
      started_at_ms: null, updated_at_ms: null,
      user_messages: 2, assistant_messages: 1, system_messages: 0,
      requests: 3, context_total_tokens: 100, context_window_tokens: 1000,
      topic_opening: "开头诉求", topic_latest: null, closing_reply: null,
      commands_run: 4, files_read: 5, files_created: 1, files_updated: 2, errors: 0,
      top_files: ["a.ts", "b.ts"], recent_errors: [], milestones: ["创建 a.ts"],
      thinking_events: 6, system_block_events: 7,
    });
    render(<ConversationDigestPanel convId="c1" />);
    await waitFor(() => expect(screen.getByText("a.ts、b.ts")).toBeTruthy());
    expect(screen.getByText("创建 a.ts")).toBeTruthy();
  });

  it("命令返回 null → 不渲染面板", async () => {
    render(<ConversationDigestPanel convId="c1" />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("conversation_digest", {
        conversationId: "c1",
      }),
    );
    expect(screen.queryByText("会话摘要")).toBeNull();
  });
});

describe("上下文窗口面板（DTO 缺字段降级）", () => {
  it("空对象 → 不渲染面板，不崩", async () => {
    handlers["conversation_context_usage"] = () => ({});
    render(<ContextWindowPanel convId="c1" />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("conversation_context_usage", {
        conversationId: "c1",
      }),
    );
    expect(screen.queryByText("上下文窗口")).toBeNull();
  });

  it("缺 categories/trend → 仍按总量渲染，不崩", async () => {
    handlers["conversation_context_usage"] = () => ({ total_tokens: 1200 });
    render(<ContextWindowPanel convId="c1" />);
    await waitFor(() => expect(screen.getByText("上下文窗口")).toBeTruthy());
  });

  it("字段齐全 → 渲染总量与分类", async () => {
    handlers["conversation_context_usage"] = () => ({
      total_tokens: 1200, window_tokens: 10000, measured: true,
      turn_count: 4, output_tokens: 300, reasoning_tokens: 100,
      cache_hit_rate: 0.5, cache_read_tokens: 900, cache_write_tokens: 100,
      fresh_tokens: 200, last_turn_at_ms: 1700000000000,
      categories: [
        { key: "messages", tokens: 1000 },
        { key: "thinking", tokens: 200 },
      ],
      trend: [100, 200, 300],
    });
    render(<ContextWindowPanel convId="c1" />);
    await waitFor(() => expect(screen.getByText("上下文窗口")).toBeTruthy());
    expect(screen.getByText("用户与助手消息")).toBeTruthy();
  });
});
