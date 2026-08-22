// 底部终端 dock：懒创建 / 输出回传(base64 解码) / 输入写入 / 重启 / 高度拖拽
import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import BottomTerminal, { b64ToBytes, xtermTheme } from "../BottomTerminal";

// xterm / fit-addon 全量 mock（jsdom 无真实渲染栈）
const termInstances: {
  open: ReturnType<typeof vi.fn>;
  write: ReturnType<typeof vi.fn>;
  reset: ReturnType<typeof vi.fn>;
  clear: ReturnType<typeof vi.fn>;
  dispose: ReturnType<typeof vi.fn>;
  onData: ReturnType<typeof vi.fn>;
  loadAddon: ReturnType<typeof vi.fn>;
  cols: number;
  rows: number;
  options: { theme?: Record<string, string> };
}[] = [];

vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn().mockImplementation(() => {
    const t = {
      open: vi.fn(),
      write: vi.fn(),
      reset: vi.fn(),
      clear: vi.fn(),
      dispose: vi.fn(),
      onData: vi.fn(() => ({ dispose: vi.fn() })),
      loadAddon: vi.fn(),
      cols: 80,
      rows: 24,
      options: {} as { theme?: Record<string, string> },
    };
    termInstances.push(t);
    return t;
  }),
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn().mockImplementation(() => ({ fit: vi.fn() })),
}));

// invoke / listen mock：事件处理器按名捕获
const handlers: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers[name] = cb;
    return () => {};
  }),
}));

import { invoke } from "@tauri-apps/api/core";

describe("纯函数", () => {
  it("b64ToBytes 解码输出字节流", () => {
    expect(Array.from(b64ToBytes("aGk="))).toEqual([104, 105]); // "hi"
  });
  it("xtermTheme 按明暗给配色", () => {
    expect(xtermTheme(true).background).toBe("#10131c");
    expect(xtermTheme(false).background).toBe("#ffffff");
  });
});

describe("BottomTerminal 组件", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    termInstances.length = 0;
    for (const k of Object.keys(handlers)) delete handlers[k];
  });

  it("关闭时不创建终端、不 spawn；dock 隐藏", () => {
    render(<BottomTerminal open={false} dark={false} height={300} onClose={() => {}} onHeightChange={() => {}} />);
    expect(termInstances).toHaveLength(0);
    const dock = screen.getByTestId("bottom-terminal");
    expect(dock.className).toContain("closed");
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("terminal_spawn", expect.anything());
  });

  it("打开时懒创建 + spawn（复用后端会话）", async () => {
    render(<BottomTerminal open dark={false} height={300} onClose={() => {}} onHeightChange={() => {}} />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("terminal_spawn", { cols: 80, rows: 24 }));
    expect(termInstances).toHaveLength(1);
    expect(termInstances[0].open).toHaveBeenCalled();
  });

  it("terminal-output 事件 → b64 解码后写入 xterm", async () => {
    render(<BottomTerminal open dark={false} height={300} onClose={() => {}} onHeightChange={() => {}} />);
    await waitFor(() => expect(handlers["terminal-output"]).toBeTruthy());
    handlers["terminal-output"]({ payload: "aGk=" });
    expect(termInstances[0].write).toHaveBeenCalled();
    const arg = termInstances[0].write.mock.calls[0][0] as Uint8Array;
    expect(Array.from(arg)).toEqual([104, 105]);
  });

  it("xterm onData → terminal_write", async () => {
    render(<BottomTerminal open dark={false} height={300} onClose={() => {}} onHeightChange={() => {}} />);
    await waitFor(() => expect(termInstances[0]?.onData).toHaveBeenCalled());
    const cb = termInstances[0].onData.mock.calls[0][0] as (d: string) => void;
    cb("ls\r");
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("terminal_write", { data: "ls\r" });
  });

  it("进程退出事件 → 已退出标记；重启 = kill + reset + spawn", async () => {
    const { container } = render(<BottomTerminal open dark={false} height={300} onClose={() => {}} onHeightChange={() => {}} />);
    await waitFor(() => expect(handlers["terminal-exit"]).toBeTruthy());
    handlers["terminal-exit"]({ payload: null });
    await waitFor(() => expect(container.textContent).toContain("已退出"));

    fireEvent.click(screen.getByTitle("kill 当前 shell 并重新启动"));
    await waitFor(() => {
      const cmds = vi.mocked(invoke).mock.calls.map((c) => c[0]);
      const killAt = cmds.indexOf("terminal_kill");
      const spawnAt = cmds.lastIndexOf("terminal_spawn");
      expect(killAt).toBeGreaterThan(-1);
      expect(spawnAt).toBeGreaterThan(killAt);
    });
    expect(termInstances[0].reset).toHaveBeenCalled();
  });

  it("顶边拖拽条 axis=y：向下拖 → onHeightChange(height - dy)", async () => {
    const onHeightChange = vi.fn();
    const { container } = render(<BottomTerminal open dark={false} height={300} onClose={() => {}} onHeightChange={onHeightChange} />);
    const bar = container.querySelector(".bottom-dock-resizer") as HTMLElement;
    expect(bar).toBeTruthy();
    fireEvent.mouseDown(bar, { clientY: 500 });
    fireEvent.mouseMove(document, { clientY: 540 }); // dy = +40（向下）→ 高度 -40
    fireEvent.mouseUp(document);
    expect(onHeightChange).toHaveBeenCalledWith(260);
  });
});
