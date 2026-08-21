// 「⏯ 恢复会话」按钮：点击直接在系统终端执行恢复命令；失败回退复制；右键复制命令文本。
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConversationDetail from "../ConversationDetail";
import { invoke } from "@tauri-apps/api/core";
import { copyToClipboard } from "../clipboard";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
vi.mock("../clipboard", () => ({ copyToClipboard: vi.fn(async () => ({ ok: true })) }));
vi.mock("../toast", () => ({ showToast: vi.fn() }));

const conv = {
  id: "c1", provider: "claude-code", source_conversation_id: "sess-abc",
  title: "标题", user_title: null, status: null, model: null,
  completeness_score: null, workspace_id: null, started_at_ms: null,
  updated_at_ms: null, source_parent_id: null, child_count: 0,
  favorite: false, archived: false,
};
const base = {
  conv, messages: [], events: [], completenessLabel: "",
  loading: false, exporting: false, timelineMode: false, highlightMsgId: null,
  collapsedMsgs: new Set<string>(), tags: [],
  onToggleTimeline: vi.fn(), onExport: vi.fn(), onExtractKnowledge: vi.fn(),
  onToggleCollapse: vi.fn(), onToggleArchive: vi.fn(),
  onAddTag: vi.fn(), onRemoveTag: vi.fn(), onRescanAudit: vi.fn(),
};

describe("恢复会话按钮", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(copyToClipboard).mockClear();
  });

  it("点击 → 调 resume_in_terminal（直接开终端，不再只复制）", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("claude --resume sess-abc");
    render(<ConversationDetail {...base} />);
    fireEvent.click(screen.getByText("⏯ 恢复会话"));
    await vi.waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("resume_in_terminal", { conversationId: "c1" }),
    );
    expect(copyToClipboard).not.toHaveBeenCalled();
  });

  it("终端打开失败 → 回退复制命令文本", async () => {
    vi.mocked(invoke)
      .mockRejectedValueOnce("Terminal 打开失败：xxx") // resume_in_terminal 失败
      .mockResolvedValueOnce("claude --resume sess-abc"); // 回退的 resume_command
    render(<ConversationDetail {...base} />);
    fireEvent.click(screen.getByText("⏯ 恢复会话"));
    await vi.waitFor(() => expect(copyToClipboard).toHaveBeenCalledWith("claude --resume sess-abc"));
  });

  it("来源不支持（null）→ 提示且不动终端/剪贴板", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    const { container } = render(
      <ConversationDetail {...base} conv={{ ...conv, provider: "zcode" }} />,
    );
    fireEvent.click(screen.getByText("⏯ 恢复会话"));
    await vi.waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledTimes(1));
    expect(copyToClipboard).not.toHaveBeenCalled();
    expect(container).toBeTruthy();
  });

  it("右键 → 复制命令文本（不开终端）", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("codex resume s2");
    render(
      <ConversationDetail {...base} conv={{ ...conv, provider: "codex", source_conversation_id: "s2" }} />,
    );
    fireEvent.contextMenu(screen.getByText("⏯ 恢复会话"));
    await vi.waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("resume_command", { conversationId: "c1" });
      expect(copyToClipboard).toHaveBeenCalledWith("codex resume s2");
    });
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "resume_in_terminal")).toBe(false);
  });
});
