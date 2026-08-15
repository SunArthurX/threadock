// 治理闭环前端测试：预算状态 / toast store / 卡片显隐 / 时间线归并
import { describe, expect, it, vi, beforeEach } from "vitest";
import { budgetState } from "../BudgetBar";
import { _resetToasts, dismissToast, getToasts, showToast, subscribeToasts } from "../toast";
import { loadHiddenCards, toggleHiddenCard, CARD_KEYS } from "../OverviewSection";
import { loadAutomationWatch, toggleAutomationWatch } from "../AssetsSection";

describe("budgetState 预算状态判定", () => {
  const base = {
    costSoFar: 10, tokensSoFar: 1000,
    projectedCost: 30, projectedTokens: 3000,
    costLimit: 50, tokenLimit: 5000,
  };
  it("预算内 → ok", () => {
    expect(budgetState(base)).toBe("ok");
  });
  it("外推超限 → warning（提前预警）", () => {
    expect(budgetState({ ...base, projectedCost: 60 })).toBe("warning");
    expect(budgetState({ ...base, projectedTokens: 6000 })).toBe("warning");
  });
  it("已超限 → over", () => {
    expect(budgetState({ ...base, costSoFar: 55 })).toBe("over");
    expect(budgetState({ ...base, tokensSoFar: 6000 })).toBe("over");
  });
  it("未设预算 → ok", () => {
    expect(budgetState({ ...base, costLimit: null, tokenLimit: null })).toBe("ok");
  });
});

describe("toast store", () => {
  beforeEach(() => _resetToasts());
  it("弹出与自动消失（fake timers）", async () => {
    vi.useFakeTimers();
    showToast("预算超限", "error");
    expect(getToasts().length).toBe(1);
    expect(getToasts()[0].text).toBe("预算超限");
    vi.advanceTimersByTime(5100);
    expect(getToasts().length).toBe(0);
    vi.useRealTimers();
  });
  it("手动关闭与订阅通知", () => {
    const cb = vi.fn();
    const unsub = subscribeToasts(cb);
    const id = showToast("a");
    expect(cb).toHaveBeenCalled();
    dismissToast(id);
    expect(getToasts().length).toBe(0);
    unsub();
  });
});

describe("卡片显隐持久化", () => {
  beforeEach(() => localStorage.removeItem("ch-cards-hidden"));
  it("切换后 loadHiddenCards 反映", () => {
    expect(loadHiddenCards().size).toBe(0);
    const next = toggleHiddenCard("cache");
    expect(next.has("cache")).toBe(true);
    expect(loadHiddenCards().has("cache")).toBe(true);
    toggleHiddenCard("cache");
    expect(loadHiddenCards().size).toBe(0);
  });
  it("CARD_KEYS 覆盖全部概览卡片", () => {
    expect(CARD_KEYS.length).toBeGreaterThanOrEqual(10);
  });
});

describe("自动化关注持久化", () => {
  beforeEach(() => localStorage.removeItem("ch-automation-watch"));
  it("toggle 往返", () => {
    const next = toggleAutomationWatch("zcode:daily-report");
    expect(next.has("zcode:daily-report")).toBe(true);
    expect(loadAutomationWatch().has("zcode:daily-report")).toBe(true);
    toggleAutomationWatch("zcode:daily-report");
    expect(loadAutomationWatch().size).toBe(0);
  });
});

describe("时间线归并排序（M15 修复验证）", () => {
  it("消息与事件按时间排序且不再截断 100", async () => {
    const { default: ConversationDetail } = await import("../ConversationDetail");
    const { render } = await import("@testing-library/react");
    const base = (i: number) => ({ id: `m${i}`, role: "user", content_text: `msg ${i}`, sequence_number: i, created_at_ms: 1000 + i });
    const messages = Array.from({ length: 150 }, (_, i) => base(i));
    // 事件时间早于全部消息 → 必须排在最前
    const events = [{ id: "e1", created_at_ms: 500, event_type: "command_started", summary: "最早事件", sequence_number: 1 }];
    const { container } = render(
      <ConversationDetail
        conv={{ id: "c1", provider: "zcode", source_conversation_id: "s", title: null, user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, started_at_ms: null, updated_at_ms: null, source_parent_id: null, child_count: 0 }}
        messages={messages} events={events} completenessLabel="" knowledge={null}
        loading={false} exporting={false} timelineMode highlightMsgId={null}
        collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}} onToggleFavorite={() => {}} onToggleArchive={() => {}}
        onAddTag={() => {}}
        onRemoveTag={() => {}} onRescanAudit={() => {}}
      />
    );
    const items = container.querySelectorAll(".tl-item");
    expect(items.length).toBe(151); // 150 消息 + 1 事件（旧实现截断 100）
    // 第一项是时间最早的事件
    expect(items[0].className).toContain("tl-event");
    expect(container.querySelector(".tl-time")?.textContent).not.toBe(""); // 事件时间不再为空
  });
});
