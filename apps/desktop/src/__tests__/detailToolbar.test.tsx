// 详情页按钮清单 / provider chips 显隐
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { RefObject } from "react";
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
    const btn = screen.getByText("知识") as HTMLButtonElement;
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

describe("执行事件挂到对应消息下 + 详情展开", () => {
  const genEvents = (n: number, startSeq: number) =>
    Array.from({ length: n }, (_, i) => ({
      id: `e${startSeq + i}`, event_type: "tool_call_started",
      summary: `事件 ${startSeq + i}`, sequence_number: startSeq + i,
      created_at_ms: startSeq + i, status: null, completed_at_ms: null, payload_json: null,
    }));
  const msgs = [
    { id: "m1", role: "user", content_text: "第一问", sequence_number: 10, created_at_ms: 10 },
    { id: "m2", role: "assistant", content_text: "回答", sequence_number: 20, created_at_ms: 20 },
  ];

  it("事件渲染在所属消息块内部（seq ≤ 最大消息序号），不再有底部平铺区", () => {
    const { container } = render(
      <ConversationDetail {...baseDetail} messages={msgs} events={genEvents(3, 11)} />,
    );
    // 3 个事件 seq 11/12/13 全部归属 m1
    const m1 = container.querySelector("#msg-m1")!;
    const m2 = container.querySelector("#msg-m2")!;
    expect(m1.querySelectorAll(".msg-event-row").length).toBe(3);
    expect(m2.querySelectorAll(".msg-event-row").length).toBe(0);
    expect(container.querySelector(".events-header")).toBeNull();
  });

  it("点击事件行展开详情（完整摘要/状态/耗时/payload JSON）", () => {
    const events = [{
      id: "e1", event_type: "command_started", summary: "cargo build --release",
      sequence_number: 11, created_at_ms: 1_000, completed_at_ms: 53_000,
      status: "completed", payload_json: JSON.stringify({ exit_code: 0, duration_s: 52 }),
    }];
    const { container } = render(
      <ConversationDetail {...baseDetail} messages={msgs} events={events} />,
    );
    expect(container.querySelector(".msg-event-detail")).toBeNull();
    fireEvent.click(container.querySelector(".msg-event-row")!);
    const detail = container.querySelector(".msg-event-detail")!;
    expect(detail.textContent).toContain("cargo build --release");
    expect(detail.textContent).toContain("状态 completed");
    expect(detail.textContent).toContain("耗时 52s");
    expect(detail.textContent).toContain("\"exit_code\": 0");
    // 再点收起
    fireEvent.click(container.querySelector(".msg-event-row")!);
    expect(container.querySelector(".msg-event-detail")).toBeNull();
  });

  it("单条消息超过 4 个事件折叠，「还有 N 条」展开", () => {
    const { container } = render(
      <ConversationDetail {...baseDetail} messages={msgs} events={genEvents(7, 11)} />,
    );
    expect(container.querySelectorAll(".msg-event-row").length).toBe(4);
    expect(screen.getByText("还有 3 条 ▾")).toBeTruthy();
    fireEvent.click(screen.getByText("还有 3 条 ▾"));
    expect(container.querySelectorAll(".msg-event-row").length).toBe(7);
    fireEvent.click(screen.getByText("收起 ▴"));
    expect(container.querySelectorAll(".msg-event-row").length).toBe(4);
  });

  it("早于首条消息的事件显示为顶部「会话前置事件」组", () => {
    const { container } = render(
      <ConversationDetail {...baseDetail} messages={msgs} events={genEvents(2, 1)} />,
    );
    expect(container.querySelector(".msg-events-label")?.textContent).toContain("会话前置事件");
    expect(container.querySelectorAll(".msg-event-row").length).toBe(2);
    expect(container.querySelector("#msg-m1")!.querySelectorAll(".msg-event-row").length).toBe(0);
  });
});

describe("快速上/下浮动按钮", () => {
  /** 伪造滚动容器：jsdom 无布局，手动喂 scrollHeight/clientHeight/scrollTop。 */
  function fakeScrollRef() {
    const listeners: ((e: unknown) => void)[] = [];
    const el = {
      scrollHeight: 3000, clientHeight: 600, scrollTop: 0,
      addEventListener: (_t: string, fn: (e: unknown) => void) => { listeners.push(fn); },
      removeEventListener: () => {},
      scrollTo: vi.fn(),
    } as unknown as HTMLElement;
    const ref = { current: { inner: el } } as unknown as RefObject<
      { inner: HTMLElement | null } | HTMLElement | null
    >;
    const setScroll = (top: number) => {
      el.scrollTop = top;
      listeners.forEach((fn) => fn({}));
    };
    return { ref, el, setScroll };
  }

  const manyMsgs = Array.from({ length: 30 }, (_, i) => ({
    id: `m${i}`, role: i % 2 ? "assistant" : "user",
    content_text: `消息 ${i}`, sequence_number: i + 1, created_at_ms: i + 1,
  }));

  it("顶部时两按钮都不显示；中部仅 ↑；底部仅 ↓", async () => {
    const { ref, setScroll } = fakeScrollRef();
    const { container, rerender } = render(
      <ConversationDetail {...baseDetail} messages={manyMsgs} scrollContainerRef={ref} />,
    );
    // 初始 scrollTop=0：都在顶部，两个按钮都不出现
    expect(container.querySelector(".jump-top-btn")).toBeNull();
    expect(container.querySelector(".jump-bottom-btn")).not.toBeNull();
    // 滚到中部（scrollTop=1500，距底 900、距顶 1500）：↑ 出现、↓ 仍在
    setScroll(1500);
    rerender(<ConversationDetail {...baseDetail} messages={manyMsgs} scrollContainerRef={ref} />);
    expect(container.querySelector(".jump-top-btn")).not.toBeNull();
    expect(container.querySelector(".jump-bottom-btn")).not.toBeNull();
    // 滚到底部（scrollTop=2400，距底 0）：↑ 在、↓ 消失
    setScroll(2400);
    rerender(<ConversationDetail {...baseDetail} messages={manyMsgs} scrollContainerRef={ref} />);
    expect(container.querySelector(".jump-top-btn")).not.toBeNull();
    expect(container.querySelector(".jump-bottom-btn")).toBeNull();
  });

  it("点 ↑ 平滑回到顶部", async () => {
    const { ref, el, setScroll } = fakeScrollRef();
    const { container } = render(
      <ConversationDetail {...baseDetail} messages={manyMsgs} scrollContainerRef={ref} />,
    );
    setScroll(1500);
    await waitFor(() => expect(container.querySelector(".jump-top-btn")).not.toBeNull());
    fireEvent.click(container.querySelector(".jump-top-btn")!);
    expect(el.scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "smooth" });
  });
});
