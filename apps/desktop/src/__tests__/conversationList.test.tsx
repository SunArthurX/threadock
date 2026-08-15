// 会话列表日期快筛测试
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ConversationList from "../ConversationList";
import type { Conversation } from "../types";

const now = Date.now();

/** 构造单个测试 props，必要时注入批量回调 */
function makeProps(
  convs: Conversation[],
  extra: Partial<{
    onBulkFavorite: (ids: string[], fav: boolean) => Promise<void> | void;
    onBulkArchive: (ids: string[], arch: boolean) => Promise<void> | void;
    onBulkDelete: (ids: string[]) => Promise<void> | void;
  }> = {},
) {
  return {
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
    ...extra,
  };
}

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
  it("默认「全部」显示所有会话", () => {
    const convs = [make("a", 0), make("b", 5), make("c", 100)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    expect(container.querySelectorAll(".list-item").length).toBe(3);
    expect(container.querySelector(".panel-header")?.textContent).toContain("(3)");
  });

  it("切到「今日」只显示当天会话", async () => {
    const convs = [make("today", 0), make("yesterday", 1.5), make("old", 100)];
    const { container, getByText } = render(<ConversationList {...makeProps(convs)} />);
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
    const { container, getByText } = render(<ConversationList {...makeProps(convs)} />);
    fireEvent.click(getByText("近 7 天"));
    await waitFor(() => {
      // 0/3/6 天都在 7 天内，10 天不在
      expect(container.querySelectorAll(".list-item").length).toBe(3);
    });
  });

  it("所有会话都超期时显示「当前日期范围无会话」空态", async () => {
    const convs = [make("a", 100), make("b", 200)];
    const { container, getByText } = render(<ConversationList {...makeProps(convs)} />);
    fireEvent.click(getByText("近 7 天"));
    await waitFor(() => {
      expect(container.querySelector(".empty")?.textContent).toMatch(/当前日期范围无会话/);
    });
  });

  it("切回「全部」恢复显示", async () => {
    const convs = [make("a", 0), make("b", 100)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
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

describe("ConversationList 多选 + 批量操作", () => {
  it("勾选 2 个 checkbox → 出现 bulk-bar 显示「已选 2 条」+ 5 个动作按钮", async () => {
    const convs = [make("a", 0), make("b", 1), make("c", 2)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    const checks = container.querySelectorAll<HTMLInputElement>(".list-item-check");
    expect(checks.length).toBe(3);
    fireEvent.click(checks[0]);
    fireEvent.click(checks[1]);
    await waitFor(() => {
      const bar = container.querySelector(".bulk-bar");
      expect(bar).toBeTruthy();
      expect(bar?.textContent).toContain("已选 2");
    });
    expect(container.querySelectorAll(".bulk-btn").length).toBeGreaterThanOrEqual(5);
  });

  it("点 ★ 收藏触发 onBulkFavorite([ids], true)", async () => {
    const onBulkFav = vi.fn();
    const convs = [make("a", 0), make("b", 1)];
    const { container } = render(<ConversationList {...makeProps(convs)} onBulkFavorite={onBulkFav} />);
    const checks = container.querySelectorAll<HTMLInputElement>(".list-item-check");
    fireEvent.click(checks[0]);
    fireEvent.click(checks[1]);
    await waitFor(() => expect(container.querySelector(".bulk-bar")).toBeTruthy());
    fireEvent.click(container.querySelectorAll(".bulk-btn")[2]); // index 0=全选, 1=清空, 2=收藏
    await waitFor(() => expect(onBulkFav).toHaveBeenCalledWith(["a", "b"], true));
  });

  it("全选/清空切换", async () => {
    const convs = [make("a", 0), make("b", 1), make("c", 2)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    const checks = container.querySelectorAll<HTMLInputElement>(".list-item-check");
    fireEvent.click(checks[0]);
    await waitFor(() => expect(container.querySelector(".bulk-bar")?.textContent).toContain("已选 1"));
    // 全选
    fireEvent.click([...container.querySelectorAll(".bulk-btn")].find((b) => b.textContent === "全选")!);
    await waitFor(() => expect(container.querySelector(".bulk-bar")?.textContent).toContain("已选 3"));
    // 清空
    fireEvent.click([...container.querySelectorAll(".bulk-btn")].find((b) => b.textContent === "清空")!);
    await waitFor(() => expect(container.querySelector(".bulk-bar")).toBeNull());
  });
});
