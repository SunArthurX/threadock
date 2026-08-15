// 纯函数单测：标签映射与格式化（types.ts / charts.tsx）
import { describe, expect, it } from "vitest";
import { eventTypeLabel, formatTime, sourceLabel } from "../types";
import { formatCost, formatDuration, formatTokens } from "../charts";

describe("sourceLabel", () => {
  it("映射已知来源为中文/品牌名", () => {
    expect(sourceLabel("claude-code")).toBe("Claude Code");
    expect(sourceLabel("zcode")).toBe("ZCode");
    expect(sourceLabel("minimax-code")).toBe("MiniMax");
    expect(sourceLabel("generic")).toBe("导入");
  });
  it("未知来源原样返回", () => {
    expect(sourceLabel("new-ide-2030")).toBe("new-ide-2030");
  });
});

describe("eventTypeLabel", () => {
  it("映射事件类型", () => {
    expect(eventTypeLabel("command_started")).toBe("命令");
    expect(eventTypeLabel("approval_denied")).toBe("拒绝");
  });
  it("未知类型原样返回", () => {
    expect(eventTypeLabel("custom_event")).toBe("custom_event");
  });
});

describe("formatTime", () => {
  it("null/0 返回空串", () => {
    expect(formatTime(null)).toBe("");
    expect(formatTime(0)).toBe("");
  });
  it("毫秒时间戳格式化为 YYYY-MM-DD HH:mm:ss", () => {
    const ms = new Date(2026, 7, 15, 9, 5, 3).getTime();
    expect(formatTime(ms)).toBe("2026-08-15 09:05:03");
  });
});

describe("formatTokens", () => {
  it("按数量级缩写", () => {
    expect(formatTokens(950)).toBe("950");
    expect(formatTokens(1_234)).toBe("1.2K");
    expect(formatTokens(1_200_000)).toBe("1.20M");
    expect(formatTokens(4_300_000_000)).toBe("4.30B");
  });
});

describe("formatCost", () => {
  it("0/负数显示破折号", () => {
    expect(formatCost(0)).toBe("—");
    expect(formatCost(-1)).toBe("—");
  });
  it("小于 100 保留两位小数，大于等于 100 取整", () => {
    expect(formatCost(1.5)).toBe("$1.50");
    expect(formatCost(123.4)).toBe("$123");
  });
});

describe("formatDuration", () => {
  it("毫秒/秒/分钟分级", () => {
    expect(formatDuration(0)).toBe("—");
    expect(formatDuration(150)).toBe("150ms");
    expect(formatDuration(2_500)).toBe("2.5s");
    expect(formatDuration(90_000)).toBe("1.5min");
  });
});
