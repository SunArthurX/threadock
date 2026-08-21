// 第 11 轮：高级交互测试
// 1) ConvItem 渲染 data-conv-row（j/k 导航锚点）
// 2) CSS 提供 --ease-modal 和 --transition-modal 令牌
// 3) ConvItem active / child 状态正确反映
import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import ConvItem from "../ConvItem";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const conv: import("../types").Conversation = {
  id: "conv-123",
  provider: "claude-code",
  source_conversation_id: "sc-123",
  title: "测试会话",
  user_title: null,
  status: null,
  model: null,
  completeness_score: null,
  workspace_id: null,
  source_parent_id: null,
  started_at_ms: Date.now() - 86_400_000,
  updated_at_ms: Date.now() - 3_600_000,
  child_count: 0,
  favorite: false,
  archived: false,
};

const baseProps = {
  isChild: false,
  isActive: false,
  isSelected: false,
  isPinned: false,
  isExpanded: false,
  scope: "all" as const,
  childCount: 0,
  onItemClick: () => {},
  onContextMenu: () => {},
  onToggleExpand: () => {},
  onRestore: () => {},
};

describe("ConvItem j/k 导航锚点（Round 11.3）", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => localStorage.clear());

  it("渲染 data-conv-row 属性（j/k 滚动到可见的钩子）", () => {
    const { container } = render(<ConvItem {...baseProps} conv={conv} />);
    const row = container.querySelector(`[data-conv-row="${conv.id}"]`);
    expect(row).toBeTruthy();
    expect(row?.getAttribute("data-conv-row")).toBe("conv-123");
  });

  it("active 项带 .active class（j/k 切换视觉反馈）", () => {
    const { container } = render(<ConvItem {...baseProps} conv={conv} isActive={true} />);
    const row = container.querySelector(`[data-conv-row="${conv.id}"]`);
    expect(row?.className).toContain("active");
  });

  it("子项带 .child-item class + 同样有 data-conv-row", () => {
    const child: import("../types").Conversation = { ...conv, id: "child-1" };
    const { container } = render(<ConvItem {...baseProps} conv={child} isChild={true} />);
    const row = container.querySelector(`[data-conv-row="child-1"]`);
    expect(row).toBeTruthy();
    expect(row?.className).toContain("child-item");
  });

  it("pinned 项带 .pinned class", () => {
    const { container } = render(<ConvItem {...baseProps} conv={conv} isPinned={true} />);
    const row = container.querySelector(`[data-conv-row="${conv.id}"]`);
    expect(row?.className).toContain("pinned");
  });
});

describe("Spring 动画令牌（Round 11.2）", () => {
  async function loadCss(): Promise<string> {
    const here = dirname(fileURLToPath(import.meta.url));
    return readFile(resolve(here, "../styles.css"), "utf8");
  }

  it("CSS 提供 --ease-modal + --transition-modal 令牌", async () => {
    const css = await loadCss();
    expect(css).toMatch(/--ease-modal:\s*cubic-bezier/);
    expect(css).toMatch(/--transition-modal:\s*\d+ms\s*var\(--ease-modal\)/);
    expect(css).toMatch(/animation:\s*modal-in\s+320ms\s+var\(--ease-modal\)/);
  });

  it("CommandPalette 用 cmd-in + ease-modal", async () => {
    const css = await loadCss();
    expect(css).toMatch(/@keyframes\s+cmd-in/);
    expect(css).toMatch(/animation:\s*cmd-in\s+\d+ms\s+var\(--ease-modal\)/);
  });
});
