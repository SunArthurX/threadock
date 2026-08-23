// en 模式冒烟：语言切换后关键界面真实渲染英文
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { setLang, getLang } from "../i18n";
import HelpShortcuts from "../HelpShortcuts";

// Tauri IPC mock（jsdom 无原生 bridge；SettingsView 需要）
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "reset_range") return { conversations: 0, messages: 0 };
    if (cmd === "governance_log_list") return [];
    if (cmd === "app_setting_get") return null;
    if (cmd === "llm_config_get")
      return {
        enabled: false, base_url: "", model: "", timeout_secs: 60, max_input_chars: 48000,
        has_api_key: false, api_key_masked: null, is_local: false, api_key_broken: false,
      };
    return {};
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

describe("en 渲染冒烟", () => {
  beforeEach(() => setLang("zh"));

  it("切换 en 后 HelpShortcuts 渲染英文标题", () => {
    setLang("en");
    expect(getLang()).toBe("en");
    render(<HelpShortcuts onClose={() => {}} />);
    expect(screen.getByText("Shortcut cheatsheet")).toBeTruthy();
    setLang("zh");
  });

  it("zh 模式恢复中文", () => {
    render(<HelpShortcuts onClose={() => {}} />);
    expect(screen.getByText("快捷键速查")).toBeTruthy();
  });
});
