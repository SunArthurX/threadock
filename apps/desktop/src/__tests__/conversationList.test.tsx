// 会话列表测试：日期 dropdown + ⌘点击多选 + 批量操作
// 第 11 轮改版：4 行 filter-chip → 1 行 toolbar + 3 dropdown；checkbox 移除 → ⌘点击多选。
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ConversationList from "../ConversationList";
import type { Conversation } from "../types";

const now = Date.now();

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

/** 打开第 N 个 dropdown（第 0=scope, 1=date, 2=sort），返回面板元素（异步等 React 渲染）。 */
async function openDropdown(container: HTMLElement, idx: number) {
  const btns = container.querySelectorAll<HTMLButtonElement>(".list-dropdown-btn");
  fireEvent.click(btns[idx]!);
  // 只有打开的 dropdown 才渲染 panel；用「开放中」筛选
  await waitFor(() => {
    const opens = container.querySelectorAll(".list-dropdown.open");
    expect(opens.length).toBeGreaterThanOrEqual(1);
  });
  // 拿刚打开的那个 panel（按 btn index 配对）
  const panels = container.querySelectorAll(".list-dropdown-panel");
  // panels 顺序 = DOM 顺序，但只有打开的渲染。简单方案：取最后一个
  // 实际上更稳的是：找第 N 个 .list-dropdown 下的 panel
  const dropdownDivs = container.querySelectorAll(".list-dropdown");
  const targetPanel = dropdownDivs[idx]?.querySelector(".list-dropdown-panel") as HTMLElement;
  return targetPanel ?? panels[panels.length - 1] as HTMLElement;
}

/** 在指定 dropdown 面板里点击指定 label 的选项。 */
async function pickDropdownItem(container: HTMLElement, idx: number, label: string) {
  const panel = await openDropdown(container, idx);
  const item = [...panel.querySelectorAll(".list-dropdown-item")].find((b) => b.textContent === label);
  expect(item).toBeTruthy();
  fireEvent.click(item!);
}

describe("ConversationList 日期快筛（dropdown）", () => {
  it("默认「全部」显示所有会话", () => {
    const convs = [make("a", 0), make("b", 5), make("c", 100)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    expect(container.querySelectorAll(".list-item").length).toBe(3);
    expect(container.querySelector(".panel-header")?.textContent).toContain("3");
  });

  it("切到「今日」只显示当天会话", async () => {
    const convs = [make("today", 0), make("yesterday", 1.5), make("old", 100)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    await pickDropdownItem(container, 1, "今日");
    await waitFor(() => {
      expect(container.querySelectorAll(".list-item").length).toBe(1);
      expect(container.querySelector(".list-item")?.textContent).toContain("today");
    });
    expect(container.querySelector(".panel-header")?.textContent).toContain("1 / 3");
  });

  it("切到「近 7 天」排除更早的会话", async () => {
    const convs = [make("a", 0), make("b", 3), make("c", 6), make("d", 10)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    await pickDropdownItem(container, 1, "近 7 天");
    await waitFor(() => {
      expect(container.querySelectorAll(".list-item").length).toBe(3); // 0/3/6 在内，10 在外
    });
  });

  it("所有会话都超期时显示「当前日期范围无会话」空态", async () => {
    const convs = [make("a", 100), make("b", 200)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    await pickDropdownItem(container, 1, "近 7 天");
    await waitFor(() => {
      expect(container.querySelector(".empty-state")?.textContent).toMatch(/当前日期范围无会话/);
    });
  });

  it("切回「全部时间」恢复显示", async () => {
    const convs = [make("a", 0), make("b", 100)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    await pickDropdownItem(container, 1, "近 7 天");
    await waitFor(() => expect(container.querySelectorAll(".list-item").length).toBe(1));
    await pickDropdownItem(container, 1, "全部时间");
    await waitFor(() => expect(container.querySelectorAll(".list-item").length).toBe(2));
  });
});

describe("ConversationList ⌘点击多选 + 批量操作", () => {
  it("⌘点击 2 个 list-item → 出现 bulk-bar 显示「已选 2 条」", async () => {
    const convs = [make("a", 0), make("b", 1), make("c", 2)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    const items = container.querySelectorAll<HTMLDivElement>(".list-item");
    fireEvent.click(items[0], { metaKey: true });
    fireEvent.click(items[1], { metaKey: true });
    await waitFor(() => {
      const bar = container.querySelector(".bulk-bar");
      expect(bar).toBeTruthy();
      expect(bar?.textContent).toContain("已选 2");
    });
  });

  it("点 ★ 收藏按钮触发 onBulkFavorite([ids], true)", async () => {
    const onBulkFav = vi.fn();
    const convs = [make("a", 0), make("b", 1)];
    const { container } = render(<ConversationList {...makeProps(convs)} onBulkFavorite={onBulkFav} />);
    const items = container.querySelectorAll<HTMLDivElement>(".list-item");
    fireEvent.click(items[0], { metaKey: true });
    fireEvent.click(items[1], { metaKey: true });
    await waitFor(() => expect(container.querySelector(".bulk-bar")).toBeTruthy());
    const favBtn = [...container.querySelectorAll(".bulk-btn")].find((b) => b.textContent?.includes("收藏"))!;
    fireEvent.click(favBtn);
    await waitFor(() => expect(onBulkFav).toHaveBeenCalledWith(["a", "b"], true));
  });

  it("全选/清空切换", async () => {
    const convs = [make("a", 0), make("b", 1), make("c", 2)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    const items = container.querySelectorAll<HTMLDivElement>(".list-item");
    fireEvent.click(items[0], { metaKey: true });
    await waitFor(() => expect(container.querySelector(".bulk-bar")?.textContent).toContain("已选 1"));
    // 全选
    fireEvent.click([...container.querySelectorAll(".bulk-btn")].find((b) => b.textContent === "全选")!);
    await waitFor(() => expect(container.querySelector(".bulk-bar")?.textContent).toContain("已选 3"));
    // 清空
    fireEvent.click([...container.querySelectorAll(".bulk-btn")].find((b) => b.textContent === "清空")!);
    await waitFor(() => expect(container.querySelector(".bulk-bar")).toBeNull());
  });

  it("Esc 清空多选", async () => {
    const convs = [make("a", 0), make("b", 1)];
    const { container } = render(<ConversationList {...makeProps(convs)} />);
    const items = container.querySelectorAll<HTMLDivElement>(".list-item");
    fireEvent.click(items[0], { metaKey: true });
    await waitFor(() => expect(container.querySelector(".bulk-bar")?.textContent).toContain("已选 1"));
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(container.querySelector(".bulk-bar")).toBeNull());
  });
});
