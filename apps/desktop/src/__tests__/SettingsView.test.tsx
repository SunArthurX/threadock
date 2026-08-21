// 设置面板测试：主题切换 / 同步间隔 / 重置输入确认（防误触）
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// Tauri IPC mock（jsdom 无原生 bridge）
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "reset_range") return { conversations: 2, messages: 10 };
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
import SettingsView, { RESET_CONFIRM_TEXT } from "../SettingsView";

const base = {
  theme: "dark" as const,
  onThemeChange: vi.fn(),
  textSize: "sm" as const,
  onTextSizeChange: vi.fn(),
  syncIntervalMin: 10,
  onSyncIntervalChange: vi.fn(),
  retentionDays: 0,
  onRetentionDaysChange: vi.fn(),
  notifyOnExceed: false,
  onNotifyOnExceedChange: vi.fn(),
  numberFormat: "raw" as const,
  onNumberFormatChange: vi.fn(),
  currency: "USD" as const,
  onCurrencyChange: vi.fn(),
  dateFormat: "relative" as const,
  onDateFormatChange: vi.fn(),
  onNavigate: vi.fn(),
  onReset: vi.fn(async () => {}),
  resetting: false,
  onClose: vi.fn(),
  onShowChangelog: vi.fn(),
};

const openSettings = () => render(<SettingsView {...base} />);

describe("SettingsView 外观与同步", () => {
  it("主题切换按钮触发回调", () => {
    openSettings();
    fireEvent.click(screen.getByText("浅色"));
    expect(base.onThemeChange).toHaveBeenCalledWith("light");
  });

  it("同步间隔选择触发回调（含关闭）", () => {
    openSettings();
    const select = screen.getByDisplayValue("每 10 分钟") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "30" } });
    expect(base.onSyncIntervalChange).toHaveBeenCalledWith(30);
    fireEvent.change(select, { target: { value: "0" } });
    expect(base.onSyncIntervalChange).toHaveBeenCalledWith(0);
  });

  it("治理导航按钮跳转并关闭弹窗", () => {
    openSettings();
    fireEvent.click(screen.getByText("前往 安全 页 →"));
    expect(base.onNavigate).toHaveBeenCalledWith("security");
    expect(base.onClose).toHaveBeenCalled();
  });
});

describe("SettingsView 重置确认（防误触）", () => {
  it("确认词不匹配时按钮禁用", () => {
    openSettings();
    const btn = screen.getByText(/重置.*之后的数据/) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    const input = screen.getByPlaceholderText(`请输入 ${RESET_CONFIRM_TEXT}`);
    fireEvent.change(input, { target: { value: "reset" } });
    expect(screen.getByText(/重置.*之后的数据/)).toBeDisabled();
    fireEvent.change(input, { target: { value: "重 置" } });
    expect(screen.getByText(/重置.*之后的数据/)).toBeDisabled();
  });

  it("输入「重置」后按钮可用，点击执行并清空输入", async () => {
    openSettings();
    const input = screen.getByPlaceholderText(`请输入 ${RESET_CONFIRM_TEXT}`);
    fireEvent.change(input, { target: { value: RESET_CONFIRM_TEXT } });
    const btn = screen.getByText(/重置.*之后的数据/) as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    fireEvent.click(btn);
    await waitFor(() => expect(base.onReset).toHaveBeenCalledTimes(1));
    // 执行后清空确认词（再次误点不会重复触发）
    expect((screen.getByPlaceholderText(`请输入 ${RESET_CONFIRM_TEXT}`) as HTMLInputElement).value).toBe("");
  });

  it("未选择日期时按钮禁用", () => {
    const { container } = render(<SettingsView {...base} />);
    const dateInput = container.querySelector('input[type="date"]') as HTMLInputElement;
    fireEvent.change(dateInput, { target: { value: "" } });
    const input = screen.getByPlaceholderText(`请输入 ${RESET_CONFIRM_TEXT}`);
    fireEvent.change(input, { target: { value: RESET_CONFIRM_TEXT } });
    const btns = screen.getAllByText(/重置/);
    const resetBtn = btns.find((b) => (b as HTMLButtonElement).disabled !== undefined) as HTMLButtonElement;
    expect(resetBtn).toBeTruthy();
    expect(resetBtn.disabled).toBe(true);
  });
});
