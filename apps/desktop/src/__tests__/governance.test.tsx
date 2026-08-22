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
    showToast("预算超限", "error", 5000);
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
  it("带 undo 回调：toast 携带 undo 函数 + 默认文本「撤销」", () => {
    const undo = vi.fn();
    showToast("已删除 3 条", "info", 6000, undo);
    const t = getToasts()[0];
    expect(t.undo).toBe(undo);
    expect(t.undoLabel).toBe("撤销");
  });
  it("带 undo + 自定义文本", () => {
    const undo = vi.fn();
    showToast("已重置", "warn", 6000, undo, "撤销重置");
    const t = getToasts()[0];
    expect(t.undoLabel).toBe("撤销重置");
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

describe("自动化任务状态分桶（174 条历史任务误判「进行中」修复）", () => {
  it("运行中：running / active / queued 等", async () => {
    const { classifyAutomation } = await import("../AssetsSection");
    expect(classifyAutomation("running")).toBe("running");
    expect(classifyAutomation("ACTIVE")).toBe("running");
    expect(classifyAutomation("in_progress")).toBe("running");
  });
  it("已启用：configured / enabled·trusted 等配置态（非执行态）", async () => {
    const { classifyAutomation } = await import("../AssetsSection");
    expect(classifyAutomation("configured")).toBe("configured");
    expect(classifyAutomation("enabled·trusted")).toBe("configured");
    expect(classifyAutomation("enabled·untrusted")).toBe("configured");
  });
  it("已结束：finished / completed / disabled / failed 及 null", async () => {
    const { classifyAutomation } = await import("../AssetsSection");
    expect(classifyAutomation("finished")).toBe("ended");
    expect(classifyAutomation("completed")).toBe("ended");
    expect(classifyAutomation("disabled")).toBe("ended");
    expect(classifyAutomation("failed")).toBe("ended");
    // 无状态的历史目录不得默认算活动
    expect(classifyAutomation(null)).toBe("ended");
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
        messages={messages} events={events} completenessLabel=""
        loading={false} exporting={false} timelineMode highlightMsgId={null}
        collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}} 
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

describe("toast 上限（防刷屏）", () => {
  beforeEach(() => _resetToasts());
  it("最多保留 4 条，溢出丢最旧", () => {
    for (let i = 1; i <= 6; i++) showToast(`t${i}`);
    const list = getToasts();
    expect(list.length).toBe(4);
    expect(list[0].text).toBe("t3"); // t1/t2 被丢弃
    expect(list[3].text).toBe("t6");
  });
});
