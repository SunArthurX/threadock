// 搜索模式行为回归测试（按主对话分组 + 右栏命中步进）：
// 1. 搜索「西游记」后左栏显示主对话分组（子对话命中折叠其下），点击分组行跳转后分组保留；
// 2. 右栏步进条出现（N/M 计数），↑/↓ 可跨主对话/子对话跳转命中并自动切换详情；
// 3. Esc / 顶栏「清除」退出搜索模式：分组与步进条消失，回到普通会话列表。
import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { APP_VERSION } from "../SettingsView";
import type { Conversation, ConversationDetailDto, SearchHitGroup, SearchResult } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null), save: vi.fn(async () => null) }));

import App from "../App";
import { invoke } from "@tauri-apps/api/core";

const conv = (id: string, title: string, parentId?: string): Conversation => ({
  id, provider: "zcode", source_conversation_id: `src-${id}`,
  title, user_title: null, status: "completed", model: null,
  completeness_score: null, workspace_id: null,
  started_at_ms: 1_700_000_000_000, updated_at_ms: 1_700_000_000_000,
  source_parent_id: parentId ?? null, child_count: parentId ? 0 : 1, favorite: false, archived: false,
});

const convA = conv("conv-a", "聊聊西游记");
const convC = conv("conv-c", "西游记番外", "src-conv-a");

const groups: SearchHitGroup[] = [
  { root_conversation_id: "conv-a", root_title: "聊聊西游记", root_updated_at_ms: 1_700_000_000_000,
    provider: "zcode", conversation_id: "conv-a", title: "聊聊西游记", is_child: false,
    hit_count: 1, best_message_id: "m-a", best_role: "user", snippet: "给我讲讲<b>西游记</b>" },
  { root_conversation_id: "conv-a", root_title: "聊聊西游记", root_updated_at_ms: 1_700_000_000_000,
    provider: "zcode", conversation_id: "conv-c", title: "西游记番外", is_child: true,
    hit_count: 1, best_message_id: "m-c", best_role: "assistant", snippet: "<b>西游记</b>番外篇" },
];

const treeHits: SearchResult[] = [
  { message_id: "m-a", conversation_id: "conv-a", provider: "zcode", role: "user", title: "聊聊西游记", snippet: "给我讲讲<b>西游记</b>" },
  { message_id: "m-c", conversation_id: "conv-c", provider: "zcode", role: "assistant", title: "西游记番外", snippet: "<b>西游记</b>番外篇" },
];

const detailOf = (c: Conversation, mid: string): ConversationDetailDto => ({
  conversation: c,
  messages: [{ id: mid, role: "user", content_text: `${c.title} 的命中正文`, sequence_number: 1, created_at_ms: 1_700_000_000_000 }],
  events: [],
  completeness_label: "完整",
  tags: [],
});

describe("搜索模式：主对话分组 + 命中步进", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.setItem("ch-onboarding-seen", "1");
    localStorage.setItem("ch-last-seen-version", APP_VERSION);
    localStorage.setItem("ch-view", "chat");
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      switch (cmd) {
        case "list_conversations": return [convA];
        case "search_grouped": return groups;
        case "search_tree_hits": return treeHits;
        case "get_conversation_detail": {
          const id = (args as { conversationId?: string } | undefined)?.conversationId;
          return detailOf(id === "conv-c" ? convC : convA, id === "conv-c" ? "m-c" : "m-a");
        }
        case "get_conversation_note": return null;
        case "list_all_tags": return [];
        case "saved_search_list": return [];
        case "available_providers": return ["zcode"];
        case "sources_new_count": return { total: 0 };
        case "auto_sync": return {};
        default: return null;
      }
    });
  });

  it("点击分组行后左栏仍保留分组，步进条可 ↑/↓ 跨子对话跳转", async () => {
    const { container } = render(<App />);

    // 搜索「西游记」→ 左栏显示主对话分组（1 个 root + 2 行）
    const input = container.querySelector<HTMLInputElement>(".search-box input")!;
    fireEvent.change(input, { target: { value: "西游记" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(container.querySelectorAll(".search-group").length).toBe(1));
    expect(container.querySelectorAll(".search-result").length).toBe(2);
    // 子对话行带「子对话」标记且折叠在主对话 root 下
    expect(container.querySelector(".search-group-row.child, .search-row-kind.child")?.textContent).toContain("子对话");
    expect(container.querySelector(".search-group-root")?.textContent).toContain("聊聊西游记");

    // 点击子对话命中行 → 打开子会话详情 + 步进条（2 处命中，当前第 2 处）
    fireEvent.click(container.querySelectorAll(".search-result")[1]);
    await waitFor(() => expect(container.textContent).toContain("西游记番外 的命中正文"));
    await waitFor(() => expect(container.querySelector(".hit-nav-bar")).toBeTruthy());
    expect(container.querySelector(".hit-nav-count")?.textContent).toContain("2 / 2");
    // 步进条必须钉在滚动区域外（.detail-col 直接子级）：滚动详情内容时 ↑/↓ 不出视野
    expect(container.querySelector(".detail-col > .hit-nav-bar")).toBeTruthy();
    expect(container.querySelector("[data-testid='scroll-area-inner'] .hit-nav-bar")).toBeNull();
    // 左栏分组保留（不被重置）
    expect(container.querySelectorAll(".search-group").length).toBe(1);

    // ↑ 回到主对话命中：详情自动切换、计数变 1/2
    fireEvent.keyDown(window, { key: "ArrowUp" });
    await waitFor(() => expect(container.textContent).toContain("聊聊西游记 的命中正文"));
    expect(container.querySelector(".hit-nav-count")?.textContent).toContain("1 / 2");

    // ↓ 再回到子对话
    fireEvent.keyDown(window, { key: "ArrowDown" });
    await waitFor(() => expect(container.textContent).toContain("西游记番外 的命中正文"));

    // Esc 退出搜索模式：分组与步进条消失，回到普通会话列表
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(container.querySelector(".hit-nav-bar")).toBeNull());
    await waitFor(() => expect(container.querySelectorAll(".search-group").length).toBe(0));
    expect(container.querySelector<HTMLInputElement>(".search-box input")!.value).toBe("");
  });

  it("顶栏「清除」按钮仍可退出搜索模式并清空关键词", async () => {
    const { container } = render(<App />);
    const input = container.querySelector<HTMLInputElement>(".search-box input")!;
    fireEvent.change(input, { target: { value: "西游记" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(container.querySelectorAll(".search-group").length).toBe(1));

    const clearBtn = [...container.querySelectorAll("button")].find((b) => b.textContent === "清除")!;
    fireEvent.click(clearBtn);

    await waitFor(() => expect(container.querySelectorAll(".search-group").length).toBe(0));
    expect(container.querySelector(".hit-nav-bar")).toBeNull();
    expect(input.value).toBe("");
  });
});
