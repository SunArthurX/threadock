// 复制到剪贴板（兼容 macOS WKWebView 的 NotAllowedError）。
//
// 优先级：
// 1) Tauri IPC：invoke("write_clipboard")（Rust 进程调 arboard，绕开 WebView 权限层）
//    → 失败时不降级到 navigator（macOS WKWebView 总是被拒），直接返回错误
// 2) 非 Tauri 环境：navigator.clipboard.writeText（普通 web 端）
// 3) 兜底：document.execCommand('copy') + 临时 textarea
//
// 修 round 25 续做按钮报错：navigator.clipboard.writeText 在 WKWebView
// 报 "NotAllowedError"——直接 invoke Rust 自定义 command 是最稳路径。
import { invoke } from "@tauri-apps/api/core";

/** 是否在 Tauri 环境（避免在普通 web 端尝试 invoke）。 */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function copyToClipboard(text: string): Promise<{ ok: boolean; error?: string }> {
  // 1) Tauri 环境：invoke Rust arboard command（最稳，不受 WebView 权限限制）
  if (isTauri()) {
    try {
      await invoke("write_clipboard", { text });
      return { ok: true };
    } catch (e) {
      // 不降级：Tauri 端失败说明 plugin/command 没装好，让用户看真实错误
      return { ok: false, error: `Tauri write_clipboard: ${typeof e === "string" ? e : String(e)}` };
    }
  }
  // 2) 非 Tauri：现代 Web API（失败后降级到 execCommand）
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return { ok: true };
    } catch {
      // 降级：navigator 失败，继续走 execCommand
    }
  }
  // 3) 兜底：临时 textarea + execCommand
  // 3) 兜底：临时 textarea + execCommand
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.top = "0";
    ta.style.left = "0";
    ta.style.width = "1px";
    ta.style.height = "1px";
    ta.style.opacity = "0";
    ta.style.pointerEvents = "none";
    document.body.appendChild(ta);
    ta.focus();
    ta.select();
    ta.setSelectionRange(0, text.length);
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return { ok, error: ok ? undefined : "execCommand('copy') returned false" };
  } catch (e) {
    return { ok: false, error: `execCommand path: ${typeof e === "string" ? e : String(e)}` };
  }
}
