// 第 7 轮大改版测试：跨会话知识引用、Settings 导入导出、批量加标签、inline 标题编辑
import { fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: any) => {
    if (cmd === "knowledge_xref") {
      const keywords = (args?.keywords ?? []) as { text: string; kind: string }[];
      return keywords
        .filter((k) => k.text.includes("main.rs") || k.text.includes("npm"))
        .map((k) => ({
          keyword: k.text,
          kind: k.kind,
          other_count: 3,
          other_conversations: [
            { id: "c-other-1", title: "其他会话 1", provider: "claude-code", updated_at_ms: Date.now() - 86_400_000 },
            { id: "c-other-2", title: "相关任务 2", provider: "zcode", updated_at_ms: Date.now() - 86_400_000 * 2 },
            { id: "c-other-3", title: null, provider: "cursor", updated_at_ms: null },
          ],
        }));
    }
    if (cmd === "save_text_file") return null;
    if (cmd === "read_text_file") return JSON.stringify({ version: 1, prefs: { "ch-theme": "light" } });
    if (cmd === "set_user_title") return null;
    return null;
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(async () => "/tmp/test.json"),
  open: vi.fn(async () => "/tmp/import.json"),
}));

import KnowledgeModal from "../KnowledgeModal";
import { exportAllSettings, importAllSettings, defaultSettingsFilename } from "../settingsIO";
import ConversationList from "../ConversationList";
import type { Conversation } from "../types";

describe("settingsIO 配置导入导出", () => {
  beforeEach(() => {
    localStorage.clear();
  });
  afterEach(() => localStorage.clear());

  it("exportAllSettings 返回带 version + exported_at 的 JSON", () => {
    localStorage.setItem("ch-theme", "light");
    const json = exportAllSettings();
    const obj = JSON.parse(json);
    expect(obj.version).toBe(1);
    expect(typeof obj.exported_at).toBe("string");
    expect(obj.prefs["ch-theme"]).toBe("light");
  });

  it("importAllSettings merge 模式只覆盖白名单 key", () => {
    localStorage.setItem("ch-theme", "dark");
    localStorage.setItem("ch-evil", "should-not-be-touched"); // 不在白名单
    const json = JSON.stringify({
      version: 1,
      prefs: { "ch-theme": "light", "ch-evil-injected": "x" },
    });
    const r = importAllSettings(json, "merge");
    expect(r.applied).toBe(1); // 只导入 ch-theme
    expect(r.skipped).toBe(1); // ch-evil-injected 不在白名单
    expect(localStorage.getItem("ch-theme")).toBe("light");
  });

  it("importAllSettings replace 模式先清空再导入", () => {
    localStorage.setItem("ch-theme", "dark");
    localStorage.setItem("ch-sort-by", "title");
    const json = JSON.stringify({
      version: 1,
      prefs: { "ch-theme": "light" },
    });
    importAllSettings(json, "replace");
    expect(localStorage.getItem("ch-theme")).toBe("light");
    expect(localStorage.getItem("ch-sort-by")).toBeNull(); // 被清空
  });

  it("importAllSettings 错误格式抛错", () => {
    expect(() => importAllSettings("not json", "merge")).toThrow();
    expect(() => importAllSettings(JSON.stringify({ version: 999, prefs: {} }), "merge")).toThrow(/版本/);
    expect(() => importAllSettings(JSON.stringify({ version: 1 }), "merge")).toThrow();
  });

  it("defaultSettingsFilename 含日期", () => {
    const fn = defaultSettingsFilename(new Date("2026-08-12T00:00:00Z"));
    expect(fn).toMatch(/^threadock-settings-2026-08-12\.json$/);
  });
});

describe("KnowledgeModal 跨会话引用", () => {
  const sample = {
    summary: "本次会话完成 X",
    decisions: [],
    todos: [],
    errors: [],
    commands: ["npm run build"],
    files: [{ path: "/src/main.rs" }, { path: "/src/lib.rs" }],
    extractor: "rule-based",
  };

  it("conversationId 存在时显示 🔗 跨会话引用 区块", async () => {
    const { container } = render(
      <KnowledgeModal knowledge={sample} conversationId="c-self" onClose={() => {}} onReextract={() => {}} />,
    );
    await waitFor(() => {
      const xref = container.querySelector(".knowledge-xref");
      expect(xref).toBeInTheDocument();
    });
  });

  it("xref 渲染文件/命令 + 点击其他会话触发 onJumpToConversation", async () => {
    const onJump = vi.fn();
    const { container } = render(
      <KnowledgeModal knowledge={sample} conversationId="c-self" onClose={() => {}} onReextract={() => {}} onJumpToConversation={onJump} />,
    );
    await waitFor(() => {
      expect(container.querySelectorAll(".knowledge-xref-item").length).toBeGreaterThan(0);
    });
    const firstRow = container.querySelector(".xref-conv-row") as HTMLButtonElement;
    fireEvent.click(firstRow);
    expect(onJump).toHaveBeenCalledWith("c-other-1");
  });

  it("无 conversationId 时不显示 xref 区块", () => {
    const { container } = render(
      <KnowledgeModal knowledge={sample} onClose={() => {}} onReextract={() => {}} />,
    );
    expect(container.querySelector(".knowledge-xref")).toBeNull();
  });
});

describe("ConversationList 批量加标签", () => {
  const convs: Conversation[] = [
    { id: "c1", provider: "zcode", source_conversation_id: "sc1", title: "标题1", user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 3600_000, updated_at_ms: Date.now() - 3600_000, child_count: 0, favorite: false, archived: false },
    { id: "c2", provider: "claude-code", source_conversation_id: "sc2", title: "标题2", user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 7200_000, updated_at_ms: Date.now() - 7200_000, child_count: 0, favorite: false, archived: false },
  ];

  it("输入 # 标签 + Enter → 调用 onBulkAddTag 去除 # 前缀", () => {
    const onBulkAddTag = vi.fn(async () => {});
    const { container } = render(
      <ConversationList
        conversations={convs} selectedConv={null} loading={false} providerFilter={null} selectedWs={null}
        expandedParents={new Set()} childConvs={{}} scope="all" onScopeChange={() => {}}
        onFilter={() => {}} onSelect={() => {}} onToggleExpand={() => {}} onClearWs={() => {}}
        onToggleFavorite={() => {}} onBulkAddTag={onBulkAddTag}
      />,
    );
    // 全选
    const checkboxes = container.querySelectorAll(".list-item-check") as NodeListOf<HTMLInputElement>;
    checkboxes.forEach((c) => { if (!c.checked) fireEvent.click(c); });
    // 输入标签 + Enter
    const input = container.querySelector(".bulk-tag-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "# urgent" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onBulkAddTag).toHaveBeenCalledWith(["c1", "c2"], "urgent");
  });

  it("无 onBulkAddTag 时 Enter 不报错（空回调）", () => {
    const { container } = render(
      <ConversationList
        conversations={convs} selectedConv={null} loading={false} providerFilter={null} selectedWs={null}
        expandedParents={new Set()} childConvs={{}} scope="all" onScopeChange={() => {}}
        onFilter={() => {}} onSelect={() => {}} onToggleExpand={() => {}} onClearWs={() => {}}
        onToggleFavorite={() => {}}
      />,
    );
    const checkboxes = container.querySelectorAll(".list-item-check") as NodeListOf<HTMLInputElement>;
    checkboxes.forEach((c) => fireEvent.click(c));
    const input = container.querySelector(".bulk-tag-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "x" } });
    // 不抛错即可
    expect(() => fireEvent.keyDown(input, { key: "Enter" })).not.toThrow();
  });
});
