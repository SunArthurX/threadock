// 详情页按钮清单 / provider chips 显隐
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ConversationDetail from "../ConversationDetail";
import ConversationList from "../ConversationList";

const conv = {
  id: "c1", provider: "zcode", source_conversation_id: "s", title: "标题", user_title: null,
  status: null, model: null, completeness_score: null, workspace_id: null,
  started_at_ms: null, updated_at_ms: null, source_parent_id: null, child_count: 0,
  favorite: false, archived: false,
};
const baseDetail = {
  conv, messages: [], events: [], completenessLabel: "",
  loading: false, exporting: false, timelineMode: false, highlightMsgId: null,
  collapsedMsgs: new Set<string>(), tags: [],
  onToggleTimeline: vi.fn(), onExport: vi.fn(), onExtractKnowledge: vi.fn(),
  onToggleCollapse: vi.fn(),onToggleArchive: vi.fn(),
  onAddTag: vi.fn(), onRemoveTag: vi.fn(), onRescanAudit: vi.fn(),
};

describe("详情页按钮清单", () => {
  it("工具栏为：时间线/知识/重扫/仅用户消息/搜索消息/复制全部/下载（收藏/归档已移至右键菜单）", () => {
    render(<ConversationDetail {...baseDetail} />);
    const bar = screen.getByText(/时间线/).closest("div.detail-actions")!;
    expect(bar).toBeTruthy();
    const labels = ["时间线", "知识", "重扫", "仅用户消息", "搜索消息", "复制全部", "下载"];
    let last = -1;
    for (const label of labels) {
      const idx = (bar.textContent ?? "").indexOf(label);
      expect(idx).toBeGreaterThan(last);
      last = idx;
    }
    // 收藏/归档不在 toolbar
    expect(bar.textContent).not.toContain("收藏");
    expect(bar.textContent).not.toContain("归档");
  });

  it("用户消息筛选：开启后仅显示 user 消息，关闭恢复全部", () => {
    const messages = [
      { id: "m1", role: "user", content_text: "我的提问", sequence_number: 1, created_at_ms: 1000 },
      { id: "m2", role: "assistant", content_text: "助手回答", sequence_number: 2, created_at_ms: 2000 },
      { id: "m3", role: "user", content_text: "第二问", sequence_number: 3, created_at_ms: 3000 },
    ];
    const { container } = render(<ConversationDetail {...baseDetail} messages={messages} />);
    expect(screen.getByText("我的提问")).toBeTruthy();
    expect(screen.getByText("助手回答")).toBeTruthy();
    fireEvent.click(screen.getByText("👤 仅用户消息"));
    expect(screen.getByText("我的提问")).toBeTruthy();
    expect(screen.queryByText("助手回答")).toBeNull();
    expect(screen.getByText("第二问")).toBeTruthy();
    expect(container.querySelectorAll(".message").length).toBe(2);
    // 再点关闭恢复
    fireEvent.click(screen.getByText("👤 仅用户消息"));
    expect(screen.getByText("助手回答")).toBeTruthy();
  });

  it("无删除入口（软删与彻底删除都已移除）", () => {
    render(<ConversationDetail {...baseDetail} />);
    expect(screen.queryByText(/删除/)).toBeNull();
  });

  it("下载为下拉：默认不显示格式项，点开可选 Markdown / JSON", () => {
    render(<ConversationDetail {...baseDetail} />);
    expect(screen.queryByText("📄 Markdown（.md）")).toBeNull();
    fireEvent.click(screen.getByText(/下载/));
    fireEvent.click(screen.getByText("📄 Markdown（.md）"));
    expect(baseDetail.onExport).toHaveBeenCalledWith("markdown");
    expect(screen.queryByText("📄 Markdown（.md）")).toBeNull();
  });

  it("无消息时知识按钮禁用", () => {
    render(<ConversationDetail {...baseDetail} />);
    const btn = screen.getByText("✨ 知识") as HTMLButtonElement;
    expect(btn).toBeDisabled();
  });

  it("消息内搜索：⌘F 唤起，输入关键词高亮并显示 N/M 计数", () => {
    const messages = [
      { id: "m1", role: "user", content_text: "数据库连接失败", sequence_number: 1, created_at_ms: 1000 },
      { id: "m2", role: "assistant", content_text: "数据库超时，建议重试", sequence_number: 2, created_at_ms: 2000 },
      { id: "m3", role: "user", content_text: "另一段对话", sequence_number: 3, created_at_ms: 3000 },
    ];
    const { container } = render(<ConversationDetail {...baseDetail} messages={messages} />);
    // 唤起搜索
    fireEvent.keyDown(window, { key: "f", metaKey: true } as KeyboardEvent);
    const input = container.querySelector(".msg-search-input") as HTMLInputElement;
    expect(input).toBeTruthy();
    // 输入关键词
    fireEvent.change(input, { target: { value: "数据库" } });
    // 应该有 2 个匹配 + 高亮
    expect(container.querySelectorAll(".msg-search-hit").length).toBe(2);
    expect(container.querySelector(".msg-search-count")?.textContent).toContain("1 / 2");
    // 没有匹配的消息不应有 hit
    const lastMsg = container.querySelectorAll(".message")[2];
    expect(lastMsg.querySelector(".msg-search-hit")).toBeNull();
  });
});

describe("provider chips 显隐", () => {
  const listProps = {
    conversations: [], selectedConv: null, loading: false, providerFilter: null,
    selectedWs: null, expandedParents: new Set<string>(), childConvs: {},
    scope: "all" as const, onScopeChange: vi.fn(), onFilter: vi.fn(),
    onSelect: vi.fn(), onToggleExpand: vi.fn(), onClearWs: vi.fn(),
   onRestore: vi.fn(),
  };

  it("availableProviders 不含 cursor 时隐藏 Cursor 标签", () => {
    render(<ConversationList {...listProps} availableProviders={new Set(["zcode", "codex"])} />);
    expect(screen.queryByText("Cursor")).toBeNull();
    expect(screen.getByText("ZCode")).toBeTruthy();
  });

  it("未加载（空集合）时显示全部来源", () => {
    render(<ConversationList {...listProps} />);
    expect(screen.getByText("Cursor")).toBeTruthy();
  });

  it("默认 scope dropdown 显示「全部会话」（第 11 轮：4 行 filter → dropdown）", () => {
    render(<ConversationList {...listProps} />);
    // scope dropdown 第一个，默认 label 是"全部会话"
    const allScopes = screen.getAllByText("全部会话");
    expect(allScopes.length).toBeGreaterThan(0);
    const scopeBtn = allScopes[0].closest(".list-dropdown-btn");
    expect(scopeBtn).toBeTruthy();
  });

  it("scope dropdown 打开后包含「全部会话/收藏/已归档/回收站」4 项", () => {
    const { container } = render(<ConversationList {...listProps} />);
    const scopeBtn = container.querySelectorAll(".list-dropdown-btn")[0]!;
    fireEvent.click(scopeBtn);
    const panel = container.querySelectorAll(".list-dropdown-panel")[0]!;
    expect(panel.textContent).toContain("全部会话");
    expect(panel.textContent).toContain("收藏");
    expect(panel.textContent).toContain("已归档");
    expect(panel.textContent).toContain("回收站");
  });
});

describe("执行事件分页（超过 30 条分页展示）", () => {
  const genEvents = (n: number) => Array.from({ length: n }, (_, i) => ({
    id: `e${i}`, event_type: "tool_call_started", summary: `事件 ${i}`,
    sequence_number: i, created_at_ms: i,
  }));

  it("≤30 条不分页、不渲染翻页器", () => {
    const { container } = render(<ConversationDetail {...baseDetail} events={genEvents(30)} />);
    expect(container.querySelectorAll(".event").length).toBe(30);
    expect(container.querySelector(".pager")).toBeNull();
  });

  it(">30 条每页 30 条，页码信息正确", () => {
    const { container } = render(<ConversationDetail {...baseDetail} events={genEvents(35)} />);
    expect(container.querySelectorAll(".event").length).toBe(30);
    expect(container.querySelector(".pager-info")?.textContent).toContain("1 / 2 页 · 共 35 条");
  });

  it("翻页到最后一条数据齐全，边界按钮禁用正确", () => {
    const { container } = render(<ConversationDetail {...baseDetail} events={genEvents(65)} />);
    expect(container.querySelector(".pager-info")?.textContent).toContain("1 / 3 页");
    fireEvent.click(screen.getByText("下一页 ›"));
    expect(container.querySelectorAll(".event").length).toBe(30);
    expect(container.querySelector(".pager-info")?.textContent).toContain("2 / 3 页");
    fireEvent.click(screen.getByText("下一页 ›"));
    expect(container.querySelectorAll(".event").length).toBe(5);
    expect((screen.getByText("‹ 上一页") as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByText("下一页 ›") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByText("‹ 上一页"));
    expect(container.querySelectorAll(".event").length).toBe(30);
    expect((screen.getByText("‹ 上一页") as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByText("‹ 上一页"));
    expect((screen.getByText("‹ 上一页") as HTMLButtonElement).disabled).toBe(true);
  });
});
