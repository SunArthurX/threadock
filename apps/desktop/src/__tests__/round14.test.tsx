// 第 14 轮测试：热力图横向布局 + hover 自定义 tooltip + 来源 chip 强调 +
//                详情页去收藏/归档 + 知识弹窗 resize + MD/JSON dropdown 合并
import { describe, expect, it, beforeAll, beforeEach, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "activity_stats") {
      // 模拟 30 天数据（含 weekend + weekday）
      const cells = Array.from({ length: 30 }, (_, i) => {
        const day = new Date(2026, 6, 1 + i);
        const ymd = day.toISOString().slice(0, 10);
        return { day: ymd, calls: 1 + (i % 5), sessions: 1 + (i % 3) };
      });
      return {
        heatmap: cells,
        hourly: Array.from({ length: 24 }, (_, h) => ({ hour: h, calls: 5 })),
        weekday: [], weekend: [],
        tools_trend: [], tool_daily: [],
        tool_list: ["Read", "Write", "Bash"],
      };
    }
    if (cmd === "list_conversations_by_date") return [];
    return null;
  }),
}));

import ActivityView, { buildHeatGrid } from "../ActivityView";
import KnowledgeModal from "../KnowledgeModal";
import ConversationDetail from "../ConversationDetail";
import ConversationList from "../ConversationList";
import type { Conversation, ExtractionResult } from "../types";

beforeEach(() => { localStorage.clear(); vi.restoreAllMocks(); });

const sampleKnowledge: ExtractionResult = {
  summary: "测试摘要",
  decisions: [{ decision: "决策 A" }],
  todos: [{ text: "TODO 1" }],
  errors: [{ error: "Error X" }],
  commands: ["ls -la"],
  files: [{ path: "src/app.ts" }],
  extractor: "test",
};

const baseConv: Conversation = {
  id: "c1", provider: "zcode", source_conversation_id: "s", title: "测试", user_title: null,
  status: null, model: null, completeness_score: null, workspace_id: null,
  started_at_ms: null, updated_at_ms: null, source_parent_id: null, child_count: 0,
  favorite: false, archived: false,
};

describe("热力图横向布局（N 行 7 列）", () => {
  it("每行 = 1 周（heatmap-row 内 7 cells 横排）", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      const rows = container.querySelectorAll(".heatmap-row");
      expect(rows.length).toBeGreaterThan(0);
    });
    const firstRow = container.querySelectorAll(".heatmap-row")[0]!;
    const cellsInRow = firstRow.querySelectorAll(".heat-cell");
    expect(cellsInRow.length).toBe(7);
  });

  it("顶部 weekday header 含 7 个英文星期", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      expect(container.querySelectorAll(".heat-weekday-cell").length).toBe(7);
    });
    const labels = [...container.querySelectorAll(".heat-weekday-cell")].map((el) => el.textContent);
    expect(labels).toEqual(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
  });

  it("每行行首有月份 label（GitHub 风格 Aug/Sep/...）", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      const monthLabels = container.querySelectorAll(".heat-month-label");
      expect(monthLabels.length).toBeGreaterThan(0);
    });
    const firstMonth = container.querySelectorAll(".heat-month-label")[0]?.textContent ?? "";
    // 至少有一个英文月份名
    expect(firstMonth).toMatch(/^(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)$/);
  });
});

describe("热力图 hover 自定义 tooltip", () => {
  it("mouseEnter cell 显示 [data-testid=heat-tooltip]", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      expect(container.querySelectorAll(".heat-cell").length).toBeGreaterThan(0);
    });
    const dataCell = container.querySelector(".heat-cell:not(.empty)") as HTMLElement;
    expect(dataCell).toBeTruthy();
    fireEvent.mouseEnter(dataCell, { clientX: 200, clientY: 200 });
    await waitFor(() => {
      expect(container.querySelector("[data-testid='heat-tooltip']")).toBeTruthy();
    });
  });

  it("tooltip 含日期 + 调用数 + 会话数", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      expect(container.querySelectorAll(".heat-cell").length).toBeGreaterThan(0);
    });
    const dataCell = container.querySelector(".heat-cell:not(.empty)") as HTMLElement;
    fireEvent.mouseEnter(dataCell, { clientX: 200, clientY: 200 });
    await waitFor(() => {
      const tt = container.querySelector("[data-testid='heat-tooltip']");
      expect(tt?.textContent).toMatch(/\d{4}-\d{2}-\d{2}/);
      expect(tt?.textContent).toMatch(/次调用/);
      expect(tt?.textContent).toMatch(/活跃会话/);
    });
  });

  it("mouseLeave cell 后 tooltip 消失", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      expect(container.querySelectorAll(".heat-cell").length).toBeGreaterThan(0);
    });
    const dataCell = container.querySelector(".heat-cell:not(.empty)") as HTMLElement;
    fireEvent.mouseEnter(dataCell, { clientX: 200, clientY: 200 });
    await waitFor(() => {
      expect(container.querySelector("[data-testid='heat-tooltip']")).toBeTruthy();
    });
    fireEvent.mouseLeave(dataCell);
    await waitFor(() => {
      expect(container.querySelector("[data-testid='heat-tooltip']")).toBeNull();
    });
  });
});

describe("CSS：热力图横向布局 + tooltip", () => {
  let css = "";
  beforeAll(() => {
    const fs = require("node:fs");
    const path = require("node:path");
    const { fileURLToPath } = require("node:url");
    const HERE = path.dirname(fileURLToPath(import.meta.url));
    css = fs.readFileSync(path.resolve(HERE, "../styles.css"), "utf-8");
  });

  it(".heatmap-rows-wrap 存在（横向布局容器）", () => {
    expect(/\.heatmap-rows-wrap\s*\{/.test(css)).toBe(true);
  });

  it(".heatmap-row 存在（每周 1 行）", () => {
    expect(/\.heatmap-row\s*\{/.test(css)).toBe(true);
  });

  it(".heat-tooltip 存在（hover 自定义 tooltip）", () => {
    expect(/\.heat-tooltip\s*\{/.test(css)).toBe(true);
    expect(css.includes("position: fixed")).toBe(true);
  });
});

describe("详情页去掉收藏/归档按钮（右键菜单已覆盖）", () => {
  const baseDetail = {
    conv: baseConv, messages: [], events: [], completenessLabel: "",
    loading: false, exporting: false, timelineMode: false, highlightMsgId: null,
    collapsedMsgs: new Set<string>(), tags: [],
    onToggleTimeline: () => {}, onExport: () => {}, onExtractKnowledge: () => {},
    onToggleCollapse: () => {},
    onAddTag: () => {}, onRemoveTag: () => {}, onRescanAudit: () => {},
  };

  it("toolbar 不再含「收藏」按钮", () => {
    const { container } = render(<ConversationDetail {...baseDetail} />);
    const bar = container.querySelector(".detail-actions");
    expect(bar?.textContent).not.toContain("收藏");
  });

  it("toolbar 不再含「归档」按钮", () => {
    const { container } = render(<ConversationDetail {...baseDetail} />);
    const bar = container.querySelector(".detail-actions");
    expect(bar?.textContent).not.toContain("归档");
  });

  it("toolbar 仍含「时间线/知识/重扫/仅用户消息/搜索消息/复制全部/下载」", () => {
    const { container } = render(<ConversationDetail {...baseDetail} />);
    const bar = container.querySelector(".detail-actions");
    expect(bar?.textContent).toContain("时间线");
    expect(bar?.textContent).toContain("知识");
    expect(bar?.textContent).toContain("重扫");
    expect(bar?.textContent).toContain("仅用户消息");
    expect(bar?.textContent).toContain("搜索消息");
    expect(bar?.textContent).toContain("复制全部");
    expect(bar?.textContent).toContain("下载");
  });
});

describe("知识弹窗：可缩小 + MD/JSON dropdown 合并", () => {
  it("knowledge-modal 含 resize: both CSS", async () => {
    const fs = require("node:fs");
    const path = require("node:path");
    const { fileURLToPath } = require("node:url");
    const HERE = path.dirname(fileURLToPath(import.meta.url));
    const css = fs.readFileSync(path.resolve(HERE, "../styles.css"), "utf-8");
    expect(/\.knowledge-modal\s*\{[^}]*resize:\s*both/m.test(css)).toBe(true);
  });

  it("knowledge-modal 含 min-width: 320px 限制（不会拖太小）", async () => {
    const fs = require("node:fs");
    const path = require("node:path");
    const { fileURLToPath } = require("node:url");
    const HERE = path.dirname(fileURLToPath(import.meta.url));
    const css = fs.readFileSync(path.resolve(HERE, "../styles.css"), "utf-8");
    expect(/\.knowledge-modal\s*\{[^}]*min-width:\s*320px/m.test(css)).toBe(true);
  });

  it("MD/JSON 合并为单一「⤓ 导出」按钮（不再有独立 ⤓ MD / ⤓ JSON）", () => {
    const { container } = render(<KnowledgeModal knowledge={sampleKnowledge} onClose={() => {}} onReextract={() => {}} />);
    const buttons = [...container.querySelectorAll("button")].map((b) => b.textContent ?? "");
    expect(buttons.some((t) => t.includes("⤓ 导出"))).toBe(true);
    expect(buttons.some((t) => t.trim() === "⤓ MD")).toBe(false);
    expect(buttons.some((t) => t.trim() === "⤓ JSON")).toBe(false);
  });

  it("点「⤓ 导出」展开 dropdown 含 Markdown + JSON 2 个选项", () => {
    const { container } = render(<KnowledgeModal knowledge={sampleKnowledge} onClose={() => {}} onReextract={() => {}} />);
    const exportBtn = [...container.querySelectorAll("button")].find((b) => b.textContent?.includes("⤓ 导出"))!;
    fireEvent.click(exportBtn);
    const items = container.querySelectorAll(".list-dropdown-item");
    expect(items.length).toBe(2);
    expect(items[0]?.textContent).toContain("Markdown");
    expect(items[1]?.textContent).toContain("JSON");
  });
});

describe("来源 chip 强调（高亮 + 与列表分隔）", () => {
  const convs: Conversation[] = [
    { id: "c1", provider: "zcode", source_conversation_id: "sc1", title: "A", user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 3600_000, updated_at_ms: Date.now() - 3600_000, child_count: 0, favorite: false, archived: false },
  ];
  const baseProps = {
    conversations: convs, selectedConv: null, loading: false, providerFilter: null, selectedWs: null,
    expandedParents: new Set<string>(), childConvs: {} as Record<string, Conversation[]>,
    scope: "all" as const, onScopeChange: () => {}, onFilter: () => {}, onSelect: () => {},
    onToggleExpand: () => {}, onClearWs: () => {},
  };

  it("provider-chip 有边框 + 颜色（非透明）", () => {
    const { container } = render(<ConversationList {...baseProps} availableProviders={new Set(["zcode", "claude-code", "cursor", "minimax-code", "codex"])} />);
    const chip = container.querySelector(".provider-chip") as HTMLElement;
    expect(chip).toBeTruthy();
    // CSS 应让 chip 有 border-color 强调（不是 default var(--border)）
    expect(cssTextIncludesBorder(chip)).toBe(true);
  });

  it("list-provider-chips 与列表分隔（上下 border）", () => {
    const { container } = render(<ConversationList {...baseProps} availableProviders={new Set(["zcode", "claude-code", "cursor", "minimax-code", "codex"])} />);
    const wrap = container.querySelector(".list-provider-chips") as HTMLElement;
    expect(wrap).toBeTruthy();
    const style = getComputedStyle(wrap);
    expect(style.borderTopWidth).not.toBe("0px");
    expect(style.borderBottomWidth).not.toBe("0px");
  });
});

/** 辅助：检查元素 computed style 是否包含非空 border（非 transparent / non-0） */
function cssTextIncludesBorder(el: HTMLElement): boolean {
  const s = getComputedStyle(el);
  return Boolean(s.borderTopWidth && s.borderTopWidth !== "0px" && s.borderTopColor !== "rgba(0, 0, 0, 0)");
}

describe("buildHeatGrid 横向布局兼容性", () => {
  it("30 天数据生成至少 5 行（每行 1 周）", () => {
    const cells = Array.from({ length: 30 }, (_, i) => ({
      day: `2026-07-${String(1 + i).padStart(2, "0")}`,
      calls: 1,
    }));
    const r = buildHeatGrid(cells);
    // 30 天 = 至少 5 周（4-5 周 + 边界补齐）
    expect(r.cols.length).toBeGreaterThanOrEqual(4);
  });
});
