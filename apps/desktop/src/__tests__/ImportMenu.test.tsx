// 导入菜单测试：单 IDE 入口已下线（统一走「立即同步全部」+「从文件导入」）
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ImportMenu from "../ImportMenu";

describe("ImportMenu", () => {
  it("立即同步全部入口在菜单中并触发 onSync", () => {
    const onToggle = vi.fn();
    const onSync = vi.fn();
    render(<ImportMenu open onToggle={onToggle} onSync={onSync} onSelect={vi.fn()} />);
    fireEvent.click(screen.getByText("立即同步全部"));
    expect(onSync).toHaveBeenCalledTimes(1);
    expect(onToggle).toHaveBeenCalled(); // 触发后收起菜单
  });

  it("同步进行中入口禁用", () => {
    const { container } = render(<ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} syncing />);
    expect(container.querySelector("button.import-sync-item")).toBeDisabled();
  });

  it("只保留 2 条入口：立即同步全部 + 从文件导入", () => {
    const onSelect = vi.fn();
    render(<ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={onSelect} />);
    // 触发「从文件导入」应能命中 onSelect("file")
    fireEvent.click(screen.getByText("从文件导入"));
    expect(onSelect).toHaveBeenCalledWith("file");
    // 单 IDE 入口应已下线（ZCode / Claude Code / Cursor / MiniMax / Codex）
    expect(screen.queryByText(/从 ZCode 导入/)).toBeNull();
    expect(screen.queryByText(/从 Claude Code 导入/)).toBeNull();
    expect(screen.queryByText(/从 Cursor 导入/)).toBeNull();
    expect(screen.queryByText(/从 MiniMax 导入/)).toBeNull();
    expect(screen.queryByText(/从 Codex 导入/)).toBeNull();
  });

  it("关闭状态不渲染菜单", () => {
    render(<ImportMenu open={false} onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} />);
    expect(screen.queryByText(/从文件导入/)).toBeNull();
  });
});

describe("ImportMenu 红点（治理优化）", () => {
  it("newCount.total > 0 时触发按钮显示红点；为 0 不显示", () => {
    const { rerender, container } = render(
      <ImportMenu open={false} onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} newCount={{ total: 3, zcode: 3 }} />
    );
    expect(container.querySelector(".new-dot")).toBeTruthy();
    rerender(
      <ImportMenu open={false} onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} newCount={{ total: 0 }} />
    );
    expect(container.querySelector(".new-dot")).toBeNull();
  });

  it("newCount > 0 时菜单项「立即同步全部」右侧显示数字徽章", () => {
    render(
      <ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()}
        newCount={{ total: 5, zcode: 5 }} />
    );
    const badge = document.querySelector(".import-item-count");
    expect(badge?.textContent).toBe("5");
  });

  it("从文件导入在菜单最后一项（立即同步全部最先）", () => {
    render(<ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} />);
    const labels = screen.getAllByRole("button").filter((b) => b.closest(".import-menu")).map((b) => b.textContent ?? "");
    const syncIdx = labels.findIndex((t) => t.includes("立即同步全部"));
    const fileIdx = labels.findIndex((t) => t.includes("从文件导入"));
    expect(syncIdx).toBe(0);
    expect(fileIdx).toBe(labels.length - 1);
  });
});
