// 底部终端 dock（v1.3.0，ZCode/Codex 风格）：topbar 右上角 toggle / ⌘J 开关。
// 真实 PTY（后端 portable-pty）+ xterm.js 渲染；输出经 terminal-output 事件
// （base64）回传。xterm 实例懒创建（首次打开），面板关闭仅隐藏（会话与
// 回滚缓冲保留），「重启」kill 重开。组件常驻挂载，holder 元素稳定存在，
// 画布不因开关重建。
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { Icon } from "./Icon";
import Resizer from "./Resizer";

/** base64 → 字节流（后端为规避 UTF-8 分块截断按 b64 回传）。 */
export function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

/** 跟随应用主题的 xterm 配色。 */
export function xtermTheme(dark: boolean): Record<string, string> {
  return dark
    ? { background: "#10131c", foreground: "#d6dcea", cursor: "#e6ebf5", selectionBackground: "#2c3550" }
    : { background: "#ffffff", foreground: "#1d2330", cursor: "#1d2330", selectionBackground: "#cfe0ff" };
}

export default function BottomTerminal({
  open,
  dark,
  height,
  onClose,
  onHeightChange,
}: {
  open: boolean;
  dark: boolean;
  height: number;
  onClose: () => void;
  onHeightChange: (h: number) => void;
}) {
  const holderRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const unlistenRef = useRef<UnlistenFn[]>([]);
  const roRef = useRef<ResizeObserver | null>(null);
  const dataSubRef = useRef<{ dispose(): void } | null>(null);
  const [exited, setExited] = useState(false);

  /** 首次打开时创建 xterm 实例并接线（输出/退出事件、输入、resize）。 */
  const ensureTerm = useCallback(() => {
    if (termRef.current || !holderRef.current) return;
    const term = new Terminal({
      fontSize: 12,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      cursorBlink: true,
      convertEol: false,
      theme: xtermTheme(document.documentElement.dataset.theme === "dark"),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(holderRef.current);
    termRef.current = term;
    fitRef.current = fit;

    void listen<string>("terminal-output", (e) => {
      term.write(b64ToBytes(e.payload));
    }).then((un) => unlistenRef.current.push(un)).catch(() => { /* 监听失败静默 */ });
    void listen("terminal-exit", () => {
      setExited(true);
      term.write("\r\n\x1b[90m[进程已退出 · 点击 ↻ 重启]\x1b[0m\r\n");
    }).then((un) => unlistenRef.current.push(un)).catch(() => { /* 静默 */ });

    dataSubRef.current = term.onData((data) => {
      void invoke("terminal_write", { data }).catch(() => { /* 会话已退出 */ });
    });

    // 容器尺寸变化 → fit → 通知 pty resize（隐藏瞬间 0 尺寸由 fit 内部 try 兜底）
    const ro = new ResizeObserver(() => {
      if (!holderRef.current || !fitRef.current || !termRef.current) return;
      try {
        fitRef.current.fit();
        const { cols, rows } = termRef.current;
        void invoke("terminal_resize", { cols, rows }).catch(() => { /* 非致命 */ });
      } catch { /* fit 在 0 尺寸下抛错，忽略 */ }
    });
    ro.observe(holderRef.current);
    roRef.current = ro;
  }, []);

  // 卸载时统一回收（开关面板不触发）
  useEffect(() => {
    const unlisteners = unlistenRef;
    return () => {
      roRef.current?.disconnect();
      dataSubRef.current?.dispose();
      for (const un of unlisteners.current) un();
      termRef.current?.dispose();
      termRef.current = null;
    };
  }, []);

  // 主题切换：更新 xterm 配色
  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = xtermTheme(dark);
  }, [dark]);

  // 打开时：懒创建 + fit + spawn（复用后端存活会话）
  useEffect(() => {
    if (!open) return;
    ensureTerm();
    try { fitRef.current?.fit(); } catch { /* 0 尺寸忽略 */ }
    void invoke("terminal_spawn", { cols: termRef.current?.cols ?? 80, rows: termRef.current?.rows ?? 24 })
      .then(() => setExited(false))
      .catch(() => { /* 后端不可用时静默 */ });
  }, [open, ensureTerm]);

  const restart = async () => {
    try { await invoke("terminal_kill"); } catch { /* 无会话时忽略 */ }
    termRef.current?.reset();
    setExited(false);
    try { fitRef.current?.fit(); } catch { /* 忽略 */ }
    await invoke("terminal_spawn", { cols: termRef.current?.cols ?? 80, rows: termRef.current?.rows ?? 24 }).catch(() => { /* 静默 */ });
  };

  // 常驻同一个 .bottom-dock 容器（closed 时 display:none）：xterm 画布与
  // 回滚缓冲跨开关保留，重开由 ResizeObserver 触发 refit 恢复排版
  return (
    <div className={`bottom-dock ${open ? "" : "closed"}`} style={{ height }} data-testid="bottom-terminal">
      {open && (
        <Resizer
          axis="y"
          className="bottom-dock-resizer"
          title="拖拽调整终端高度"
          onDrag={(dy) => onHeightChange(Math.max(160, Math.min(720, height - dy)))}
        />
      )}
      {open && (
        <div className="bottom-dock-header">
          <Icon name="terminal" size={12} />
          <span>终端</span>
          {exited && <span className="bottom-dock-exited">已退出</span>}
          <span style={{ flex: 1 }} />
          <button className="action-btn" onClick={() => termRef.current?.clear()} title="清屏">清屏</button>
          <button className="action-btn" onClick={() => void restart()} title="kill 当前 shell 并重新启动">↻ 重启</button>
          <button className="action-btn" onClick={onClose} title="收起面板（⌘J）">✕ 收起</button>
        </div>
      )}
      <div className="bottom-xterm" ref={holderRef} />
    </div>
  );
}
