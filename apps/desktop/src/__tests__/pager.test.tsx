// 分页 hook 与安全页跳转测试
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { usePager } from "../usePager";

describe("usePager（安全/资产分页）", () => {
  it("不足一页不启用", () => {
    const { result } = renderHook(() => usePager([1, 2, 3], 20));
    expect(result.current.needed).toBe(false);
    expect(result.current.slice).toEqual([1, 2, 3]);
    expect(result.current.totalPages).toBe(1);
  });

  it("45 条 / 每页 20 → 3 页，翻页切片正确", () => {
    const items = Array.from({ length: 45 }, (_, i) => i);
    const { result } = renderHook(() => usePager(items, 20));
    expect(result.current.needed).toBe(true);
    expect(result.current.totalPages).toBe(3);
    expect(result.current.slice).toEqual(Array.from({ length: 20 }, (_, i) => i));
    act(() => result.current.next());
    expect(result.current.slice[0]).toBe(20);
    act(() => result.current.next());
    expect(result.current.slice).toEqual([40, 41, 42, 43, 44]); // 末页只剩 5 条
    // 边界：末页 next 不越界
    act(() => result.current.next());
    expect(result.current.page).toBe(2);
    act(() => result.current.prev());
    act(() => result.current.prev());
    act(() => result.current.prev());
    expect(result.current.page).toBe(0);
  });

  it("reset() 强制回到首页", () => {
    const items = Array.from({ length: 60 }, (_, i) => i);
    const { result } = renderHook(() => usePager(items, 20));
    act(() => result.current.next());
    act(() => result.current.next());
    expect(result.current.page).toBe(2);
    act(() => result.current.reset());
    expect(result.current.page).toBe(0);
    expect(result.current.slice[0]).toBe(0);
  });

  it("数据缩短到 1 页时（搜索后清空）useEffect 自动回首页", () => {
    const long = Array.from({ length: 100 }, (_, i) => i);
    const short = [1, 2, 3];
    const { result, rerender } = renderHook(({ data }) => usePager(data, 20), {
      initialProps: { data: long },
    });
    act(() => result.current.next());
    act(() => result.current.next());
    expect(result.current.page).toBe(2);
    // 模拟搜索后数据变少
    rerender({ data: short });
    expect(result.current.page).toBe(0);
    expect(result.current.totalPages).toBe(1);
    expect(result.current.needed).toBe(false);
  });
});
