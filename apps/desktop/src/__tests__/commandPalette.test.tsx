// Command Palette（⌘K 全局跳转 + 会话搜索）测试
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommandPalette } from "../CommandPalette";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "list_conversations") {
      return [
        { id: "c1", provider: "codex", title: "修 bug 复盘", user_title: null, started_at_ms: Date.now() - 3600_000 },
        { id: "c2", provider: "zcode", title: "RAG 性能优化", user_title: "性能优化复盘", started_at_ms: Date.now() - 86400_000 },
        { id: "c3", provider: "minimax", title: "数据迁移", user_title: null, started_at_ms: null },
      ];
    }
    return [];
  }),
}));

describe("CommandPalette（⌘K）", () => {
  it("打开时自动聚焦输入框 + 列出全部页面", () => {
    const onJumpPage = vi.fn();
    const { container, getByPlaceholderText } = render(
      <CommandPalette open onClose={vi.fn()} onJumpPage={onJumpPage} />,
    );
    // 8 个页面都列出来
    expect(container.querySelectorAll(".cmd-row").length).toBeGreaterThanOrEqual(8);
    expect(getByPlaceholderText(/跳到页面/)).toBeTruthy();
  });

  it("过滤后只显示匹配的页面（搜「成本」→ cost 1 个）", async () => {
    const { container, getByPlaceholderText } = render(
      <CommandPalette open onClose={vi.fn()} onJumpPage={vi.fn()} />,
    );
    const input = getByPlaceholderText(/跳到页面/);
    fireEvent.change(input, { target: { value: "成本" } });
    await waitFor(() => {
      const rows = container.querySelectorAll(".cmd-row");
      // 只有 cost 1 个页面 + 0 个会话（因为没匹配）
      expect(rows.length).toBe(1);
      expect(rows[0].textContent).toContain("成本");
    });
  });

  it("会话搜索（搜「性能」→ 命中带 user_title 的会话）", async () => {
    const { container, getByPlaceholderText } = render(
      <CommandPalette open onClose={vi.fn()} onJumpPage={vi.fn()} />,
    );
    const input = getByPlaceholderText(/跳到页面/);
    fireEvent.change(input, { target: { value: "性能" } });
    await waitFor(() => {
      const rows = container.querySelectorAll(".cmd-row");
      // 性能 没匹配页面，但有 1 个会话
      expect(rows.length).toBe(1);
      expect(rows[0].textContent).toContain("性能");
    });
  });

  it("点击页面行触发 onJumpPage + onClose", () => {
    const onJumpPage = vi.fn();
    const onClose = vi.fn();
    const { container } = render(
      <CommandPalette open onClose={onClose} onJumpPage={onJumpPage} />,
    );
    // 点「活动」页面
    const activityRow = [...container.querySelectorAll(".cmd-row")].find((r) => r.textContent?.includes("活动"));
    expect(activityRow).toBeTruthy();
    fireEvent.click(activityRow!);
    expect(onJumpPage).toHaveBeenCalledWith("activity");
    expect(onClose).toHaveBeenCalled();
  });

  it("点击会话行触发 onJumpConversation", async () => {
    const onJumpConv = vi.fn();
    const onClose = vi.fn();
    const { container } = render(
      <CommandPalette open onClose={onClose} onJumpPage={vi.fn()} onJumpConversation={onJumpConv} />,
    );
    // 等会话列表加载完成
    await waitFor(() => {
      const convRows = [...container.querySelectorAll(".cmd-row")].filter((r) => r.textContent?.includes("bug"));
      expect(convRows.length).toBeGreaterThanOrEqual(1);
    });
    const convRow = [...container.querySelectorAll(".cmd-row")].find((r) => r.textContent?.includes("bug"));
    fireEvent.click(convRow!);
    expect(onJumpConv).toHaveBeenCalledWith("c1");
    expect(onClose).toHaveBeenCalled();
  });

  it("Esc 触发 onClose", () => {
    const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} onJumpPage={vi.fn()} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("未传 onJumpConversation 时点会话行只跳 chat 页", async () => {
    const onJumpPage = vi.fn();
    const onClose = vi.fn();
    const { container } = render(
      <CommandPalette open onClose={onClose} onJumpPage={onJumpPage} />,
    );
    // 等会话列表加载
    await waitFor(() => {
      const rows = [...container.querySelectorAll(".cmd-row")].filter((r) => r.textContent?.includes("bug"));
      expect(rows.length).toBeGreaterThanOrEqual(1);
    });
    const convRow = [...container.querySelectorAll(".cmd-row")].find((r) => r.textContent?.includes("bug"));
    fireEvent.click(convRow!);
    expect(onJumpPage).toHaveBeenCalledWith("chat");
    expect(onClose).toHaveBeenCalled();
  });
});
