// 第 5 轮大改版测试：报告收藏/搜索、About 面板、HelpShortcuts、Search 历史、Cost projection
import { fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// 全局 mock invoke（多场景返回不同数据）
const sampleReports = [
  { name: "weekly-2026-07-29.html", size: 12_345, mtime_ms: Date.now() - 86_400_000 * 2 },
  { name: "weekly-2026-08-05.html", size: 23_456, mtime_ms: Date.now() - 86_400_000 * 1 },
  { name: "weekly-2026-08-12.html", size: 18_900, mtime_ms: Date.now() - 3600_000 * 4 },
];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "list_reports") return sampleReports;
    if (cmd === "ops_weekly_report") return "<h1>本周报</h1>";
    if (cmd === "read_report") return "<h1>历史</h1>";
    if (cmd === "ops_by_provider") return [
      { provider: "claude-code", requests: 10, total_tokens: 1000, output_tokens: 500, errors: 1, cost_usd: 5.2 },
      { provider: "zcode", requests: 5, total_tokens: 200, output_tokens: 100, errors: 0, cost_usd: 1.1 },
    ];
    return null;
  }),
}));

import ReportModal, { loadReportFavs } from "../ReportModal";
import HelpShortcuts from "../HelpShortcuts";
import { APP_VERSION, CORE_VERSION } from "../SettingsView";
import { calcProjection } from "../CostSection";
import CostSection from "../CostSection";
import AssetsSection from "../AssetsSection";
import SecuritySection from "../SecuritySection";
import type { AuditReport, PolicyRule } from "../ops-types";

describe("ReportModal 历史报告搜索 + 收藏", () => {
  beforeEach(() => localStorage.removeItem("ch-report-favs"));
  afterEach(() => localStorage.removeItem("ch-report-favs"));

  it("默认展示全部历史报告", async () => {
    const { container } = render(<ReportModal onClose={() => {}} />);
    await waitFor(() => {
      const items = container.querySelectorAll(".report-history-item");
      expect(items.length).toBe(3);
    });
  });

  it("关键词过滤生效（子串不区分大小写）", async () => {
    const { container } = render(<ReportModal onClose={() => {}} />);
    await waitFor(() => expect(container.querySelectorAll(".report-history-item").length).toBe(3));
    const input = container.querySelector(".report-search") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "08-12" } });
    await waitFor(() => {
      const items = container.querySelectorAll(".report-history-item");
      expect(items.length).toBe(1);
      expect(items[0].textContent).toContain("2026-08-12");
    });
  });

  it("无匹配关键词显示空态文案", async () => {
    const { container } = render(<ReportModal onClose={() => {}} />);
    await waitFor(() => expect(container.querySelectorAll(".report-history-item").length).toBe(3));
    const input = container.querySelector(".report-search") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "xxx-no-match" } });
    await waitFor(() => {
      const items = container.querySelectorAll(".report-history-item");
      expect(items.length).toBe(0);
      expect(container.textContent).toContain("没有匹配");
    });
  });

  it("收藏按钮切换并写入 localStorage", async () => {
    const { container } = render(<ReportModal onClose={() => {}} />);
    await waitFor(() => expect(container.querySelectorAll(".report-history-item").length).toBe(3));
    const firstFav = container.querySelectorAll(".report-fav-btn")[0] as HTMLButtonElement;
    fireEvent.click(firstFav);
    await waitFor(() => {
      expect(firstFav.textContent).toBe("★");
      const stored = JSON.parse(localStorage.getItem("ch-report-favs") ?? "[]") as string[];
      expect(stored.length).toBe(1);
      expect(stored[0]).toBe("weekly-2026-07-29.html");
    });
    // loadReportFavs 同步反映
    expect(loadReportFavs().has("weekly-2026-07-29.html")).toBe(true);
    // 再点取消
    fireEvent.click(firstFav);
    await waitFor(() => {
      expect(loadReportFavs().has("weekly-2026-07-29.html")).toBe(false);
    });
  });

  it("「仅收藏」过滤生效", async () => {
    // 预置收藏 1 条
    localStorage.setItem("ch-report-favs", JSON.stringify(["weekly-2026-08-05.html"]));
    const { container } = render(<ReportModal onClose={() => {}} />);
    await waitFor(() => expect(container.querySelectorAll(".report-history-item").length).toBe(3));
    const favChip = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes("收藏")) as HTMLButtonElement;
    fireEvent.click(favChip);
    await waitFor(() => {
      const items = container.querySelectorAll(".report-history-item");
      expect(items.length).toBe(1);
      expect(items[0].textContent).toContain("2026-08-05");
    });
  });

  it("点击历史项触发 read_report invoke", async () => {
    const { container } = render(<ReportModal onClose={() => {}} />);
    await waitFor(() => expect(container.querySelectorAll(".report-history-item").length).toBe(3));
    const openBtn = container.querySelectorAll(".report-open-btn")[1] as HTMLButtonElement;
    fireEvent.click(openBtn);
    await waitFor(() => {
      const iframe = container.querySelector(".report-frame") as HTMLIFrameElement;
      expect(iframe.getAttribute("srcdoc")).toBe("<h1>历史</h1>");
    });
  });
});

describe("HelpShortcuts 组件", () => {
  it("渲染所有分组与至少 15 条快捷键", () => {
    const { container } = render(<HelpShortcuts onClose={() => {}} />);
    const groups = container.querySelectorAll(".help-shortcuts-group");
    expect(groups.length).toBeGreaterThanOrEqual(3);
    const rows = container.querySelectorAll(".help-shortcuts-row");
    expect(rows.length).toBeGreaterThanOrEqual(15);
  });

  it("必含 ⌘K / ⌘? / ⌘1..8", () => {
    const { container } = render(<HelpShortcuts onClose={() => {}} />);
    const text = container.textContent ?? "";
    expect(text).toMatch(/K/);
    expect(text).toContain("?");
    for (let i = 1; i <= 8; i++) {
      expect(container.textContent).toContain(String(i));
    }
  });

  it("点 backdrop 触发 onClose", () => {
    const onClose = vi.fn();
    const { container } = render(<HelpShortcuts onClose={onClose} />);
    const backdrop = container.querySelector(".settings-backdrop") as HTMLDivElement;
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalled();
  });

  it("Esc 关闭（onClose 被调用）", () => {
    const onClose = vi.fn();
    render(<HelpShortcuts onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});

describe("SettingsView About 元数据", () => {
  it("APP_VERSION 与 CORE_VERSION 来自正确来源", async () => {
    // 契约：构建期从 package.json / workspace Cargo.toml 派生，永不手工同步
    const pkg = (await import("../../package.json")).default;
    expect(APP_VERSION).toBe(pkg.version);
    const cargo = (await import("../../../../Cargo.toml?raw")).default;
    const m = cargo.match(/^version\s*=\s*"([^"]+)"/m);
    expect(CORE_VERSION).toBe(m?.[1]);
    // 回归：GUI 真人测试发现的版本漂移（曾硬编码 0.1.0/0.2.0 忘更新）
    expect(CORE_VERSION).toBe(APP_VERSION);
  });
});

describe("CostSection 月末预测 calcProjection", () => {
  it("monthUsage 为 null → 返回 null", () => {
    expect(calcProjection(null, new Date(2026, 7, 15))).toBeNull();
  });

  it("dayOfMonth < 2 → 数据不足返回 null", () => {
    expect(calcProjection({ tokens: 100, cost_usd: 1 }, new Date(2026, 7, 1))).toBeNull();
  });

  it("按当前速率外推到整月（day=15 / 31）", () => {
    const r = calcProjection({ tokens: 1000, cost_usd: 50 }, new Date(2026, 7, 15));
    expect(r).not.toBeNull();
    // 1000 / (15/31) = 2066.67
    expect(r!.tokens).toBeCloseTo(1000 / (15 / 31), 1);
    expect(r!.cost).toBeCloseTo(50 / (15 / 31), 1);
    expect(r!.dayOfMonth).toBe(15);
    expect(r!.daysInMonth).toBe(31); // 2026-08 = 31 天
  });

  it("月底（day=31）→ 预测 = 当前用量", () => {
    const r = calcProjection({ tokens: 8000, cost_usd: 400 }, new Date(2026, 0, 31));
    expect(r).not.toBeNull();
    expect(r!.tokens).toBeCloseTo(8000, 5);
    expect(r!.cost).toBeCloseTo(400, 5);
    expect(r!.daysInMonth).toBe(31);
  });

  it("2 月（28 天）处理", () => {
    const r = calcProjection({ tokens: 100, cost_usd: 1 }, new Date(2026, 1, 14));
    expect(r).not.toBeNull();
    expect(r!.daysInMonth).toBe(28);
    expect(r!.tokens).toBeCloseTo(200, 5);
  });
});

describe("CostSection 月末预测卡片渲染", () => {
  it("数据 + 预算 → 显示预测卡", () => {
    // 8 月中旬 dayOfMonth=15 >= 2 一定有 projection
    const { container } = render(
      <CostSection
        dirCosts={[]} byProvider={[]}
        budget={{ monthly_token_limit: 1000, monthly_cost_limit: 100, notify_on_exceed: true }}
        summary={null}
        monthUsage={{ tokens: 200, cost_usd: 20 }}
        budgetInput={{ tokens: "1000", cost: "100" }}
        loading={false}
        onBudgetInput={() => {}} onSaveBudget={() => {}} onRecalc={() => {}}
      />,
    );
    const card = container.querySelector(".projection-card");
    expect(card).toBeInTheDocument();
  });

  it("无 monthUsage → 不显示预测卡", () => {
    const { container } = render(
      <CostSection
        dirCosts={[]} byProvider={[]}
        budget={{ monthly_token_limit: 1000, monthly_cost_limit: 100, notify_on_exceed: true }}
        summary={null}
        monthUsage={null}
        budgetInput={{ tokens: "1000", cost: "100" }}
        loading={false}
        onBudgetInput={() => {}} onSaveBudget={() => {}} onRecalc={() => {}}
      />,
    );
    expect(container.querySelector(".projection-card")).toBeNull();
  });

  it("byProvider 含 cost_usd → 显示 Provider 维度 BarChart", () => {
    const { container } = render(
      <CostSection
        dirCosts={[]} byProvider={[
          { provider: "claude-code", requests: 10, total_tokens: 1000, output_tokens: 500, errors: 0, cost_usd: 5.2 },
          { provider: "zcode", requests: 5, total_tokens: 200, output_tokens: 100, errors: 0, cost_usd: 1.1 },
        ]}
        budget={{ monthly_token_limit: null, monthly_cost_limit: null, notify_on_exceed: false }}
        summary={null}
        monthUsage={null}
        budgetInput={{ tokens: "", cost: "" }}
        loading={false}
        onBudgetInput={() => {}} onSaveBudget={() => {}} onRecalc={() => {}}
      />,
    );
    expect(container.textContent).toContain("按 Agent（Provider）成本分布");
    expect(container.querySelector(".barchart")).toBeInTheDocument();
  });
});

describe("AssetsSection 详情弹窗", () => {
  const sampleAsset = {
    provider: "claude-code",
    kind: "plugin",
    name: "code-review-helper",
    version: "1.2.3",
    description: "自动代码审查",
    risky_hits: 2,
    installed_at: "2026-08-01",
    path: "/Users/me/.claude/skills/code-review-helper",
  };

  it("点击资产卡片 → 弹窗出现 + 路径/版本/风险点可读", () => {
    const { container } = render(<AssetsSection assets={[sampleAsset]} automations={[]} loading={false} />);
    const item = container.querySelector(".asset-item") as HTMLElement;
    fireEvent.click(item);
    const modal = container.querySelector(".asset-detail-modal");
    expect(modal).toBeInTheDocument();
    const text = modal?.textContent ?? "";
    expect(text).toContain("code-review-helper");
    expect(text).toContain("v1.2.3");
    expect(text).toContain("/Users/me/.claude/skills/code-review-helper");
    expect(text).toContain("2 处风险");
  });

  it("弹窗关闭按钮生效", () => {
    const { container } = render(<AssetsSection assets={[sampleAsset]} automations={[]} loading={false} />);
    fireEvent.click(container.querySelector(".asset-item") as HTMLElement);
    const close = container.querySelector(".asset-detail-modal .settings-close") as HTMLButtonElement;
    fireEvent.click(close);
    expect(container.querySelector(".asset-detail-modal")).toBeNull();
  });

  it("按 Provider 分组展示", () => {
    const { container } = render(
      <AssetsSection
        assets={[
          sampleAsset,
          { ...sampleAsset, provider: "zcode", name: "skill-a", risky_hits: 0 },
        ]}
        automations={[]}
        loading={false}
      />,
    );
    const groups = container.querySelectorAll(".asset-group");
    expect(groups.length).toBe(2);
  });
});

describe("SecuritySection 批量处置 + 策略导入导出", () => {
  const sampleAudit: AuditReport = {
    generated_at: "2026-08-12T00:00:00Z",
    scanned_messages: 1000,
    scanned_tool_calls: 50,
    findings: [
      { fingerprint: "fp-1", kind: "dangerous_command", severity: "high", rule: "rm -rf", provider: "claude-code", source_conversation_id: "c1", conversation_title: "X", message_id: "m1", tool_call_id: "t1", snippet: "rm -rf /" },
      { fingerprint: "fp-2", kind: "sensitive", severity: "medium", rule: "api_key", provider: "zcode", source_conversation_id: "c2", conversation_title: "Y", message_id: "m2", tool_call_id: null, snippet: "sk-xxxx" },
    ],
    high: 1, medium: 1, low: 0,
  };
  const samplePolicies: PolicyRule[] = [
    { id: "p1", name: "no-rm-rf", pattern: "rm\\s+-rf", kind: "dangerous_command", severity: "high", enabled: true },
    { id: "p2", name: "no-secret", pattern: "sk-[A-Za-z0-9]+", kind: "sensitive", severity: "medium", enabled: true },
  ];

  const baseProps = (overrides: Partial<React.ComponentProps<typeof SecuritySection>> = {}) => ({
    anomalies: [], audit: sampleAudit, auditing: false, auditKindFilter: "all" as const,
    policies: samplePolicies, newPolicy: { name: "", pattern: "", kind: "dangerous_command", severity: "high" },
    risky: [], expandedRisk: new Set<string>(), loading: false,
    onScan: () => {}, onExportHtml: () => {}, onFilter: () => {},
    onAddPolicy: () => {}, onRemovePolicy: () => {}, onPolicyInput: () => {},
    onTogglePolicyEnabled: () => {}, onDisposeFinding: () => {},
    onBulkDisposeFindings: () => {}, onRefreshAfterDispose: () => {},
    onToggleRisk: () => {}, onImportPolicies: () => {},
    onJump: () => {},
    ...overrides,
  });

  it("「全部忽略」触发 onBulkDisposeFindings，fingerprints 全部传入", () => {
    const onBulk = vi.fn();
    const { container } = render(<SecuritySection {...baseProps({ onBulkDisposeFindings: onBulk })} />);
    const btn = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes("全部忽略")) as HTMLButtonElement;
    expect(btn).toBeTruthy();
    fireEvent.click(btn);
    expect(onBulk).toHaveBeenCalledWith(["fp-1", "fp-2"], "ignored");
  });

  it("「全部误报」触发 onBulkDisposeFindings，状态为 false_positive", () => {
    const onBulk = vi.fn();
    const { container } = render(<SecuritySection {...baseProps({ onBulkDisposeFindings: onBulk })} />);
    const btn = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes("全部误报")) as HTMLButtonElement;
    fireEvent.click(btn);
    expect(onBulk).toHaveBeenCalledWith(["fp-1", "fp-2"], "false_positive");
  });

  it("「导入」按钮 prompt 取出 JSON → 调用 onImportPolicies", () => {
    const onImport = vi.fn();
    const promptSpy = vi.spyOn(window, "prompt").mockReturnValue('[{"id":"p3","name":"x","pattern":"y","kind":"dangerous_command","severity":"high","enabled":true}]');
    const { container } = render(<SecuritySection {...baseProps({ onImportPolicies: onImport })} />);
    const btn = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "⤒ 导入") as HTMLButtonElement;
    fireEvent.click(btn);
    expect(promptSpy).toHaveBeenCalled();
    expect(onImport).toHaveBeenCalledWith(expect.stringContaining("p3"));
  });

  it("导入 prompt 取消 → onImportPolicies 不被调用", () => {
    const onImport = vi.fn();
    const promptSpy = vi.spyOn(window, "prompt").mockReturnValue(null);
    const { container } = render(<SecuritySection {...baseProps({ onImportPolicies: onImport })} />);
    const btn = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "⤒ 导入") as HTMLButtonElement;
    fireEvent.click(btn);
    expect(promptSpy).toHaveBeenCalled();
    expect(onImport).not.toHaveBeenCalled();
  });

  it("空 findings → 全部忽略按钮仍渲染但 onBulk 收到空数组", () => {
    const emptyAudit: AuditReport = { ...sampleAudit, findings: [], high: 0, medium: 0, low: 0 };
    const onBulk = vi.fn();
    const { container } = render(
      <SecuritySection {...baseProps({ audit: emptyAudit, onBulkDisposeFindings: onBulk })} />,
    );
    // 空 findings → 整体空态文案出现，bulk 按钮不出现
    expect(container.textContent).toContain("扫描完成，未发现未处置风险");
    const allIgnore = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes("全部忽略"));
    expect(allIgnore).toBeUndefined();
  });
});
