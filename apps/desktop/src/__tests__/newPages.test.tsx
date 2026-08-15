// 新页面核心逻辑测试：热力图网格 / 知识提取断言 / 提示词收藏
import { render } from "@testing-library/react";
import { describe, expect, it, beforeEach, vi } from "vitest";
import { buildHeatGrid, heatColor, dayPart, daysToRange, weekdayCN, isWeekend, calcStreak } from "../ActivityView";
import { loadPromptFavorites, togglePromptFavorite } from "../KnowledgeView";
import { sortProjects, projectsToCsv } from "../ProjectsView";

// 为「.slice 崩溃回归」专门 mock 一次 invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => ({
    heatmap: [],
    hourly: [{ hour: 14, calls: 5 }],
    tools_trend: [
      { month: "2026-07", tool: "Bash", calls: 10 },
      { month: undefined, tool: "Read", calls: 1 },
      { month: null, tool: "Read", calls: 2 },
      { month: "", tool: "Read", calls: 3 },
      { month: "2026-08", tool: "Bash", calls: 20 },
      { month: "2026-08", tool: "Edit", calls: 8 },
    ],
  })),
}));

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

  it("颜色分档：0=近透明，最大=最深", () => {
    expect(heatColor(0, 10)).toBe("rgba(255, 255, 255, 0.04)"); // 空档极淡底
    const darkest = heatColor(10, 10);
    const lightest = heatColor(2, 10);
    expect(darkest).not.toBe(lightest);
    // 5 档不同
    expect(heatColor(0, 100)).not.toBe(heatColor(20, 100));
    expect(heatColor(40, 100)).not.toBe(heatColor(70, 100));
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

describe("项目页第 6-10 轮优化", () => {
  const rows = [
    { dir: "/a/p1", sessions: 1, tokens: 100, cost_usd: 1, requests: 1, last_active_ms: 300, main_agent: "zcode" },
    { dir: "/a/p2", sessions: 5, tokens: 900, cost_usd: 0.5, requests: 9, last_active_ms: 100, main_agent: "codex" },
    { dir: "/a/p3", sessions: 3, tokens: 500, cost_usd: 2, requests: 5, last_active_ms: 200, main_agent: null },
  ] as never[];

  it("sortProjects 升降序切换正确", () => {
    expect(sortProjects(rows, "cost", "desc")[0].cost_usd).toBe(2);
    expect(sortProjects(rows, "cost", "asc")[0].cost_usd).toBe(0.5);
    expect(sortProjects(rows, "tokens", "asc")[0].tokens).toBe(100);
    expect(sortProjects(rows, "active", "asc")[0].last_active_ms).toBe(100);
    expect(sortProjects(rows, "sessions", "asc")[0].sessions).toBe(1);
  });

  it("projectsToCsv 含 UTF-8 BOM + 表头 + 转义", () => {
    const csv = projectsToCsv([
      { dir: '/dir with "quote",comma', sessions: 2, tokens: 100, cost_usd: 0.5, requests: 3, last_active_ms: 1, main_agent: "zcode" },
      { dir: "/simple", sessions: 1, tokens: 50, cost_usd: 0.1, requests: 1, last_active_ms: null, main_agent: null },
    ] as never[]);
    // UTF-8 BOM
    expect(csv.charCodeAt(0)).toBe(0xFEFF);
    // 表头
    expect(csv).toContain("目录,会话数,请求数,Tokens,成本USD,主力Agent,最近活跃(ms)");
    // 含逗号和引号要转义
    expect(csv).toContain('"/dir with ""quote"",comma"');
    // null main_agent
    expect(csv).toContain(",,");
  });
});

describe("活动页第 6-10 轮优化", () => {
  it("daysToRange 输出 YYYY-MM-DD ~ YYYY-MM-DD 格式", () => {
    // 固定 now 避免时区/时间漂移
    const now = new Date("2026-08-16T00:00:00").getTime();
    expect(daysToRange(7, now)).toBe("2026-08-09 ~ 2026-08-16");
    expect(daysToRange(30, now)).toBe("2026-07-17 ~ 2026-08-16");
    expect(daysToRange(365, now)).toBe("2025-08-16 ~ 2026-08-16");
  });

  it("weekdayCN 返回中文星期几", () => {
    expect(weekdayCN("2026-08-16")).toBe("周日"); // 2026-08-16 是周日
    expect(weekdayCN("2026-08-17")).toBe("周一");
    expect(weekdayCN("2026-08-22")).toBe("周六");
  });

  it("isWeekend 正确判定周末", () => {
    expect(isWeekend("2026-08-16")).toBe(true);  // 周日
    expect(isWeekend("2026-08-17")).toBe(false); // 周一
    expect(isWeekend("2026-08-22")).toBe(true);  // 周六
  });

  it("calcStreak 连续活跃天数（容许今天没活动）", () => {
    const today = new Date();
    const todayKey = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
    const yesterday = new Date(today); yesterday.setDate(today.getDate() - 1);
    const yKey = `${yesterday.getFullYear()}-${String(yesterday.getMonth() + 1).padStart(2, "0")}-${String(yesterday.getDate()).padStart(2, "0")}`;
    const d2 = new Date(today); d2.setDate(today.getDate() - 2);
    const d2Key = `${d2.getFullYear()}-${String(d2.getMonth() + 1).padStart(2, "0")}-${String(d2.getDate()).padStart(2, "0")}`;
    // 今天 + 昨天 + 前天 都活跃 → 3 天
    expect(calcStreak([{ day: todayKey, calls: 1 }, { day: yKey, calls: 1 }, { day: d2Key, calls: 1 }])).toBe(3);
    // 今天没活动但昨天 + 前天活跃 → 仍算 2 天（容许今天没活动）
    expect(calcStreak([{ day: yKey, calls: 1 }, { day: d2Key, calls: 1 }])).toBe(2);
    // 完全没数据 → 0
    expect(calcStreak([])).toBe(0);
    // 只有今天活跃 → 1
    expect(calcStreak([{ day: todayKey, calls: 1 }])).toBe(1);
  });
});

describe("热力图防御（黑屏回归）", () => {
  it("非法日期串不崩溃（过滤后空网格）", () => {
    expect(() => buildHeatGrid([{ day: "garbage", calls: 1 }, { day: "", calls: 2 }])).not.toThrow();
    const r = buildHeatGrid([{ day: "garbage", calls: 1 }]);
    expect(r.cols).toHaveLength(0);
  });

  it("合法数据网格与月份标签正常", () => {
    const r = buildHeatGrid([
      { day: "2026-08-10", calls: 3 },
      { day: "2026-08-11", calls: 5 },
      { day: "2026-08-12", calls: 1 },
    ]);
    expect(r.max).toBe(5);
    expect(r.labels[0].label).toBe("8月");
    expect(r.cols.length).toBeGreaterThanOrEqual(1);
    // 08-10 周一：首列首格补 null，第二格为 08-10
    expect(r.cols[0][0]).toBeNull();
    expect(r.cols[0][1]?.day).toBe("2026-08-10");
  });

  it("错误边界捕获渲染错误不黑屏", async () => {
    const { default: ErrorBoundary } = await import("../ErrorBoundary");
    const Boom = () => { throw new Error("BOOM"); };
    const { findByText } = render(<ErrorBoundary><Boom /></ErrorBoundary>);
    expect(await findByText("⚠ 页面渲染出错")).toBeTruthy();
    expect(await findByText(/BOOM/)).toBeTruthy();
  });
});

describe("活动页 .slice 崩溃回归（month 为 undefined）", () => {
  // 模拟后端早期版本：tools_trend 数组里有 month 字段缺失/异常的脏行
  // 之前是 `month.slice(2)` 直接抛 "undefined is not an object"
  // 现已在前端用 /^\d{4}-\d{2}$/ 过滤安全 month
  it("脏 month 不进入 trend 渲染（不抛错）", async () => {
    const { default: ActivityView } = await import("../ActivityView");
    // 触发渲染；如果 .slice bug 还在会 throw
    const { findByText } = render(<ActivityView />);
    // 找到标题即视为渲染成功
    expect(await findByText("📆 活动节律")).toBeTruthy();
  });
});
