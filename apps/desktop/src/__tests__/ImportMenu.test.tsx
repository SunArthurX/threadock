// 导入菜单测试：增量入口并入菜单（原顶栏独立按钮已移除）
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ImportMenu from "../ImportMenu";

describe("ImportMenu", () => {
  it("增量同步入口在菜单中并触发 onSync", () => {
    const onToggle = vi.fn();
    const onSync = vi.fn();
    render(<ImportMenu open onToggle={onToggle} onSync={onSync} onSelect={vi.fn()} />);
    fireEvent.click(screen.getByText("⇩ 增量同步（全部来源）"));
    expect(onSync).toHaveBeenCalledTimes(1);
    expect(onToggle).toHaveBeenCalled(); // 触发后收起菜单
  });

  it("同步进行中入口禁用", () => {
    render(<ImportMenu open onToggle={vi.fn()} onSync={vi.fn()} onSelect={vi.fn()} syncing />);
    expect(screen.getByText("⟳ 同步中…")).toBeDisabled();
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
