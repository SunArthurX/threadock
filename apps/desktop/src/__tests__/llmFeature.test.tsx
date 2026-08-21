// LLM 提取功能测试：知识弹窗引擎切换、工具栏提取按钮参数契约、设置页 LLM 配置区交互。
// 回归背景：
// - 工具栏 onClick 直通事件对象 → MouseEvent 被当成 engine 传给 invoke（invalid args）
// - AI 提取失败（未启用/网络错）后弹窗 switching 状态永不清除 → 按钮永久禁用
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import KnowledgeModal from "../KnowledgeModal";
import ConversationDetail from "../ConversationDetail";
import SettingsView from "../SettingsView";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "llm_config_get")
      return {
        enabled: false, base_url: "", model: "", timeout_secs: 60, max_input_chars: 48000,
        has_api_key: false, api_key_masked: null, is_local: false, api_key_broken: false,
      };
    if (cmd === "llm_config_set")
      return {
        enabled: true, base_url: "http://127.0.0.1:11434/v1", model: "qwen2.5:7b",
        timeout_secs: 60, max_input_chars: 48000,
        has_api_key: true, api_key_masked: "sk-***5678", is_local: true, api_key_broken: false,
      };
    if (cmd === "app_setting_get") return null;
    if (cmd === "governance_log_list") return [];
    return {};
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const knowledge = {
  summary: "s", decisions: [], todos: [], errors: [], commands: [], files: [],
  extractor: "llm:mock-1@prompt-v1",
};

beforeEach(() => {
  vi.mocked(invoke).mockClear();
});

describe("KnowledgeModal 引擎切换", () => {
  it("点 AI 引擎 / 规则 以对应引擎重提取；重新提取用当前引擎", async () => {
    const onReextract = vi.fn(async () => {});
    render(<KnowledgeModal knowledge={knowledge} onClose={() => {}} onReextract={onReextract} />);
    fireEvent.click(screen.getByText("AI 引擎"));
    expect(onReextract).toHaveBeenCalledWith("llm");
    // 提取完成后按钮恢复，再切规则（真实时序：两次点击间隔一次完整请求）
    await waitFor(() => expect(screen.getByText("规则").closest("button")).not.toBeDisabled());
    fireEvent.click(screen.getByText("规则"));
    expect(onReextract).toHaveBeenCalledWith("rule");
    await waitFor(() => expect(screen.getByText("重新提取").closest("button")).not.toBeDisabled());
    fireEvent.click(screen.getByText("重新提取"));
    expect(onReextract).toHaveBeenLastCalledWith("rule"); // 默认引擎 = rule（prop 缺省）
  });

  it("提取失败（onReextract 不更新结果）后按钮恢复可用，不得永久禁用", async () => {
    const onReextract = vi.fn(async () => { /* 模拟失败：内部 catch、不更新 knowledge */ });
    render(<KnowledgeModal knowledge={knowledge} onClose={() => {}} onReextract={onReextract} />);
    fireEvent.click(screen.getByText("AI 引擎"));
    await waitFor(() => {
      const ai = screen.getByText("AI 引擎").closest("button") as HTMLButtonElement;
      expect(ai).not.toBeDisabled();
      const rule = screen.getByText("规则").closest("button") as HTMLButtonElement;
      expect(rule).not.toBeDisabled();
      const re = screen.getByText("重新提取").closest("button") as HTMLButtonElement;
      expect(re).not.toBeDisabled();
    });
  });

  it("llm: 结果显示模型徽标", () => {
    render(<KnowledgeModal knowledge={knowledge} onClose={() => {}} onReextract={() => {}} />);
    expect(screen.getByText("mock-1")).toBeTruthy();
  });
});

describe("详情页 知识 按钮", () => {
  const conv = {
    id: "c1", provider: "zcode", source_conversation_id: "s", title: "标题", user_title: null,
    status: null, model: null, completeness_score: null, workspace_id: null,
    started_at_ms: null, updated_at_ms: null, source_parent_id: null, child_count: 0,
    favorite: false, archived: false,
  };
  const messages = [
    { id: "m1", role: "user", content_text: "问题", sequence_number: 1, created_at_ms: 1 },
  ];

  it("点击知识按钮不得把事件对象作为参数传给提取回调（engine 参数契约）", () => {
    const onExtractKnowledge = vi.fn();
    render(
      <ConversationDetail
        {...{
          conv, messages, events: [], completenessLabel: "",
          loading: false, exporting: false, timelineMode: false, highlightMsgId: null,
          collapsedMsgs: new Set<string>(), tags: [],
          onToggleTimeline: vi.fn(), onExport: vi.fn(), onExtractKnowledge,
          onToggleCollapse: vi.fn(), onToggleArchive: vi.fn(),
          onAddTag: vi.fn(), onRemoveTag: vi.fn(), onRescanAudit: vi.fn(),
        }}
      />,
    );
    fireEvent.click(screen.getByText("知识"));
    // 必须无参调用：onClick 直通会把 SyntheticEvent 传成 engine → invoke invalid args
    expect(onExtractKnowledge).toHaveBeenCalledTimes(1);
    expect(onExtractKnowledge).toHaveBeenCalledWith();
  });
});

describe("设置页 AI 提取（大模型）配置区", () => {
  const base = {
    theme: "dark" as const, onThemeChange: vi.fn(),
    textSize: "sm" as const, onTextSizeChange: vi.fn(),
    syncIntervalMin: 10, onSyncIntervalChange: vi.fn(),
    retentionDays: 0, onRetentionDaysChange: vi.fn(),
    notifyOnExceed: false, onNotifyOnExceedChange: vi.fn(),
    numberFormat: "raw" as const, onNumberFormatChange: vi.fn(),
    currency: "USD" as const, onCurrencyChange: vi.fn(),
    dateFormat: "relative" as const, onDateFormatChange: vi.fn(),
    onNavigate: vi.fn(), onReset: vi.fn(async () => {}), resetting: false,
    onClose: vi.fn(), onShowChangelog: vi.fn(),
  };

  it("预设一键填入端点与模型", async () => {
    render(<SettingsView {...base} />);
    fireEvent.click(await screen.findByText("Ollama 本地"));
    const baseUrl = screen.getByPlaceholderText(/api\.openai\.com/) as HTMLInputElement;
    expect(baseUrl.value).toBe("http://127.0.0.1:11434/v1");
    const model = screen.getByPlaceholderText(/qwen2\.5/) as HTMLInputElement;
    expect(model.value).toBe("qwen2.5:7b");
  });

  it("保存配置提交密封请求，成功后显示 masked 回显", async () => {
    render(<SettingsView {...base} />);
    fireEvent.click(await screen.findByText("Ollama 本地"));
    const keyInput = screen.getByPlaceholderText(/本地推理可留空/) as HTMLInputElement;
    fireEvent.change(keyInput, { target: { value: "  sk-journey-key-123456  " } });
    fireEvent.click(screen.getByText("💾 保存配置"));
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("llm_config_set", {
        input: {
          enabled: false,
          base_url: "http://127.0.0.1:11434/v1",
          model: "qwen2.5:7b",
          api_key: "sk-journey-key-123456", // 提交前 trim
          clear_api_key: false,
        },
      });
    });
    // 保存响应带 masked → 密码框 placeholder 提示已存储形态
    await waitFor(() => {
      const ph = (screen.getByPlaceholderText(/已存储/) as HTMLInputElement).placeholder;
      expect(ph).toContain("sk-***5678");
    });
  });

  it("清除密钥提交 clear_api_key", async () => {
    // 先保存一次让 meta.has_api_key = true，出现「清除密钥」按钮
    render(<SettingsView {...base} />);
    fireEvent.click(await screen.findByText("Ollama 本地"));
    fireEvent.click(screen.getByText("💾 保存配置"));
    const clearBtn = await screen.findByText("🗑 清除密钥");
    fireEvent.click(clearBtn);
    await waitFor(() => {
      const calls = vi.mocked(invoke).mock.calls.filter(([c]) => c === "llm_config_set");
      const last = calls[calls.length - 1]?.[1] as { input: { clear_api_key: boolean } };
      expect(last.input.clear_api_key).toBe(true);
    });
  });
});
