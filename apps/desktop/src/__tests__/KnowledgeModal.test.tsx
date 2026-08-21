// 知识弹窗测试：分区渲染 / 纪要生成 / 空态 / 关闭交互
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import KnowledgeModal, { knowledgeToMarkdown } from "../KnowledgeModal";
import type { ExtractionResult } from "../types";

const full: ExtractionResult = {
  summary: "摘要内容",
  decisions: [{ decision: "用 SQLite" }],
  todos: [{ text: "写测试" }],
  errors: [{ error: "超时" }],
  commands: ["cargo build"],
  files: [{ path: "src/main.rs" }],
  extractor: "rule-v1",
};
const base = { knowledge: full, convTitle: "会话A", onClose: vi.fn(), onReextract: vi.fn() };

describe("KnowledgeModal", () => {
  it("弹窗展示标题与各分区", () => {
    render(<KnowledgeModal {...base} />);
    expect(screen.getByText("知识提取结果")).toBeTruthy();
    expect(screen.getByText("会话A")).toBeTruthy();
    expect(screen.getByText("摘要内容")).toBeTruthy();
    expect(screen.getByText(/用 SQLite/)).toBeTruthy();
    expect(screen.getByText(/写测试/)).toBeTruthy();
    expect(screen.getByText(/超时/)).toBeTruthy();
    expect(screen.getByText(/cargo build/)).toBeTruthy();
    expect(screen.getByText(/src\/main.rs/)).toBeTruthy();
  });

  it("关闭与重新提取按钮触发回调", () => {
    render(<KnowledgeModal {...base} />);
    // 关闭按钮改用 Icon 组件 + aria-label="关闭"
    fireEvent.click(screen.getByLabelText("关闭"));
    expect(base.onClose).toHaveBeenCalled();
    fireEvent.click(screen.getByText("重新提取"));
    expect(base.onReextract).toHaveBeenCalled();
  });

  it("空结果显示空态说明", () => {
    const empty = { summary: "", decisions: [], todos: [], errors: [], commands: [], files: [], extractor: "r" };
    render(<KnowledgeModal {...base} knowledge={empty} />);
    expect(screen.getByText(/未提取到知识要点/)).toBeTruthy();
  });

  it("knowledgeToMarkdown 生成完整纪要", () => {
    const md = knowledgeToMarkdown(full);
    expect(md).toContain("# 会话纪要");
    expect(md).toContain("- 用 SQLite");
    expect(md).toContain("- [ ] 写测试");
    expect(md).toContain("- `cargo build`");
  });
});
