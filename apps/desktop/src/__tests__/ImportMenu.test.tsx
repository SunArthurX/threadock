// 导入菜单测试：增量入口并入菜单（原顶栏独立按钮已移除）
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ImportMenu from "../ImportMenu";

describe("ImportMenu", () => {
  it("增量同步入口在菜单中并触发 onSync", () => {
    const onToggle = vi.fn();
    const onSync = vi.fn();
    render(<ImportMenu open onToggle={onToggle} onSync={onSync} onSelect={vi.fn()} />);
    fireEvent.click(screen.getByText("增量同步"));
    expect(onSync).toHaveBeenCalledTimes(1);
    expect(onToggle).toHaveBeenCalled(); // 触发后收起菜单
  });

  it("同步进行中入口禁用", () => {
    const { container } = render(<ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} syncing />);
    expect(container.querySelector("button.import-sync-item")).toBeDisabled();
  });

  it("来源与文件导入入口保留", () => {
    const onSelect = vi.fn();
    render(<ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={onSelect} />);
    fireEvent.click(screen.getByText(/从文件导入/));
    expect(onSelect).toHaveBeenCalledWith("file");
    fireEvent.click(screen.getByText(/从 ZCode 导入/));
    expect(onSelect).toHaveBeenCalledWith("zcode");
  });

  it("关闭状态不渲染菜单", () => {
    render(<ImportMenu open={false} onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} />);
    expect(screen.queryByText(/从文件导入/)).toBeNull();
  });
});

describe("ImportMenu 红点与顺序（治理优化）", () => {
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

  it("从文件导入在菜单最后一项（增量同步最先）", () => {
    render(<ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} />);
    const labels = screen.getAllByRole("button").filter((b) => b.closest(".import-menu")).map((b) => b.textContent ?? "");
    const syncIdx = labels.findIndex((t) => t.includes("增量同步"));
    const lastSourceIdx = labels.findIndex((t) => t.includes("从 Codex 导入"));
    const fileIdx = labels.findIndex((t) => t.includes("从文件导入"));
    expect(syncIdx).toBe(0);
    expect(fileIdx).toBe(labels.length - 1);
    expect(fileIdx).toBeGreaterThan(lastSourceIdx);
  });

  it("来源项显示未导入计数副标题", () => {
    render(
      <ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()}
        newCount={{ total: 2, zcode: 2 }} />
    );
    expect(screen.getByText("2 条未导入")).toBeTruthy();
    expect(screen.getAllByText("已全部导入").length).toBeGreaterThanOrEqual(1);
  });
});
