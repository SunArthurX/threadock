// 会话列表日期快筛测试
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ConversationList from "../ConversationList";
import type { Conversation } from "../types";

const now = Date.now();
const make = (id: string, startedAgoDays: number | null): Conversation => ({
  id,
  provider: "codex",
  source_conversation_id: id,
  title: `Conv ${id}`,
  user_title: null,
  status: null,
  model: null,
  workspace_id: null,
  started_at_ms: startedAgoDays === null ? null : now - (startedAgoDays ?? 0) * 86_400_000,
  updated_at_ms: now - (startedAgoDays ?? 0) * 86_400_000,
  source_parent_id: null,
  child_count: 0,
  favorite: false,
  archived: false,
  completeness_score: null,
});

describe("ConversationList 日期快筛", () => {
  const props = (convs: Conversation[]) => ({
    conversations: convs,
    selectedConv: null,
    loading: false,
    providerFilter: null,
    selectedWs: null,
    expandedParents: new Set<string>(),
    childConvs: {} as Record<string, Conversation[]>,
    scope: "all" as const,
    onScopeChange: vi.fn(),
    onFilter: vi.fn(),
    onSelect: vi.fn(),
    onToggleExpand: vi.fn(),
    onClearWs: vi.fn(),
    onToggleFavorite: vi.fn(),
  });

  it("默认「全部」显示所有会话", () => {
    const convs = [make("a", 0), make("b", 5), make("c", 100)];
    const { container } = render(<ConversationList {...props(convs)} />);
    expect(container.querySelectorAll(".list-item").length).toBe(3);
    expect(container.querySelector(".panel-header")?.textContent).toContain("(3)");
  });

  it("切到「今日」只显示当天会话", async () => {
    const convs = [make("today", 0), make("yesterday", 1.5), make("old", 100)];
    const { container, getByText } = render(<ConversationList {...props(convs)} />);
    fireEvent.click(getByText("今日"));
    await waitFor(() => {
      expect(container.querySelectorAll(".list-item").length).toBe(1);
      expect(container.querySelector(".list-item")?.textContent).toContain("today");
    });
    // header 显示 filtered/total
    expect(container.querySelector(".panel-header")?.textContent).toContain("1/3");
  });

  it("切到「近 7 天」排除更早的会话", async () => {
    const convs = [make("a", 0), make("b", 3), make("c", 6), make("d", 10)];
    const { container, getByText } = render(<ConversationList {...props(convs)} />);
    fireEvent.click(getByText("近 7 天"));
    await waitFor(() => {
      // 0/3/6 天都在 7 天内，10 天不在
      expect(container.querySelectorAll(".list-item").length).toBe(3);
    });
  });

  it("所有会话都超期时显示「当前日期范围无会话」空态", async () => {
    const convs = [make("a", 100), make("b", 200)];
    const { container, getByText } = render(<ConversationList {...props(convs)} />);
    fireEvent.click(getByText("近 7 天"));
    await waitFor(() => {
      expect(container.querySelector(".empty")?.textContent).toMatch(/当前日期范围无会话/);
    });
  });

  it("切回「全部」恢复显示", async () => {
    const convs = [make("a", 0), make("b", 100)];
    const { container } = render(<ConversationList {...props(convs)} />);
    // 第二个「全部」是 date filter 的 chip
    const allChips = [...container.querySelectorAll(".filter-chip")].filter((b) => b.textContent === "全部");
    expect(allChips.length).toBeGreaterThanOrEqual(1);
    // 点 date filter 的「近 7 天」
    const weekChip = [...container.querySelectorAll(".filter-chip")].find((b) => b.textContent === "近 7 天");
    fireEvent.click(weekChip!);
    await waitFor(() => expect(container.querySelectorAll(".list-item").length).toBe(1));
    // 找日期行的「全部」（第二个）
    const dateAllChip = allChips[allChips.length - 1];
    fireEvent.click(dateAllChip);
    await waitFor(() => expect(container.querySelectorAll(".list-item").length).toBe(2));
  });
});
