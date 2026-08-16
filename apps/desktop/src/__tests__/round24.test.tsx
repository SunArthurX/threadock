// 第 24 轮测试：按时间重置 — 用户自选任意开始日期 + 移除 31 天硬限制
// - bounds 来自后端 `reset_range_bounds` command（库中最早数据时间戳）
// - 库为空时 fallback 到今天
// - 后端命令失败时也降级（不让用户卡死）
// - `fetchResetDateBounds` 校验 earliest_ms=0 → today（空库约定）
// - `resetDateBoundsSync` 始终 earliest=today（兜底）
import { describe, expect, it, beforeEach, vi, afterEach } from "vitest";

// 用 vi.mock 在文件顶部（ESM module frozen 必须在 import 之前）
let invokeMock: ReturnType<typeof vi.fn>;
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// 注意：import 必须在 vi.mock 之后
import { fetchResetDateBounds, resetDateBoundsSync } from "../SettingsView";

describe("fetchResetDateBounds：bounds 来自后端，无 31 天硬限制", () => {
  beforeEach(() => {
    invokeMock = vi.fn();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-16T15:00:00Z"));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("库有数据：earliest 取库最早，today 取最新", async () => {
    const earliest = Date.parse("2025-09-01T00:00:00Z");
    const latest = Date.parse("2026-08-16T15:00:00Z");
    invokeMock.mockResolvedValueOnce({ earliest_ms: earliest, latest_ms: latest });
    const b = await fetchResetDateBounds();
    expect(invokeMock).toHaveBeenCalledWith("reset_range_bounds", {});
    expect(b.earliest).toBe("2025-09-01");
    expect(b.today).toBe("2026-08-16");
  });

  it("空库（earliest_ms=0）：fallback 到 today", async () => {
    const latest = Date.parse("2026-08-16T15:00:00Z");
    invokeMock.mockResolvedValueOnce({ earliest_ms: 0, latest_ms: latest });
    const b = await fetchResetDateBounds();
    expect(b.earliest).toBe("2026-08-16");
    expect(b.today).toBe("2026-08-16");
  });

  it("后端命令失败：降级到 resetDateBoundsSync（earliest=today）", async () => {
    invokeMock.mockRejectedValueOnce("network error");
    const b = await fetchResetDateBounds();
    expect(b.earliest).toBe("2026-08-16");
    expect(b.today).toBe("2026-08-16");
  });

  it("任意历史日期（>31 天）能作为 earliest：用户可自选很早的日期", async () => {
    // 1 年前 = 2025-08-16（旧 31 天限制下不可选）
    const earliest = Date.parse("2025-08-16T00:00:00Z");
    const latest = Date.parse("2026-08-16T15:00:00Z");
    invokeMock.mockResolvedValueOnce({ earliest_ms: earliest, latest_ms: latest });
    const b = await fetchResetDateBounds();
    expect(b.earliest).toBe("2025-08-16");
    // ⚠️ 不再是 today-31days；可以远早于 30 天前
    expect(new Date(b.earliest).getTime()).toBeLessThan(Date.parse("2026-07-17"));
  });
});

describe("resetDateBoundsSync：纯函数兜底", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-16T15:00:00Z"));
  });
  afterEach(() => vi.useRealTimers());

  it("earliest 和 today 都是今天（兜底用）", () => {
    const b = resetDateBoundsSync();
    expect(b.earliest).toBe("2026-08-16");
    expect(b.today).toBe("2026-08-16");
  });
});
