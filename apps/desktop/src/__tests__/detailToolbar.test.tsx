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
  onToggleCollapse: vi.fn(), onToggleFavorite: vi.fn(), onToggleArchive: vi.fn(),
  onAddTag: vi.fn(), onRemoveTag: vi.fn(), onRescanAudit: vi.fn(),
};

describe("详情页按钮清单", () => {
  it("工具栏为：时间线/知识/重扫/收藏/归档/下载", () => {
    render(<ConversationDetail {...baseDetail} />);
    const bar = screen.getByText(/消息|时间线/).closest("div.detail-actions")!;
    expect(bar).toBeTruthy();
    expect(bar.textContent).toContain("时间线");
    expect(bar.textContent).toContain("知识");
    expect(bar.textContent).toContain("重扫");
    expect(bar.textContent).toContain("收藏");
    expect(bar.textContent).toContain("归档");
    expect(bar.textContent).toContain("下载");
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
});

describe("provider chips 显隐", () => {
  const listProps = {
    conversations: [], selectedConv: null, loading: false, providerFilter: null,
    selectedWs: null, expandedParents: new Set<string>(), childConvs: {},
    scope: "all" as const, onScopeChange: vi.fn(), onFilter: vi.fn(),
    onSelect: vi.fn(), onToggleExpand: vi.fn(), onClearWs: vi.fn(),
    onToggleFavorite: vi.fn(), onRestore: vi.fn(),
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
});
