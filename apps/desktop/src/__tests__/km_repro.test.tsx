// 弹窗打开链路回归：骨架（knowledge=null）→ 存档到达 → 各分区内容必须渲染。
// 背景（2026-08）：「默认都不展示」反馈——防御骨架态与数据态切换的渲染契约。
import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import KnowledgeModal from "../KnowledgeModal";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(async () => null) }));

const full = {
  summary: "这是摘要内容",
  decisions: [{ decision: "决定用 SQLite" }],
  todos: [],
  errors: [],
  commands: ["cargo test"],
  files: [],
  extractor: "rule-v2",
};

describe("骨架 → 数据到达", () => {
  it("rerender 后各分区内容显示", async () => {
    const { rerender } = render(
      <KnowledgeModal knowledge={null} onClose={() => {}} onReextract={() => {}} />,
    );
    expect(screen.getByText(/正在读取/)).toBeTruthy();
    rerender(
      <KnowledgeModal knowledge={full} onClose={() => {}} onReextract={() => {}} />,
    );
    await waitFor(() => expect(screen.getByText(/这是摘要内容/)).toBeTruthy());
    expect(screen.getByText(/决定用 SQLite/)).toBeTruthy();
    expect(screen.getByText(/cargo test/)).toBeTruthy();
    expect(screen.getByText("全部")).toBeTruthy();
  });
});
