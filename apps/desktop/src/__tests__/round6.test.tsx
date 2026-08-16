// 第 6 轮大改版测试：KnowledgeModal tabs/导出、ActivityView 工具维度、CostSection 周对比 +
// per-model、prefs 货币/数字/日期、App 快捷键、ConversationList Pin/排序、ChangelogModal
import { fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "activity_stats") {
      // 工具维度数据，给 ActivityView 的 tool_daily 用
      const heatmap: { day: string; calls: number; sessions: number }[] = [];
      const now = Date.now();
      for (let i = 30; i >= 0; i--) {
        const d = new Date(now - i * 86_400_000);
        const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
        heatmap.push({ day: key, calls: Math.floor(Math.random() * 10), sessions: 2 });
      }
      return {
        heatmap,
        hourly: [{ hour: 14, calls: 13 }],
        hourly_weekday: Array.from({ length: 24 }, (_, h) => ({ hour: h, calls: h === 14 ? 13 : 0 })),
        hourly_weekend: Array.from({ length: 24 }, () => ({ hour: 0, calls: 0 })),
        tools_trend: [{ month: "2026-08", tool: "Bash", calls: 100 }],
        tool_daily: [
          { day: heatmap[0].day, tool: "Bash", calls: 3 },
          { day: heatmap[0].day, tool: "Read", calls: 2 },
        ],
      };
    }
    if (cmd === "ops_by_provider") return [
      { provider: "claude-code", requests: 10, total_tokens: 1000, output_tokens: 500, errors: 1, cost_usd: 5.2 },
    ];
    if (cmd === "ops_by_model") return [
      { model: "claude-sonnet-4-5", provider_id: "claude-code", requests: 100, input_tokens: 5000, output_tokens: 2000, errors: 1, cost_usd: 30.5 },
    ];
    if (cmd === "ops_timeseries") {
      const arr: { day: string; total_tokens: number; requests: number }[] = [];
      const now = Date.now();
      for (let i = 13; i >= 0; i--) {
        const d = new Date(now - i * 86_400_000);
        arr.push({
          day: `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`,
          total_tokens: i < 7 ? 1_000_000 : 800_000,
          requests: i < 7 ? 100 : 80,
        });
      }
      return arr;
    }
    if (cmd === "save_text_file") return null;
    return null;
  }),
}));

// dialog plugin mock
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(async () => "/tmp/test-export.md"),
  open: vi.fn(async () => null),
}));

import KnowledgeModal, { knowledgeToMarkdown, knowledgeToJson } from "../KnowledgeModal";
import { weekOverWeek, calcProjection } from "../CostSection";
import CostSection from "../CostSection";
import { formatCostPref, formatTokensPref, formatTimePref } from "../prefs";
import { loadPinnedIds } from "../ConversationList";
import ChangelogModal, { getLastSeenVersion, markVersionSeen, shouldShowChangelog } from "../ChangelogModal";
import ActivityView from "../ActivityView";

describe("KnowledgeModal 第 6 轮增强", () => {
  const sample = {
    summary: "本次会话完成了 X / Y / Z 三件事",
    decisions: [{ decision: "用 TypeScript 重写" }, { decision: "走 Tauri 2.x 升级路径" }],
    todos: [{ text: "跑完 e2e 测试" }, { text: "更新用户文档" }],
    errors: [{ error: "vite build OOM" }],
    commands: ["npm run build", "cargo test"],
    files: [{ path: "/src/main.rs" }, { path: "/apps/desktop/src/App.tsx" }],
    extractor: "rule-based",
  };

  it("knowledgeToMarkdown 生成 6 个 section", () => {
    const md = knowledgeToMarkdown(sample);
    expect(md).toContain("# 会话纪要");
    expect(md).toContain("## 摘要");
    expect(md).toContain("## 决策");
    expect(md).toContain("## TODO");
    expect(md).toContain("## 错误");
    expect(md).toContain("## 命令");
    expect(md).toContain("## 涉及文件");
    expect(md).toContain("- [ ] 跑完 e2e 测试");
  });

  it("knowledgeToJson 包含全部字段 + 导出时间戳", () => {
    const json = JSON.parse(knowledgeToJson(sample));
    expect(json.summary).toBe(sample.summary);
    expect(json.decisions.length).toBe(2);
    expect(json.todos.length).toBe(2);
    expect(json.errors.length).toBe(1);
    expect(json.commands.length).toBe(2);
    expect(json.files.length).toBe(2);
    expect(json.extractor).toBe("rule-based");
    expect(typeof json.exported_at).toBe("string");
  });

  it("tabs 默认「全部」展示所有 section", () => {
    const { container } = render(<KnowledgeModal knowledge={sample} onClose={() => {}} onReextract={() => {}} />);
    expect(container.querySelectorAll(".knowledge-block").length).toBe(6);
  });

  it("点击「决策」tab 只显示决策 block", () => {
    const { container } = render(<KnowledgeModal knowledge={sample} onClose={() => {}} onReextract={() => {}} />);
    const decTab = Array.from(container.querySelectorAll(".knowledge-tabs button")).find((b) => b.textContent?.includes("决策")) as HTMLButtonElement;
    expect(decTab).toBeTruthy();
    fireEvent.click(decTab);
    expect(container.querySelectorAll(".knowledge-block").length).toBe(1);
    expect(container.querySelector(".knowledge-block.decisions")).toBeInTheDocument();
  });

  it("点击「TODO」tab 只显示 TODO block", () => {
    const { container } = render(<KnowledgeModal knowledge={sample} onClose={() => {}} onReextract={() => {}} />);
    const tab = Array.from(container.querySelectorAll(".knowledge-tabs button")).find((b) => b.textContent?.includes("TODO")) as HTMLButtonElement;
    fireEvent.click(tab);
    expect(container.querySelectorAll(".knowledge-block").length).toBe(1);
    expect(container.querySelector(".knowledge-block.todos")).toBeInTheDocument();
  });

  it("tab 按钮带计数徽标", () => {
    const { container } = render(<KnowledgeModal knowledge={sample} onClose={() => {}} onReextract={() => {}} />);
    const decTab = Array.from(container.querySelectorAll(".knowledge-tabs button")).find((b) => b.textContent?.includes("决策")) as HTMLButtonElement;
    expect(decTab.textContent).toContain("2"); // 2 个决策
    const todoTab = Array.from(container.querySelectorAll(".knowledge-tabs button")).find((b) => b.textContent?.includes("TODO")) as HTMLButtonElement;
    expect(todoTab.textContent).toContain("2");
  });

  it("导出 MD dropdown → 选 MD 触发 save_text_file", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const { container } = render(<KnowledgeModal knowledge={sample} convTitle="测试会话" onClose={() => {}} onReextract={() => {}} />);
    // 第 14 轮：MD/JSON 合并为单一 dropdown 按钮"⤓ 导出"
    const exportBtn = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes("⤓ 导出")) as HTMLButtonElement;
    expect(exportBtn).toBeTruthy();
    fireEvent.click(exportBtn);
    // 展开 dropdown 后点 Markdown
    const mdItem = await waitFor(() => {
      const items = container.querySelectorAll(".list-dropdown-item");
      const found = Array.from(items).find((b) => b.textContent?.includes("Markdown"));
      if (!found) throw new Error("Markdown option not found");
      return found as HTMLButtonElement;
    });
    fireEvent.click(mdItem);
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("save_text_file", expect.objectContaining({
        path: expect.stringMatching(/\.md$/),
        content: expect.stringContaining("# 会话纪要"),
      }));
    });
  });
});

describe("CostSection weekOverWeek 周对比", () => {
  it("timeseries 不足 14 天 → 返回 null", () => {
    expect(weekOverWeek(null)).toBeNull();
    expect(weekOverWeek([])).toBeNull();
    expect(weekOverWeek([{ day: "2026-08-12", total_tokens: 1000, requests: 10 }])).toBeNull();
  });

  it("本周 vs 上周：本周 25% 增长", () => {
    const arr = Array.from({ length: 14 }, (_, i) => ({
      day: `2026-08-${String(1 + i).padStart(2, "0")}`,
      total_tokens: i >= 7 ? 1_000_000 : 800_000, // 本周高
      requests: i >= 7 ? 100 : 80,
    }));
    const r = weekOverWeek(arr);
    expect(r).not.toBeNull();
    expect(r!.thisWeek.tokens).toBe(7_000_000);
    expect(r!.lastWeek.tokens).toBe(5_600_000);
    expect(r!.tokenPct).toBeCloseTo(0.25, 2); // 25% 增长
  });

  it("自定义单价：本周 cost 应按比例变化", () => {
    const arr = Array.from({ length: 14 }, (_, i) => ({
      day: `2026-08-${String(1 + i).padStart(2, "0")}`,
      total_tokens: 1_000_000,
      requests: 100,
    }));
    const r1 = weekOverWeek(arr, 4);
    const r2 = weekOverWeek(arr, 8);
    expect(r2!.thisWeek.cost).toBe(r1!.thisWeek.cost * 2);
  });
});

describe("CostSection calcProjection 兼容", () => {
  it("null + day<2 → null", () => {
    expect(calcProjection(null, new Date(2026, 7, 15))).toBeNull();
    expect(calcProjection({ tokens: 100, cost_usd: 1 }, new Date(2026, 7, 1))).toBeNull();
  });
});

describe("CostSection byModel + WoW 渲染", () => {
  it("byModel 不为空时显示「按模型成本 Top10」表", () => {
    const { container } = render(
      <CostSection
        dirCosts={[]} byProvider={[]}
        byModel={[
          { model: "claude-sonnet-4-5", provider_id: "claude-code", requests: 100, input_tokens: 5000, output_tokens: 2000, errors: 1, cost_usd: 30.5 },
        ]}
        budget={{ monthly_token_limit: null, monthly_cost_limit: null, notify_on_exceed: false }}
        summary={null}
        monthUsage={null}
        budgetInput={{ tokens: "", cost: "" }}
        loading={false}
        onBudgetInput={() => {}} onSaveBudget={() => {}} onRecalc={() => {}}
      />,
    );
    expect(container.textContent).toContain("按模型成本 Top10");
    expect(container.textContent).toContain("claude-sonnet-4-5");
  });

  it("timeseries 14 天 → 显示 WoW 卡", () => {
    const now = Date.now();
    const ts = Array.from({ length: 14 }, (_, i) => ({
      day: new Date(now - i * 86_400_000).toISOString().slice(0, 10),
      total_tokens: 1_000_000,
      requests: 100,
    }));
    const { container } = render(
      <CostSection
        dirCosts={[]} byProvider={[]}
        timeseries={ts}
        budget={{ monthly_token_limit: null, monthly_cost_limit: null, notify_on_exceed: false }}
        summary={null}
        monthUsage={null}
        budgetInput={{ tokens: "", cost: "" }}
        loading={false}
        onBudgetInput={() => {}} onSaveBudget={() => {}} onRecalc={() => {}}
      />,
    );
    expect(container.textContent).toContain("本周 vs 上周");
  });
});

describe("prefs 格式化函数", () => {
  it("formatCostPref USD vs CNY 转换", () => {
    expect(formatCostPref(10, "USD")).toBe("$10.00");
    expect(formatCostPref(10, "CNY")).toBe("¥72.00");
  });

  it("formatCostPref 小数精度", () => {
    expect(formatCostPref(0.0123, "USD")).toBe("$0.012");
  });

  it("formatTokensPref raw 模式带千分位", () => {
    expect(formatTokensPref(1234567, "raw")).toBe("1,234,567");
  });

  it("formatTokensPref k 模式自动 M/K", () => {
    expect(formatTokensPref(1500, "k")).toBe("1.5K");
    expect(formatTokensPref(2_500_000, "k")).toBe("2.5M");
    expect(formatTokensPref(500, "k")).toBe("500");
  });

  it("formatTokensPref wan / yi 模式", () => {
    expect(formatTokensPref(15000, "wan")).toBe("1.5万");
    expect(formatTokensPref(150_000_000, "wan")).toBe("1.50亿");
    expect(formatTokensPref(2_500_000_000, "yi")).toBe("2.50B");
  });

  it("formatTimePref null → —", () => {
    expect(formatTimePref(null, "relative")).toBe("—");
  });

  it("formatTimePref relative 模式", () => {
    const ms = Date.now() - 3 * 60 * 1000;
    expect(formatTimePref(ms, "relative")).toContain("分钟前");
  });

  it("formatTimePref absolute 模式 YYYY-MM-DD HH:mm", () => {
    const ms = new Date(2026, 7, 12, 14, 23, 45).getTime();
    const s = formatTimePref(ms, "absolute");
    expect(s).toBe("2026-08-12 14:23");
  });

  it("formatTimePref iso 模式返回 ISO 字符串", () => {
    const ms = new Date(2026, 7, 12, 14, 23, 45).getTime();
    const s = formatTimePref(ms, "iso");
    // toISOString 转 UTC，所以小时数可能不是 14
    expect(s).toMatch(/^2026-08-12T\d{2}:23:45$/);
  });
});

describe("ConversationList Pin 持久化", () => {
  beforeEach(() => localStorage.removeItem("ch-conv-pins"));
  afterEach(() => localStorage.removeItem("ch-conv-pins"));

  it("loadPinnedIds 默认空集合", () => {
    expect(loadPinnedIds().size).toBe(0);
  });

  it("localStorage 缓存往返", () => {
    localStorage.setItem("ch-conv-pins", JSON.stringify(["c1", "c2"]));
    const s = loadPinnedIds();
    expect(s.has("c1")).toBe(true);
    expect(s.has("c2")).toBe(true);
    expect(s.size).toBe(2);
  });
});

describe("ChangelogModal 启动检测", () => {
  beforeEach(() => localStorage.removeItem("ch-last-seen-version"));
  afterEach(() => localStorage.removeItem("ch-last-seen-version"));

  it("首次启动 shouldShowChangelog → true", () => {
    expect(shouldShowChangelog()).toBe(true);
  });

  it("标记已读后 → false", () => {
    markVersionSeen("0.1.0");
    expect(shouldShowChangelog()).toBe(false);
  });

  it("getLastSeenVersion 反映标记", () => {
    expect(getLastSeenVersion()).toBeNull();
    markVersionSeen("0.1.0");
    expect(getLastSeenVersion()).toBe("0.1.0");
  });

  it("ChangelogModal 渲染版本号 + highlights", () => {
    const { container } = render(<ChangelogModal onClose={() => {}} />);
    expect(container.textContent).toContain("v0.1.0");
    expect(container.querySelectorAll(".changelog-list li").length).toBeGreaterThan(5);
  });

  it("ChangelogModal 点关闭 → 标记已读 + onClose 触发", () => {
    const onClose = vi.fn();
    const { container } = render(<ChangelogModal onClose={onClose} />);
    const closeBtn = container.querySelector(".settings-close") as HTMLButtonElement;
    fireEvent.click(closeBtn);
    expect(onClose).toHaveBeenCalled();
    expect(getLastSeenVersion()).toBe("0.1.0");
  });
});

describe("ActivityView 工具维度筛选", () => {
  it("有 tool_daily 时显示「指定工具」select", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      const sel = container.querySelector(".heat-tool-select");
      expect(sel).toBeInTheDocument();
    });
  });

  it("默认「全部工具」→ 热力图全量", async () => {
    const { container } = render(<ActivityView />);
    await waitFor(() => {
      const cells = container.querySelectorAll(".heat-cell");
      expect(cells.length).toBeGreaterThan(0);
    });
  });
});
