// 交接（Handoff）功能测试：HandoffModal 目标清单/预演/执行 + ConvMenu 菜单项 +
// 详情工具栏「⇄ 交接」按钮。
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "handoff_targets") {
      return [
        { target: "claude-code", available: true, destination: "/tmp/x/.claude" },
        { target: "codex", available: true, destination: "/tmp/x/.codex" },
        { target: "zcode", available: false, destination: "/tmp/x/.zcode/cli/db/db.sqlite" },
        { target: "minimax-code", available: false, destination: "/tmp/mm.sqlite" },
        { target: "cursor", available: false, destination: "/tmp/vscdb" },
      ];
    }
    if (cmd === "handoff_conversations") {
      const target = args?.target === "all" ? "claude-code" : (args?.target as string);
      return {
        matched: 1,
        missing: [],
        dry_run: Boolean(args?.dryRun),
        results: [
          {
            target,
            ok: [
              {
                target,
                conversation_id: "c1",
                title: "修登录",
                session_id: "s-123",
                destination: "/tmp/x/projects/-tmp-proj/s-123.jsonl",
                items_written: 3,
                dry_run: Boolean(args?.dryRun),
              },
            ],
            failed: [],
          },
        ],
      };
    }
    return null;
  }),
}));

import { invoke } from "@tauri-apps/api/core";
import HandoffModal from "../HandoffModal";
import { buildConvMenuItems } from "../ConvMenu";
import ConversationDetail from "../ConversationDetail";
import type { Conversation } from "../types";

const conv: Conversation = {
  id: "c1", provider: "zcode", source_conversation_id: "s", title: "标题", user_title: null,
  status: null, model: null, completeness_score: null, workspace_id: null,
  started_at_ms: null, updated_at_ms: null, source_parent_id: null, child_count: 0,
  favorite: false, archived: false,
} as unknown as Conversation;

describe("HandoffModal", () => {
  it("渲染五个目标：可用可选、未安装灰掉，默认选中全部可用目标", async () => {
    render(<HandoffModal conversationIds={["c1"]} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText("Claude Code")).toBeTruthy());
    expect(screen.getByText("Codex")).toBeTruthy();
    // 三个未安装目标显示「未检测到」
    expect(screen.getAllByText("未检测到").length).toBeGreaterThanOrEqual(3);
    expect(screen.getByText("2 个目标可用")).toBeTruthy();
  });

  it("开始交接：以选中会话调用 handoff_conversations 并展示 resume 命令", async () => {
    render(<HandoffModal conversationIds={["c1"]} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText("Claude Code")).toBeTruthy());
    fireEvent.click(screen.getByText("开始交接"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("handoff_conversations", expect.objectContaining({
        conversationIds: ["c1"],
        target: "all",
        all: false,
        dryRun: false,
        includeContext: true,
      })),
    );
    expect(await screen.findByText(/claude --resume s-123/)).toBeTruthy();
  });

  it("预演：dryRun=true 且结果标注未写入", async () => {
    render(<HandoffModal conversationIds={["c1"]} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText("Claude Code")).toBeTruthy());
    fireEvent.click(screen.getByText("预演（不写入）"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("handoff_conversations", expect.objectContaining({
        dryRun: true,
      })),
    );
    expect(await screen.findByText(/预演结果（未写入）/)).toBeTruthy();
  });

  it("勾选「全部会话」后以 all=true 调用", async () => {
    render(<HandoffModal conversationIds={[]} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText("Claude Code")).toBeTruthy());
    fireEvent.click(screen.getByText("改为交接全部会话（排除已删除与子任务）"));
    fireEvent.click(screen.getByText("开始交接"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("handoff_conversations", expect.objectContaining({
        all: true,
      })),
    );
  });
});

describe("交接触点", () => {
  it("右键菜单：多选时出现「交接 N 条到其他 Agent…」并回传目标 ids", () => {
    const onBulkHandoff = vi.fn();
    const items = buildConvMenuItems({
      conv,
      x: 10,
      y: 10,
      scope: "all",
      pinned: new Set(),
      onTogglePin: () => {},
      selectedIds: new Set(["c1", "c2"]),
      conversations: [conv],
      onBulkHandoff,
      openTagInput: () => {},
    });
    const item = items.find((i) => i.label.includes("交接"));
    expect(item).toBeTruthy();
    expect(item?.label).toContain("2");
    item?.onClick();
    expect(onBulkHandoff).toHaveBeenCalledWith(["c1", "c2"]);
  });

  it("详情工具栏：提供 onHandoff 时渲染「⇄ 交接」按钮", () => {
    const onHandoff = vi.fn();
    render(
      <ConversationDetail
        conv={conv}
        messages={[]}
        events={[]}
        completenessLabel=""
        loading={false}
        exporting={false}
        timelineMode={false}
        highlightMsgId={null}
        collapsedMsgs={new Set()}
        tags={[]}
        onToggleTimeline={() => {}}
        onExport={() => {}}
        onHandoff={onHandoff}
        onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}}
        onRemoveTag={() => {}}
        onRescanAudit={() => {}}
      />,
    );
    const btn = screen.getByText("⇄ 交接");
    fireEvent.click(btn);
    expect(onHandoff).toHaveBeenCalledTimes(1);
  });
});
