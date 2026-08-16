// Round 25 续做按钮修复：clipboard 兜底策略（macOS WKWebView NotAllowedError 兼容）
// Round 25b 续：Tauri 原生 clipboard 插件作为第一优先级（最稳）
import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { copyToClipboard } from "../clipboard";

// Mock Tauri clipboard 插件（默认成功）
let tauriWriteText: ReturnType<typeof vi.fn>;
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: (...args: unknown[]) => tauriWriteText(...args),
}));

describe("copyToClipboard：macOS WKWebView NotAllowedError 兼容", () => {
  let originalClipboard: PropertyDescriptor | undefined;
  let originalExecCommand: typeof document.execCommand;

  beforeEach(() => {
    originalExecCommand = document.execCommand;
    // 默认 Tauri 插件成功；各 case 可覆盖
    tauriWriteText = vi.fn().mockResolvedValue(undefined);
  });
  afterEach(() => {
    if (originalClipboard) {
      Object.defineProperty(navigator, "clipboard", originalClipboard);
    } else {
      // @ts-ignore
      delete (navigator as any).clipboard;
    }
    document.execCommand = originalExecCommand;
  });

  it("Tauri clipboard 插件成功时直接返回 true，不调 navigator", async () => {
    const webWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    const ok = await copyToClipboard("tauri win");
    expect(ok).toBe(true);
    expect(tauriWriteText).toHaveBeenCalledWith("tauri win");
    // navigator.clipboard 不应被调（Tauri 插件是首选）
    expect(webWrite).not.toHaveBeenCalled();
  });

  it("Tauri 插件不可用（非 Tauri 环境）→ 降级到 navigator.clipboard", async () => {
    tauriWriteText = vi.fn().mockRejectedValue(new Error("not in Tauri"));
    const webWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    const ok = await copyToClipboard("web fallback");
    expect(ok).toBe(true);
    expect(webWrite).toHaveBeenCalledWith("web fallback");
  });

  it("Tauri + navigator 都失败时降级到 execCommand 仍成功", async () => {
    tauriWriteText = vi.fn().mockRejectedValue(new Error("not in Tauri"));
    const webWrite = vi.fn().mockRejectedValue(new DOMException("NotAllowedError"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    document.execCommand = vi.fn().mockReturnValue(true);
    const ok = await copyToClipboard("legacy win");
    expect(ok).toBe(true);
    expect(document.execCommand).toHaveBeenCalledWith("copy");
  });

  it("三者都失败时返回 false（让调用方弹 toast）", async () => {
    tauriWriteText = vi.fn().mockRejectedValue(new Error("not in Tauri"));
    const webWrite = vi.fn().mockRejectedValue(new DOMException("NotAllowedError"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    document.execCommand = vi.fn().mockReturnValue(false);
    const ok = await copyToClipboard("all fail");
    expect(ok).toBe(false);
  });

  it("只有 navigator.clipboard 路径（无 Tauri）成功", async () => {
    tauriWriteText = vi.fn().mockRejectedValue(new Error("not in Tauri"));
    const webWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    const ok = await copyToClipboard("plain web");
    expect(ok).toBe(true);
    expect(webWrite).toHaveBeenCalledWith("plain web");
  });
});
