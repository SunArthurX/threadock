// 新页面核心逻辑测试：热力图网格 / 知识提取断言 / 提示词收藏
import { describe, expect, it, beforeEach } from "vitest";
import { buildHeatGrid, heatColor } from "../ActivityView";
import { loadPromptFavorites, togglePromptFavorite } from "../KnowledgeView";

describe("热力图（活动节律页）", () => {
  it("按周列排布、空档补 null、max 正确", () => {
    // 2026-08-10 是周一，连续 8 天（跨两周）
    const cells = Array.from({ length: 8 }, (_, i) => ({
      day: `2026-08-${String(10 + i).padStart(2, "0")}`,
      calls: i + 1,
    }));
    const { cols, max } = buildHeatGrid(cells);
    expect(max).toBe(8);
    expect(cols.length).toBeGreaterThanOrEqual(2);
    // 第一列第一格是 08-10
    expect(cols[0][1]?.day).toBe("2026-08-10");
    expect(cols[0][1]?.calls).toBe(1);
  });

  it("无数据返回空网格", () => {
    expect(buildHeatGrid([]).cols).toHaveLength(0);
  });

  it("颜色分档：0=边框色，最大=最深", () => {
    expect(heatColor(0, 10)).toContain("var(");
    const darkest = heatColor(10, 10);
    const lightest = heatColor(2, 10);
    expect(darkest).not.toBe(lightest);
  });
});

describe("提示词收藏（知识库页）", () => {
  beforeEach(() => localStorage.removeItem("ch-prompt-favs"));
  it("收藏/取消往返持久化", () => {
    expect(loadPromptFavorites()).toHaveLength(0);
    let next = togglePromptFavorite("m1");
    expect(next).toContain("m1");
    next = togglePromptFavorite("m2");
    expect(loadPromptFavorites().sort()).toEqual(["m1", "m2"]);
    togglePromptFavorite("m1");
    expect(loadPromptFavorites()).toEqual(["m2"]);
  });
});
