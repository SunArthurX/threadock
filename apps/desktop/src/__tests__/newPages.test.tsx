// 新页面核心逻辑测试：热力图网格 / 知识提取断言 / 提示词收藏
import { describe, expect, it, beforeEach } from "vitest";
import { buildHeatGrid, heatColor, dayPart } from "../ActivityView";
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
    const first = togglePromptFavorite("m1");
    expect(first).toContain("m1");
    togglePromptFavorite("m2");
    expect(loadPromptFavorites().sort()).toEqual(["m1", "m2"]);
    togglePromptFavorite("m1");
    expect(loadPromptFavorites()).toEqual(["m2"]);
  });
});

describe("活动页 5 轮优化", () => {
  it("热力图带月份标签", () => {
    const cells = Array.from({ length: 40 }, (_, i) => ({
      day: `2026-07-${String(20 + i).padStart(2, "0")}`,
      calls: i,
    })).concat(Array.from({ length: 5 }, (_, i) => ({
      day: `2026-08-${String(10 + i).padStart(2, "0")}`,
      calls: i,
    })));
    const { labels } = buildHeatGrid(cells);
    const texts = labels.map((l) => l.label);
    expect(texts).toContain("7月");
    expect(texts).toContain("8月");
  });

  it("时段分组正确", () => {
    expect(dayPart(2)).toBe("凌晨");
    expect(dayPart(9)).toBe("上午");
    expect(dayPart(14)).toBe("下午");
    expect(dayPart(22)).toBe("晚上");
  });
});

describe("知识库 5 轮优化", () => {
  beforeEach(() => localStorage.removeItem("ch-todo-done"));
  it("TODO 完成勾选往返持久化", async () => {
    const { loadDoneTodos, toggleDoneTodo } = await import("../KnowledgeView");
    expect(loadDoneTodos().size).toBe(0);
    let s = toggleDoneTodo("写测试");
    expect(s.has("写测试")).toBe(true);
    s = toggleDoneTodo("写测试");
    expect(s.has("写测试")).toBe(false);
  });

  it("知识库导出 Markdown 含完成状态", async () => {
    const { knowledgeBaseToMarkdown, toggleDoneTodo } = await import("../KnowledgeView");
    toggleDoneTodo("已完成事项");
    const md = knowledgeBaseToMarkdown({
      todos: [{ text: "已完成事项", title: "A" }, { text: "未完成事项", title: "B" }],
      decisions: [{ text: "用 SQLite", title: "A" }],
      top_commands: [{ cmd: "cargo test", count: 9 }],
      top_files: [],
    });
    expect(md).toContain("- [x] 已完成事项（A）");
    expect(md).toContain("- [ ] 未完成事项（B）");
    expect(md).toContain("- 用 SQLite（A）");
    expect(md).toContain("`cargo test` ×9");
  });
});

describe("项目页 5 轮优化", () => {
  const rows = [
    { dir: "/a/p1", sessions: 1, tokens: 100, cost_usd: 1, requests: 1, last_active_ms: 300, main_agent: "zcode" },
    { dir: "/a/p2", sessions: 5, tokens: 900, cost_usd: 0.5, requests: 9, last_active_ms: 100, main_agent: "codex" },
    { dir: "/a/p3", sessions: 3, tokens: 500, cost_usd: 2, requests: 5, last_active_ms: 200, main_agent: null },
  ] as never[];
  it("四种排序键正确排序", async () => {
    const { sortProjects } = await import("../ProjectsView");
    expect(sortProjects(rows, "cost")[0].cost_usd).toBe(2);
    expect(sortProjects(rows, "tokens")[0].tokens).toBe(900);
    expect(sortProjects(rows, "sessions")[0].sessions).toBe(5);
    expect(sortProjects(rows, "active")[0].last_active_ms).toBe(300);
  });
});
