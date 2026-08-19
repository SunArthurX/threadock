// SearchResultsPanel（按主对话分组）单元测试：
// 分组折叠、命中计数、角色筛选回调、行点击回调、当前会话高亮。
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import SearchResultsPanel from "../SearchResultsPanel";
import type { SearchHitGroup } from "../types";

const g = (over: Partial<SearchHitGroup>): SearchHitGroup => ({
  root_conversation_id: "root-1",
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
  g({ root_conversation_id: "root-2", root_title: "主对话二", conversation_id: "c-2", title: "另一会话", hit_count: 1 }),
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
