// 搜索结果保留回归测试：搜索「西游记」后点击左栏搜索结果跳进会话，
// 结果列表必须保留（修复前 jumpToSearchResult 会 setSearchResults(null) 清空列表）；
// 同时验证 Esc / 清除按钮仍可正常退出搜索模式。
import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { APP_VERSION } from "../SettingsView";
import type { Conversation, ConversationDetailDto, SearchResult } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null), save: vi.fn(async () => null) }));

import App from "../App";
import { invoke } from "@tauri-apps/api/core";

const conv = (id: string, title: string): Conversation => ({
  id, provider: "zcode", source_conversation_id: `src-${id}`,
  title, user_title: null, status: "completed", model: null,
  completeness_score: null, workspace_id: null,
  started_at_ms: 1_700_000_000_000, updated_at_ms: 1_700_000_000_000,
  source_parent_id: null, child_count: 0, favorite: false, archived: false,
});

const convA = conv("conv-a", "聊聊西游记");
const convB = conv("conv-b", "西游记读后感");

const results: SearchResult[] = [
  { message_id: "m-a", conversation_id: "conv-a", provider: "zcode", role: "user", title: "聊聊西游记", snippet: "给我讲讲<b>西游记</b>的故事" },
  { message_id: "m-b", conversation_id: "conv-b", provider: "zcode", role: "assistant", title: "西游记读后感", snippet: "读完<b>西游记</b>之后" },
];

const detail = (c: Conversation, mid: string): ConversationDetailDto => ({
  conversation: c,
  messages: [{ id: mid, role: "user", content_text: "孙悟空三打白骨精", sequence_number: 1, created_at_ms: 1_700_000_000_000 }],
  events: [],
  completeness_label: "完整",
  tags: [],
});

describe("搜索结果点击跳转后保留结果列表", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // 压掉启动弹窗（新手引导 / 更新日志），避免遮罩干扰
    localStorage.setItem("ch-onboarding-seen", "1");
    localStorage.setItem("ch-last-seen-version", APP_VERSION);
    localStorage.setItem("ch-view", "chat");
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      switch (cmd) {
        case "list_conversations": return [convA, convB];
        case "search": return results;
        case "get_conversation_detail": {
          const id = (args as { conversationId?: string } | undefined)?.conversationId;
          return detail(id === "conv-b" ? convB : convA, id === "conv-b" ? "m-b" : "m-a");
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

  it("点击搜索结果跳进会话后，左栏仍展示「西游记」结果，且详情已加载", async () => {
    const { container } = render(<App />);

    // 输入关键词并回车搜索
    const input = container.querySelector<HTMLInputElement>(".search-box input")!;
    fireEvent.change(input, { target: { value: "西游记" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(container.querySelectorAll(".search-result").length).toBe(2));

    // 点击第一条搜索结果 → 跳进会话详情
    fireEvent.click(container.querySelector(".search-result")!);

    // 详情加载完成（消息内容出现）
    await waitFor(() => expect(container.textContent).toContain("孙悟空三打白骨精"));

    // 核心断言：搜索结果列表依然保留，不因点击跳转而被重置
    expect(container.querySelectorAll(".search-result").length).toBe(2);
    expect(container.textContent).toContain("西游记");

    // Esc 仍可正常退出搜索模式，回到普通会话列表
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(container.querySelectorAll(".search-result").length).toBe(0));
    expect(container.textContent).toContain("聊聊西游记");
  });

  it("顶栏「清除」按钮仍可退出搜索模式并清空关键词", async () => {
    const { container } = render(<App />);
    const input = container.querySelector<HTMLInputElement>(".search-box input")!;
    fireEvent.change(input, { target: { value: "西游记" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(container.querySelectorAll(".search-result").length).toBe(2));

    const clearBtn = [...container.querySelectorAll("button")].find((b) => b.textContent === "清除")!;
    fireEvent.click(clearBtn);

    await waitFor(() => expect(container.querySelectorAll(".search-result").length).toBe(0));
    expect(input.value).toBe("");
  });
});
