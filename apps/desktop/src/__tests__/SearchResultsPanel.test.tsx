// SearchResultsPanel（按主对话分组）单元测试：
// 分组折叠、命中计数、角色筛选回调、行点击回调、当前会话高亮、
// 右键菜单（与普通左栏共用 ConvMenu，行为一致）。
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import SearchResultsPanel from "../SearchResultsPanel";
import { buildConvMenuItems } from "../ConvMenu";
import type { Conversation, SearchHitGroup } from "../types";

const g = (over: Partial<SearchHitGroup>): SearchHitGroup => ({
  root_conversation_id: "c-1",
  root_title: "主对话一",
  root_updated_at_ms: 1,
  provider: "zcode",
  conversation_id: "c-1",
  title: "行标题",
  is_child: false,
  hit_count: 1,
  best_message_id: "m-1",
  best_role: "user",
  snippet: "片段",
  ...over,
});

const groups = [
  g({ conversation_id: "c-child", title: "子任务甲", is_child: true, hit_count: 3, snippet: "子<b>白板</b>" }),
  g({ conversation_id: "c-1", title: "主对话正文", hit_count: 2, snippet: "主<b>白板</b>" }),
  g({ root_conversation_id: "c-2", root_title: "主对话二", conversation_id: "c-2", title: "另一会话", hit_count: 1 }),
];

describe("SearchResultsPanel", () => {
  it("按主对话分组渲染：两个 root，主对话自身行排在子对话之前，计数为合计", () => {
    const { container } = render(<SearchResultsPanel groups={groups} query="白板" role="" onRoleChange={vi.fn()} onOpen={vi.fn()} />);
    const roots = container.querySelectorAll(".search-group-root");
    expect(roots.length).toBe(2);
    expect(roots[0].textContent).toContain("主对话一");
    expect(roots[0].textContent).toContain("5 处"); // 2 + 3
    expect(roots[1].textContent).toContain("1 处");
    // 第一个分组内：主对话行在前、子对话行在后
    const rows = container.querySelectorAll(".search-group")[0].querySelectorAll(".search-result");
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain("主对话");
    expect(rows[1].textContent).toContain("子对话");
    expect(rows[1].textContent).toContain("3 处");
  });

  it("点击 root 头打开该组第一行；点击行回调对应分组", () => {
    const onOpen = vi.fn();
    const { container } = render(<SearchResultsPanel groups={groups} query="白板" role="" onRoleChange={vi.fn()} onOpen={onOpen} />);
    fireEvent.click(container.querySelector(".search-group-root")!);
    // 主对话自身行（排序后在首位）
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: "c-1" }));
    const rows = container.querySelectorAll(".search-result");
    fireEvent.click(rows[1]);
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: "c-child", is_child: true }));
  });

  it("角色筛选变更触发回调；当前会话行高亮", () => {
    const onRoleChange = vi.fn();
    const { container } = render(
      <SearchResultsPanel groups={groups} query="白板" role="user" onRoleChange={onRoleChange} onOpen={vi.fn()} activeConversationId="c-child" />,
    );
    fireEvent.change(container.querySelector<HTMLSelectElement>(".search-panel-select")!, { target: { value: "assistant" } });
    expect(onRoleChange).toHaveBeenCalledWith("assistant");
    const active = container.querySelector(".search-result.active");
    expect(active?.textContent).toContain("子任务甲");
  });

  it("空结果显示无匹配", () => {
    const { container } = render(<SearchResultsPanel groups={[]} query="不存在" role="" onRoleChange={vi.fn()} onOpen={vi.fn()} />);
    expect(container.textContent).toContain("无匹配");
    expect(container.textContent).toContain("命中 0 个会话");
  });
});

// ── 右键菜单：与普通左栏（ConversationList）共用 ConvMenu，行为一致 ──
describe("SearchResultsPanel 右键菜单", () => {
  const conv = (id: string, over: Partial<Conversation> = {}): Conversation => ({
    id,
    provider: "zcode",
    source_conversation_id: `src-${id}`,
    title: `标题-${id}`,
    user_title: null,
    status: "active",
    model: null,
    completeness_score: null,
    workspace_id: null,
    started_at_ms: 1,
    updated_at_ms: 2,
    source_parent_id: null,
    child_count: 0,
    ...over,
  });
  const convs = [conv("c-1"), conv("c-child"), conv("c-2")];

  const renderPanel = (over: Partial<React.ComponentProps<typeof SearchResultsPanel>> = {}) =>
    render(
      <SearchResultsPanel
        groups={groups} query="白板" role="" onRoleChange={vi.fn()} onOpen={vi.fn()}
        conversations={convs} {...over}
      />,
    );

  const openMenuOnRow = (container: HTMLElement, rowIdx: number) => {
    const rows = container.querySelectorAll(".search-result");
    fireEvent.contextMenu(rows[rowIdx]);
    return container.querySelector<HTMLElement>('[data-testid="contextmenu"]');
  };

  it("右键主对话行：弹出与普通列表一致的菜单项（收藏/归档/置顶/加标签/复制标题/删除）", () => {
    const { container } = renderPanel();
    const menu = openMenuOnRow(container, 0); // 主对话行（c-1）
    expect(menu).not.toBeNull();
    const labels = [...menu!.querySelectorAll(".contextmenu-label")].map((x) => x.textContent);
    for (const expected of ["收藏", "归档", "置顶（排在最前）", "加标签…", "复制标题", "删除（带撤销）"]) {
      expect(labels.some((l) => l?.includes(expected))).toBe(true);
    }
  });

  it("菜单动作走批量 handler：删除传命中行 id；收藏传 favorite=true", async () => {
    const onBulkDelete = vi.fn();
    const onBulkFavorite = vi.fn();
    const { container } = renderPanel({ onBulkDelete, onBulkFavorite });
    const rows = container.querySelectorAll(".search-result");
    fireEvent.contextMenu(rows[0]); // 主对话行（c-1）
    const menu = container.querySelector<HTMLElement>('[data-testid="contextmenu"]')!;
    const item = (label: string) =>
      [...menu.querySelectorAll(".contextmenu-item")].find((x) => x.textContent?.includes(label)) as HTMLElement;
    fireEvent.click(item("删除"));
    await waitFor(() => expect(onBulkDelete).toHaveBeenCalledWith(["c-1"]));
    // 重新打开菜单做收藏
    fireEvent.contextMenu(rows[0]);
    const menu2 = container.querySelector<HTMLElement>('[data-testid="contextmenu"]')!;
    const fav2 = [...menu2.querySelectorAll(".contextmenu-item")].find((x) => x.textContent?.includes("收藏")) as HTMLElement;
    fireEvent.click(fav2);
    await waitFor(() => expect(onBulkFavorite).toHaveBeenCalledWith(["c-1"], true));
  });

  it("子对话行与分组 root 头同样可右键（按 conversation_id 定位对象）", () => {
    const onBulkDelete = vi.fn();
    const { container } = renderPanel({ onBulkDelete });
    const rows = container.querySelectorAll(".search-result");
    fireEvent.contextMenu(rows[1]); // 子对话行（c-child）
    let menu = container.querySelector<HTMLElement>('[data-testid="contextmenu"]')!;
    fireEvent.click([...menu.querySelectorAll(".contextmenu-item")].find((x) => x.textContent?.includes("删除")) as HTMLElement);
    expect(onBulkDelete).toHaveBeenCalledWith(["c-child"]);
    // root 头（root-1 对应主对话 c-1）
    fireEvent.contextMenu(container.querySelector(".search-group-root")!);
    menu = container.querySelector<HTMLElement>('[data-testid="contextmenu"]')!;
    expect(menu.textContent).toContain("收藏");
  });

  it("加标签：右键 → 内联输入 → Enter 提交批量标签", async () => {
    const onBulkAddTag = vi.fn();
    const { container } = renderPanel({ onBulkAddTag });
    const rows = container.querySelectorAll(".search-result");
    fireEvent.contextMenu(rows[0]);
    const menu = container.querySelector<HTMLElement>('[data-testid="contextmenu"]')!;
    fireEvent.click([...menu.querySelectorAll(".contextmenu-item")].find((x) => x.textContent?.includes("加标签")) as HTMLElement);
    const input = container.querySelector<HTMLInputElement>(".bulk-tag-input")!;
    expect(input).not.toBeNull();
    fireEvent.change(input, { target: { value: "#重点" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(onBulkAddTag).toHaveBeenCalledWith(["c-1"], "重点"));
  });

  it("会话不在列表数据中（如已删除未刷新）时右键不弹菜单", () => {
    const { container } = render(
      <SearchResultsPanel groups={groups} query="白板" role="" onRoleChange={vi.fn()} onOpen={vi.fn()} conversations={[]} />,
    );
    fireEvent.contextMenu(container.querySelector(".search-result")!);
    expect(container.querySelector('[data-testid="contextmenu"]')).toBeNull();
  });

  it("菜单项与普通列表同源：buildConvMenuItems（scope=all 单选）产出同六个动作", () => {
    const items = buildConvMenuItems({
      conv: conv("c-1"),
      x: 0, y: 0,
      scope: "all",
      pinned: new Set(),
      onTogglePin: vi.fn(),
      selectedIds: new Set(),
      conversations: convs,
      openTagInput: vi.fn(),
    });
    expect(items.map((i) => i.label)).toEqual([
      "收藏", "归档", "置顶（排在最前）", "加标签…", "复制标题", "删除（带撤销）",
    ]);
  });
});
