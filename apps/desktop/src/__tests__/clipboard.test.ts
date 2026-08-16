// Round 25 续做按钮修复：clipboard 兜底策略（macOS WKWebView NotAllowedError 兼容）
import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { copyToClipboard } from "../clipboard";

describe("copyToClipboard：macOS WKWebView NotAllowedError 兼容", () => {
  let originalClipboard: PropertyDescriptor | undefined;
  let originalExecCommand: typeof document.execCommand;

  beforeEach(() => {
    originalExecCommand = document.execCommand;
  });
  afterEach(() => {
    // 恢复
    if (originalClipboard) {
      Object.defineProperty(navigator, "clipboard", originalClipboard);
    } else {
      // @ts-ignore
      delete (navigator as any).clipboard;
    }
    document.execCommand = originalExecCommand;
  });

  it("navigator.clipboard.writeText 成功时直接返回 true", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const ok = await copyToClipboard("hello");
    expect(ok).toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("navigator.clipboard.writeText 报 NotAllowedError 时降级到 execCommand 仍成功", async () => {
    // 模拟 macOS WKWebView 拦截
    const writeText = vi.fn().mockRejectedValue(new DOMException("NotAllowedError"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    document.execCommand = vi.fn().mockReturnValue(true);
    const ok = await copyToClipboard("fallback text");
    expect(ok).toBe(true);
    expect(writeText).toHaveBeenCalledWith("fallback text");
    // execCommand 已被调用
    expect(document.execCommand).toHaveBeenCalledWith("copy");
  });

  it("两者都失败时返回 false（让调用方弹 toast）", async () => {
    const writeText = vi.fn().mockRejectedValue(new DOMException("NotAllowedError"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    document.execCommand = vi.fn().mockReturnValue(false);
    const ok = await copyToClipboard("nope");
    expect(ok).toBe(false);
  });

  it("navigator.clipboard 不存在时直接走 execCommand 路径", async () => {
    // @ts-ignore
    delete (navigator as any).clipboard;
    document.execCommand = vi.fn().mockReturnValue(true);
    const ok = await copyToClipboard("legacy");
    expect(ok).toBe(true);
    expect(document.execCommand).toHaveBeenCalledWith("copy");
  });
});
