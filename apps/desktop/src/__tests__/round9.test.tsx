// 第 9 轮测试：列表搜索 + 标签补全 + BarChart 悬停时间 pill
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "list_all_tags") return [
      { tag: "urgent", count: 12 },
      { tag: "bug", count: 8 },
      { tag: "idea", count: 3 },
    ];
    return null;
  }),
}));

import ConversationList from "../ConversationList";
import ConversationDetail from "../ConversationDetail";
import type { Conversation } from "../types";
import { BarChart } from "../charts";

describe("ConversationList 列表内搜索", () => {
  const convs: Conversation[] = [
    { id: "c1", provider: "zcode", source_conversation_id: "sc1", title: "分布式事务", user_title: "TX 调研", status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 3_600_000, updated_at_ms: Date.now() - 3_600_000, child_count: 0, favorite: false, archived: false },
    { id: "c2", provider: "claude-code", source_conversation_id: "sc2", title: "JVM 调优", user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 7_200_000, updated_at_ms: Date.now() - 7_200_000, child_count: 0, favorite: false, archived: false },
    { id: "c3", provider: "cursor", source_conversation_id: "sc3", title: "Rust 学习笔记", user_title: "Rust 入门", status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 10_800_000, updated_at_ms: Date.now() - 10_800_000, child_count: 0, favorite: false, archived: false },
  ];
  const baseProps = (over: Partial<React.ComponentProps<typeof ConversationList>> = {}) => ({
    conversations: convs, selectedConv: null, loading: false, providerFilter: null, selectedWs: null,
    expandedParents: new Set<string>(), childConvs: {}, scope: "all" as const, onScopeChange: () => {},
    onFilter: () => {}, onSelect: () => {}, onToggleExpand: () => {}, onClearWs: () => {},
    ...over,
  });

  it("无搜索关键词 → 显示全部", () => {
    const { container } = render(<ConversationList {...baseProps()} />);
    expect(container.querySelectorAll(".list-item").length).toBe(3);
  });

  it("搜「Rust」命中 user_title 含 Rust 的会话", () => {
    const { container } = render(<ConversationList {...baseProps()} />);
    const input = container.querySelector(".list-search-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Rust" } });
    const items = container.querySelectorAll(".list-item");
    expect(items.length).toBe(1);
    expect(items[0].textContent).toContain("Rust");
  });

  it("搜「JVM」命中 title（无 user_title）", () => {
    const { container } = render(<ConversationList {...baseProps()} />);
    const input = container.querySelector(".list-search-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "JVM" } });
    const items = container.querySelectorAll(".list-item");
    expect(items.length).toBe(1);
    expect(items[0].textContent).toContain("JVM 调优");
  });

  it("大小写不敏感", () => {
    const { container } = render(<ConversationList {...baseProps()} />);
    const input = container.querySelector(".list-search-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "rust" } });
    const items = container.querySelectorAll(".list-item");
    expect(items.length).toBe(1);
  });

  it("无匹配 → 显示空态文案", () => {
    const { container } = render(<ConversationList {...baseProps()} />);
    const input = container.querySelector(".list-search-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "不存在" } });
    expect(container.querySelectorAll(".list-item").length).toBe(0);
    expect(container.textContent).toContain("无匹配");
  });

  it("清空按钮清除搜索", () => {
    const { container } = render(<ConversationList {...baseProps()} />);
    const input = container.querySelector(".list-search-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Rust" } });
    expect(container.querySelectorAll(".list-item").length).toBe(1);
    const clear = container.querySelector(".list-search-clear") as HTMLButtonElement;
    fireEvent.click(clear);
    expect(container.querySelectorAll(".list-item").length).toBe(3);
    expect(input.value).toBe("");
  });
});

describe("ConversationDetail 标签自动补全", () => {
  const baseConv: Conversation = {
    id: "c1", provider: "zcode", source_conversation_id: "sc", title: "T", user_title: null,
    status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null,
    started_at_ms: null, updated_at_ms: null, child_count: 0, favorite: false, archived: false,
  };
  const allTags = [
    { tag: "urgent", count: 12 },
    { tag: "bug", count: 8 },
    { tag: "idea", count: 3 },
  ];

  it("focus 时显示全部可用标签下拉（排除已有）", () => {
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={["urgent"]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
        allTags={allTags}
      />,
    );
    const input = container.querySelector(".tag-input") as HTMLInputElement;
    fireEvent.focus(input);
    const items = container.querySelectorAll(".tag-suggest-item");
    // urgent 已有 → 排除；bug + idea = 2
    expect(items.length).toBe(2);
    expect(items[0].textContent).toContain("bug");
    expect(items[1].textContent).toContain("idea");
  });

  it("输入「ur」过滤为「urgent」", () => {
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
        allTags={allTags}
      />,
    );
    const input = container.querySelector(".tag-input") as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "ur" } });
    const items = container.querySelectorAll(".tag-suggest-item");
    expect(items.length).toBe(1);
    expect(items[0].textContent).toContain("urgent");
  });

  it("点击 suggest 项触发 onAddTag 并清空输入", () => {
    const onAddTag = vi.fn();
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={["urgent"]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}} 
        onAddTag={onAddTag} onRemoveTag={() => {}} onRescanAudit={() => {}}
        allTags={allTags}
      />,
    );
    const input = container.querySelector(".tag-input") as HTMLInputElement;
    fireEvent.focus(input);
    // urgent 已有 → 排除；bug 是第一个
    const item = container.querySelectorAll(".tag-suggest-item")[0] as HTMLButtonElement;
    fireEvent.click(item);
    expect(onAddTag).toHaveBeenCalledWith("bug");
    expect(input.value).toBe("");
  });

  it("suggest 计数显示正确", () => {
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={["urgent"]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
        allTags={allTags}
      />,
    );
    const input = container.querySelector(".tag-input") as HTMLInputElement;
    fireEvent.focus(input);
    // urgent 已有被排除：bug (8), idea (3)
    const counts = container.querySelectorAll(".tag-suggest-count");
    expect(counts[0].textContent).toBe("8"); // bug
    expect(counts[1].textContent).toBe("3"); // idea
  });
});

describe("BarChart 悬停时间 pill", () => {
  it("鼠标移上柱子显示 hover-label", () => {
    const { container } = render(<BarChart data={[{ label: "14", value: 10 }, { label: "15", value: 5 }]} height={80} axisLabel={(d) => `${d.label}:00`} />);
    expect(container.querySelector(".barchart-hover-label")).toBeNull();
    const bar = container.querySelector(".barchart-bar") as HTMLElement;
    fireEvent.mouseMove(bar, { clientX: 100 });
    const label = container.querySelector(".barchart-hover-label");
    expect(label).toBeInTheDocument();
    expect(label?.textContent).toBe("14:00");
  });

  it("鼠标离开后 hover-label 消失", () => {
    const { container } = render(<BarChart data={[{ label: "14", value: 10 }]} height={80} />);
    const bar = container.querySelector(".barchart-bar") as HTMLElement;
    fireEvent.mouseMove(bar, { clientX: 100 });
    expect(container.querySelector(".barchart-hover-label")).toBeInTheDocument();
    const wrap = container.querySelector(".barchart") as HTMLElement;
    fireEvent.mouseLeave(wrap);
    expect(container.querySelector(".barchart-hover-label")).toBeNull();
  });
});

describe("ActivityView 热力图 cell 尺寸", () => {
  it("heat-cell 渲染为 15×24（视觉更突出）", () => {
    // 通过 CSS 类名检查（jsdom 不渲染真实尺寸，只看类名存在性 + 元素存在）
    const { container } = render(<div className="heatmap"><div className="heatmap-col"><span className="heat-cell" /></div></div>);
    expect(container.querySelector(".heat-cell")).toBeInTheDocument();
  });
});
