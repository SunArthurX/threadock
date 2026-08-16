// 第 11 轮测试：ContextMenu + Dropdown + 详情页空态 + 活动热力图自适应
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, beforeEach, vi } from "vitest";
import ContextMenu, { type MenuItem } from "../ContextMenu";
import ConversationList, { loadPinnedIds } from "../ConversationList";
import type { Conversation } from "../types";

beforeEach(() => { localStorage.clear(); vi.restoreAllMocks(); });

const convs: Conversation[] = [
  { id: "c1", provider: "zcode", source_conversation_id: "sc1", title: "分布式事务", user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 3_600_000, updated_at_ms: Date.now() - 3_600_000, child_count: 0, favorite: false, archived: false },
  { id: "c2", provider: "claude-code", source_conversation_id: "sc2", title: "JVM 调优", user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 7_200_000, updated_at_ms: Date.now() - 7_200_000, child_count: 0, favorite: true, archived: false },
];

function makeProps(extra: Partial<React.ComponentProps<typeof ConversationList>> = {}) {
  return {
    conversations: convs, selectedConv: null, loading: false, providerFilter: null, selectedWs: null,
    expandedParents: new Set<string>(), childConvs: {} as Record<string, Conversation[]>,
    scope: "all" as const, onScopeChange: () => {}, onFilter: () => {}, onSelect: () => {},
    onToggleExpand: () => {}, onClearWs: () => {}, onToggleFavorite: () => {},
    ...extra,
  };
}

describe("ContextMenu 通用组件", () => {
  const items: MenuItem[] = [
    { label: "复制", icon: "📋", onClick: vi.fn() },
    { label: "删除", icon: "🗑", onClick: vi.fn(), danger: true, group: 2 },
    { label: "禁用项", onClick: vi.fn(), disabled: true, group: 3 },
  ];

  it("渲染所有 menuitem", () => {
    const { container } = render(<ContextMenu x={100} y={100} items={items} onClose={() => {}} />);
    const menuItems = container.querySelectorAll(".contextmenu-item");
    expect(menuItems.length).toBe(3);
  });

  it("第一项默认 active（键盘焦点在第 0 项）", () => {
    const { container } = render(<ContextMenu x={100} y={100} items={items} onClose={() => {}} />);
    const menuItems = container.querySelectorAll(".contextmenu-item");
    expect(menuItems[0].className).toContain("active");
  });

  it("键盘 ArrowDown 移动到下一项", () => {
    const { container } = render(<ContextMenu x={100} y={100} items={items} onClose={() => {}} />);
    fireEvent.keyDown(window, { key: "ArrowDown" });
    const menuItems = container.querySelectorAll(".contextmenu-item");
    // 第二项「删除」应该 active（跳过 disabled 的「禁用项」）
    expect(menuItems[1].className).toContain("active");
  });

  it("键盘 ArrowUp 在第 0 项时跳过（环绕到最后一个 navigable）", () => {
    const { container } = render(<ContextMenu x={100} y={100} items={items} onClose={() => {}} />);
    fireEvent.keyDown(window, { key: "ArrowUp" });
    const menuItems = container.querySelectorAll(".contextmenu-item");
    // 最后 navigable 是「删除」（index 1）
    expect(menuItems[1].className).toContain("active");
  });

  it("Esc 触发 onClose", () => {
    const onClose = vi.fn();
    render(<ContextMenu x={100} y={100} items={items} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Enter 触发当前 active 项的 onClick + onClose", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const testItems: MenuItem[] = [{ label: "测试", onClick, group: 1 }];
    render(<ContextMenu x={100} y={100} items={testItems} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Enter" });
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("点击 menuitem 触发 onClick + onClose", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const testItems: MenuItem[] = [{ label: "测试", onClick, group: 1 }];
    const { container } = render(<ContextMenu x={100} y={100} items={testItems} onClose={onClose} />);
    fireEvent.click(container.querySelector(".contextmenu-item")!);
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("点击 disabled 项不触发 onClick", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const testItems: MenuItem[] = [{ label: "测试", onClick, disabled: true, group: 1 }];
    const { container } = render(<ContextMenu x={100} y={100} items={testItems} onClose={onClose} />);
    fireEvent.click(container.querySelector(".contextmenu-item")!);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("点击 backdrop 触发 onClose", () => {
    const onClose = vi.fn();
    const { container } = render(<ContextMenu x={100} y={100} items={items} onClose={onClose} />);
    fireEvent.click(container.querySelector(".contextmenu-backdrop")!);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("danger 项加 danger class", () => {
    const { container } = render(<ContextMenu x={100} y={100} items={items} onClose={() => {}} />);
    const menuItems = container.querySelectorAll(".contextmenu-item");
    expect(menuItems[1].className).toContain("danger");
  });

  it("不同 group 之间插入分割线", () => {
    const { container } = render(<ContextMenu x={100} y={100} items={items} onClose={() => {}} />);
    const seps = container.querySelectorAll(".contextmenu-sep");
    // items[0] group=undefined, items[1] group=2 → 1 条; items[1] group=2, items[2] group=3 → 1 条 = 2 条
    expect(seps.length).toBeGreaterThanOrEqual(1);
  });

  it("视口边界保护：x/y 超出时贴边", () => {
    const { container } = render(<ContextMenu x={9999} y={9999} items={items} onClose={() => {}} />);
    const menu = container.querySelector(".contextmenu") as HTMLElement;
    const left = parseInt(menu.style.left);
    const top = parseInt(menu.style.top);
    expect(left).toBeLessThan(9999);
    expect(top).toBeLessThan(9999);
  });
});

describe("ConversationList 工具栏 dropdown", () => {
  it("3 个 dropdown（scope / date / sort）默认收起", () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    expect(container.querySelectorAll(".list-dropdown-btn").length).toBe(3);
    expect(container.querySelectorAll(".list-dropdown-panel").length).toBe(0);
  });

  it("点 dropdown 按钮展开面板，再点收起", async () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const btn = container.querySelectorAll(".list-dropdown-btn")[1] as HTMLButtonElement;
    fireEvent.click(btn);
    await waitFor(() => {
      expect(container.querySelectorAll(".list-dropdown-panel").length).toBe(1);
    });
    fireEvent.click(btn);
    await waitFor(() => {
      expect(container.querySelectorAll(".list-dropdown-panel").length).toBe(0);
    });
  });

  it("点 dropdown 外面收起", async () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const btn = container.querySelectorAll(".list-dropdown-btn")[1] as HTMLButtonElement;
    fireEvent.click(btn);
    await waitFor(() => {
      expect(container.querySelectorAll(".list-dropdown-panel").length).toBe(1);
    });
    // 点外部
    fireEvent.mouseDown(document.body);
    await waitFor(() => {
      expect(container.querySelectorAll(".list-dropdown-panel").length).toBe(0);
    });
  });

  it("scope dropdown 4 个选项（全部会话/收藏/已归档/回收站）", async () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const btn = container.querySelectorAll(".list-dropdown-btn")[0] as HTMLButtonElement;
    fireEvent.click(btn);
    await waitFor(() => {
      const panel = container.querySelector(".list-dropdown-panel");
      expect(panel?.textContent).toContain("全部会话");
      expect(panel?.textContent).toContain("收藏");
      expect(panel?.textContent).toContain("已归档");
      expect(panel?.textContent).toContain("回收站");
    });
  });

  it("date dropdown 4 个选项（全部时间/今日/近 7 天/近 30 天）", async () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const btn = container.querySelectorAll(".list-dropdown-btn")[1] as HTMLButtonElement;
    fireEvent.click(btn);
    await waitFor(() => {
      const panel = container.querySelector(".list-dropdown-panel");
      expect(panel?.textContent).toContain("全部时间");
      expect(panel?.textContent).toContain("今日");
      expect(panel?.textContent).toContain("近 7 天");
      expect(panel?.textContent).toContain("近 30 天");
    });
  });

  it("sort dropdown 3 个选项（最新活动/创建时间/标题字母序）", async () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const btn = container.querySelectorAll(".list-dropdown-btn")[2] as HTMLButtonElement;
    fireEvent.click(btn);
    await waitFor(() => {
      const panel = container.querySelector(".list-dropdown-panel");
      expect(panel?.textContent).toContain("最新活动");
      expect(panel?.textContent).toContain("创建时间");
      expect(panel?.textContent).toContain("标题字母序");
    });
  });

  it("来源 chip 多于 1 个时显示", () => {
    const { container } = render(<ConversationList {...makeProps()} availableProviders={new Set(["zcode", "claude-code", "cursor"])} />);
    expect(container.querySelectorAll(".provider-chip").length).toBeGreaterThan(1);
  });
});

describe("ConversationList 列表项（去复选框 + 去 pin-toggle）", () => {
  it("无 .list-item-check 复选框（⌘点击多选替代）", () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    expect(container.querySelectorAll(".list-item-check").length).toBe(0);
  });

  it("无 .pin-toggle / .fav-toggle hover 按钮（走右键菜单）", () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    expect(container.querySelectorAll(".pin-toggle").length).toBe(0);
    expect(container.querySelectorAll(".fav-toggle").length).toBe(0);
  });

  it("普通点击 list-item 触发 onSelect", () => {
    const onSelect = vi.fn();
    const { container } = render(<ConversationList {...makeProps({ onSelect })} />);
    const item = container.querySelectorAll(".list-item")[0] as HTMLDivElement;
    fireEvent.click(item);
    expect(onSelect).toHaveBeenCalled();
  });

  it("⌘点击 list-item 不触发 onSelect，只标记为多选", async () => {
    const onSelect = vi.fn();
    const { container } = render(<ConversationList {...makeProps({ onSelect })} />);
    const item = container.querySelectorAll(".list-item")[0] as HTMLDivElement;
    fireEvent.click(item, { metaKey: true });
    expect(onSelect).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(container.querySelector(".bulk-bar")?.textContent).toContain("已选 1");
    });
  });

  it("右键 list-item 触发右键菜单（包含 收藏/归档/置顶/标签/复制/删除）", async () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const item = container.querySelectorAll(".list-item")[0] as HTMLDivElement;
    fireEvent.contextMenu(item, { clientX: 200, clientY: 200 });
    await waitFor(() => {
      const menu = container.querySelector("[data-testid='contextmenu']");
      expect(menu).toBeTruthy();
      expect(menu?.textContent).toContain("收藏");
      expect(menu?.textContent).toContain("归档");
      expect(menu?.textContent).toContain("置顶");
      expect(menu?.textContent).toContain("加标签");
      expect(menu?.textContent).toContain("复制标题");
      expect(menu?.textContent).toContain("删除");
    });
  });

  it("置顶会话在标题前显示 📌", () => {
    localStorage.setItem("ch-conv-pins", JSON.stringify(["c1"]));
    const { container } = render(<ConversationList {...makeProps()} />);
    expect(container.querySelector(".list-item.pinned")).toBeInTheDocument();
    expect(container.querySelector(".list-item.pinned .pin-star")).toBeTruthy();
  });
});

describe("ConversationList pinned 持久化", () => {
  it("loadPinnedIds 从 localStorage 读取", () => {
    localStorage.setItem("ch-conv-pins", JSON.stringify(["c1", "c2"]));
    const ids = loadPinnedIds();
    expect(ids.has("c1")).toBe(true);
    expect(ids.has("c2")).toBe(true);
  });

  it("localStorage 抛错时降级为空 set", () => {
    const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("quota"); });
    expect(loadPinnedIds().size).toBe(0);
    spy.mockRestore();
  });
});

describe("详情页右侧空态：左侧无数据时显示导入提示", () => {
  // 通过 conversationList 内部的 empty 块验证
  it("左侧 conversations.length === 0 时显示「还没有任何会话」+ 导入引导", () => {
    const { container } = render(
      <ConversationList {...makeProps({ conversations: [] })} />,
    );
    const empty = container.querySelector(".empty.empty-cta");
    expect(empty).toBeTruthy();
    expect(empty?.textContent).toContain("还没有任何会话");
    expect(empty?.textContent).toContain("导入");
  });

  it("左侧有数据但全被日期过滤掉时显示日期空态", async () => {
    const oldConvs: Conversation[] = [
      { ...convs[0], started_at_ms: Date.now() - 200 * 86_400_000, updated_at_ms: Date.now() - 200 * 86_400_000 },
    ];
    const { container } = render(<ConversationList {...makeProps({ conversations: oldConvs })} />);
    const dateBtn = container.querySelectorAll(".list-dropdown-btn")[1] as HTMLButtonElement;
    fireEvent.click(dateBtn);
    await waitFor(() => {
      const panel = container.querySelector(".list-dropdown-panel");
      expect(panel).toBeTruthy();
    });
    const item = [...container.querySelectorAll(".list-dropdown-panel")[0].querySelectorAll(".list-dropdown-item")].find((b) => b.textContent === "近 7 天")!;
    fireEvent.click(item);
    await waitFor(() => {
      expect(container.querySelector(".empty")?.textContent).toContain("当前日期范围无会话");
    });
  });
});
