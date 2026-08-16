// 复制到剪贴板（兼容 macOS WKWebView 的 NotAllowedError）。
//
// 优先级：
// 1) navigator.clipboard.writeText（Chrome / Edge / 正常 webview）
// 2) document.execCommand('copy') + 临时 textarea（macOS WKWebView 经常拒绝前者）
// 3) 失败：返回 false，调用方弹 toast 让用户手动 Cmd+C
//
// 修 round 25 续做按钮报错：navigator.clipboard.writeText 在 WKWebView
// 报 "NotAllowedError: The request is not allowed by the user agent or
// the platform in the current context, possibly because the user denied
// permission"——切到 execCommand 兜底后稳定。
export async function copyToClipboard(text: string): Promise<boolean> {
  // 1) 优先：现代 API
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // NotAllowedError 等：降级
    }
  }
  // 2) 兜底：临时 textarea + execCommand
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
