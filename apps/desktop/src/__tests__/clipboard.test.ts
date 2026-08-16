// Round 25 续做按钮修复：clipboard 兜底策略（macOS WKWebView NotAllowedError 兼容）
// Round 25c 续：Tauri IPC invoke("write_clipboard") 作为第一优先级（最稳）
// Round 25d 续：失败时不降级到 navigator（避免 WebView 二次拦截），暴露真实错误
import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";

// Mock Tauri invoke（默认成功）
let invokeMock: ReturnType<typeof vi.fn>;
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// vi.mock 必须在 import 之前
import { copyToClipboard } from "../clipboard";

describe("copyToClipboard：Tauri IPC 优先 + 失败暴露真实错误", () => {
  let originalClipboard: PropertyDescriptor | undefined;
  let originalExecCommand: typeof document.execCommand;
  let hasTauri: boolean;

  beforeEach(() => {
    invokeMock = vi.fn().mockResolvedValue(undefined);
    originalExecCommand = document.execCommand;
    hasTauri = true; // 默认 Tauri 环境
    if (hasTauri) {
      (window as any).__TAURI_INTERNALS__ = { invoke: invokeMock };
    }
  });
  afterEach(() => {
    if (originalClipboard) {
      Object.defineProperty(navigator, "clipboard", originalClipboard);
    } else {
      // @ts-ignore
      delete (navigator as any).clipboard;
    }
    document.execCommand = originalExecCommand;
    // @ts-ignore
    delete (window as any).__TAURI_INTERNALS__;
  });

  it("Tauri invoke 成功：返回 ok=true，不调 navigator", async () => {
    const webWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    const r = await copyToClipboard("tauri win");
    expect(r.ok).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("write_clipboard", { text: "tauri win" });
    // navigator 不应被调（Tauri 路径成功时不降级）
    expect(webWrite).not.toHaveBeenCalled();
  });

  it("Tauri invoke 失败时**不**降级到 navigator，直接返回错误", async () => {
    invokeMock.mockRejectedValue("command not registered");
    const webWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    const r = await copyToClipboard("fail");
    expect(r.ok).toBe(false);
    expect(r.error).toContain("Tauri write_clipboard");
    expect(r.error).toContain("command not registered");
    // 关键：navigator 不应被调（避免 macOS WKWebView 二次拦截）
    expect(webWrite).not.toHaveBeenCalled();
  });

  it("非 Tauri 环境 + navigator.clipboard 成功", async () => {
    // @ts-ignore
    delete (window as any).__TAURI_INTERNALS__;
    const webWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    const r = await copyToClipboard("plain web");
    expect(r.ok).toBe(true);
    expect(webWrite).toHaveBeenCalledWith("plain web");
  });

  it("非 Tauri 环境 + navigator 失败 → 降级到 execCommand", async () => {
    // @ts-ignore
    delete (window as any).__TAURI_INTERNALS__;
    const webWrite = vi.fn().mockRejectedValue(new DOMException("NotAllowedError"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    document.execCommand = vi.fn().mockReturnValue(true);
    const r = await copyToClipboard("legacy");
    // 暴露真实状态让失败时报错包含 error
    expect(r).toEqual({ ok: true, error: undefined });
    expect(document.execCommand).toHaveBeenCalledWith("copy");
  });

  it("全失败：返回 ok=false 带 error", async () => {
    // @ts-ignore
    delete (window as any).__TAURI_INTERNALS__;
    const webWrite = vi.fn().mockRejectedValue(new DOMException("NotAllowedError"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: webWrite },
    });
    document.execCommand = vi.fn().mockReturnValue(false);
    const r = await copyToClipboard("all fail");
    expect(r.ok).toBe(false);
    expect(r.error).toBeDefined();
  });
});
