// 复制到剪贴板（兼容 macOS WKWebView 的 NotAllowedError）。
//
// 优先级：
// 1) @tauri-apps/plugin-clipboard-manager（Rust 进程写剪贴板，最稳）
// 2) navigator.clipboard.writeText（Chrome / Edge / 正常 webview）
// 3) document.execCommand('copy') + 临时 textarea（macOS WKWebView 兜底）
// 4) 失败：返回 false，调用方弹 toast 让用户手动 Cmd+C
//
// 修 round 25 续做按钮报错：navigator.clipboard.writeText 在 WKWebView
// 报 "NotAllowedError"——Tauri clipboard 插件走 Rust 进程，不受 WebView
// clipboard 权限限制，是 macOS 上最稳的方案。
export async function copyToClipboard(text: string): Promise<boolean> {
  // 1) 优先：Tauri 原生 clipboard 插件（Rust 进程，绕开 WebView 权限）
  try {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
    return true;
  } catch {
    // 插件不可用（非 Tauri 环境 / 权限未开）→ 降级
  }
  // 2) 降级：现代 Web API
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // NotAllowedError 等：再降级
    }
  }
  // 3) 最后兜底：临时 textarea + execCommand
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    // 避免滚动条 / 焦点跳转：放屏幕外、不可见
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
    return ok;
  } catch {
    return false;
  }
}
