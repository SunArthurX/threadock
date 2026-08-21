// 第 12 轮测试：顶栏精简 + Dropdown label + Resizer 拖拽 + 详情页居中
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, beforeEach, vi } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import Resizer, { loadClampedNumber, saveNumber } from "../Resizer";
import ConversationList from "../ConversationList";
import type { Conversation } from "../types";

/** 定位到 apps/desktop/src 目录（不依赖 @types/node） */
const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC_DIR = path.resolve(HERE, "..");
const APP_TSX = path.resolve(SRC_DIR, "App.tsx");
const STYLES_CSS = path.resolve(SRC_DIR, "styles.css");
const readSrc = (p: string) => fs.readFileSync(p, "utf-8");

beforeEach(() => { localStorage.clear(); vi.restoreAllMocks(); });

const convs: Conversation[] = [
  { id: "c1", provider: "zcode", source_conversation_id: "sc1", title: "Conv1", user_title: null, status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null, started_at_ms: Date.now() - 3_600_000, updated_at_ms: Date.now() - 3_600_000, child_count: 0, favorite: false, archived: false },
];
function makeProps(extra: Partial<React.ComponentProps<typeof ConversationList>> = {}) {
  return {
    conversations: convs, selectedConv: null, loading: false, providerFilter: null, selectedWs: null,
    expandedParents: new Set<string>(), childConvs: {} as Record<string, Conversation[]>,
    scope: "all" as const, onScopeChange: () => {}, onFilter: () => {}, onSelect: () => {},
    onToggleExpand: () => {}, onClearWs: () => {}, onToggleFavorite: () => {},
    ...extra,
  };
}

describe("Resizer 通用拖拽条", () => {
  it("渲染 .resizer 元素（role=separator）", () => {
    const { container } = render(<Resizer onDrag={() => {}} />);
    const r = container.querySelector('[role="separator"]');
    expect(r).toBeTruthy();
    expect(r?.classList.contains("resizer")).toBe(true);
  });

  it("mousedown 触发 active 态 + body cursor: col-resize", () => {
    const { container } = render(<Resizer onDrag={() => {}} />);
    const r = container.querySelector(".resizer") as HTMLElement;
    fireEvent.mouseDown(r, { clientX: 100 });
    expect(r.classList.contains("active")).toBe(true);
    expect(document.body.style.cursor).toBe("col-resize");
    // 清理
    fireEvent.mouseUp(document);
    expect(document.body.style.cursor).toBe("");
  });

  it("mousemove 触发 onDrag + 传入 dx", () => {
    const onDrag = vi.fn();
    const { container } = render(<Resizer onDrag={onDrag} />);
    const r = container.querySelector(".resizer") as HTMLElement;
    fireEvent.mouseDown(r, { clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 120 });
    expect(onDrag).toHaveBeenCalled();
    expect(onDrag.mock.calls[0]![0]).toBeCloseTo(20, 0);
    fireEvent.mouseUp(document);
  });

  it("mouseup 后 onDrag 不再触发", () => {
    const onDrag = vi.fn();
    const { container } = render(<Resizer onDrag={onDrag} />);
    const r = container.querySelector(".resizer") as HTMLElement;
    fireEvent.mouseDown(r, { clientX: 100 });
    fireEvent.mouseUp(document);
    fireEvent.mouseMove(document, { clientX: 200 });
    expect(onDrag).not.toHaveBeenCalled();
  });
});

describe("Resizer loadClampedNumber / saveNumber 工具", () => {
  it("loadClampedNumber 默认值 + 范围 clamp", () => {
    expect(loadClampedNumber("none", 100, 50, 200)).toBe(100);
    localStorage.setItem("x", "300");
    expect(loadClampedNumber("x", 100, 50, 200)).toBe(100); // 超 max → fallback
    localStorage.setItem("x", "30");
    expect(loadClampedNumber("x", 100, 50, 200)).toBe(100); // 低于 min → fallback
    localStorage.setItem("x", "120");
    expect(loadClampedNumber("x", 100, 50, 200)).toBe(120); // 范围内
  });

  it("loadClampedNumber localStorage 抛错时返回 fallback", () => {
    const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("quota"); });
    expect(loadClampedNumber("x", 100, 50, 200)).toBe(100);
    spy.mockRestore();
  });

  it("saveNumber 写 localStorage + 异常静默", () => {
    saveNumber("k", 200);
    expect(localStorage.getItem("k")).toBe("200");
    const spy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("quota"); });
    expect(() => saveNumber("k", 200)).not.toThrow();
    spy.mockRestore();
  });
});

describe("ConversationList Dropdown 带 label", () => {
  it("3 个 dropdown 按钮各带「视图/日期/排序」小 label", () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const labels = [...container.querySelectorAll(".list-dropdown-label-text")].map((l) => l.textContent);
    expect(labels).toEqual(["视图", "日期", "排序"]);
  });

  it("label 元素位于 dropdown 容器内、按钮之前", () => {
    const { container } = render(<ConversationList {...makeProps()} />);
    const dropdowns = container.querySelectorAll(".list-dropdown");
    const sortLabel = dropdowns[2]?.querySelector(".list-dropdown-label-text");
    const sortBtn = dropdowns[2]?.querySelector(".list-dropdown-btn");
    expect(sortLabel).toBeTruthy();
    expect(sortBtn).toBeTruthy();
    // label 在 DOM 中位置早于 btn
    if (sortLabel && sortBtn) {
      const pos = sortLabel.compareDocumentPosition(sortBtn);
      expect(pos & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
  });
});

describe("App 顶栏：保留 ⌘K / ? / ⚙（⌘K 是命令面板核心入口，保留）", () => {
  it("顶栏保留 命令面板 / 快捷键速查 / 设置 三个按钮（macOS 标准）", async () => {
    const src = readSrc(APP_TSX);
    expect(/title="命令面板[^"]*"/.test(src)).toBe(true);
    expect(/title="快捷键速查[^"]*"/.test(src)).toBe(true);
    expect(/title="设置"/.test(src)).toBe(true);
  });
});

describe("Resizer 在 App 顶栏已部署（DOM 存在 .sidebar-resizer）", () => {
  // 单元测试只测组件；DOM 集成由 puppeteer / e2e 验证
  it("Resizer 默认 props（className=title）渲染", () => {
    const { container } = render(<Resizer onDrag={() => {}} title="拖拽调整侧边栏宽度" />);
    expect(container.querySelector(".resizer")?.getAttribute("title")).toBe("拖拽调整侧边栏宽度");
  });
});

describe("详情页 .detail max-width 居中（CSS 规则）", () => {
  it("styles.css 中 .detail 包含 max-width: 920px + margin: 0 auto", async () => {
    const css = readSrc(STYLES_CSS);
    // 找到 .detail { 块
    const m = /\.detail\s*\{[^}]*max-width:\s*920px[^}]*margin:\s*0\s*auto[^}]*\}/m.exec(css);
    expect(m).toBeTruthy();
  });
});

describe("详情页 / 概览 / 成本等页 max-width 自适应", () => {
  it("styles.css 中 .ops-view max-width: 1080px + margin: 0 auto", async () => {
    const css = readSrc(STYLES_CSS);
    const m = /\.ops-view\s*\{[^}]*max-width:\s*1080px[^}]*margin:\s*0\s*auto/m.exec(css);
    expect(m).toBeTruthy();
  });

  it("styles.css 中 .knowledge-page / .activity-page / .projects-page max-width + align-self: center", async () => {
    const css = readSrc(STYLES_CSS);
    const m = /\.knowledge-page,\s*\.activity-page,\s*\.projects-page\s*\{[^}]*max-width:\s*1180px[^}]*align-self:\s*center/m.exec(css);
    expect(m).toBeTruthy();
  });
});

describe("全局滚动条样式（webkit）", () => {
  it("styles.css 中包含 ::-webkit-scrollbar 规则", async () => {
    const css = readSrc(STYLES_CSS);
    expect(css.includes("::-webkit-scrollbar")).toBe(true);
    expect(css.includes("scrollbar-width: thin")).toBe(true);
  });

  it("styles.css 中包含 :focus-visible 焦点环", async () => {
    const css = readSrc(STYLES_CSS);
    expect(css.includes(":focus-visible")).toBe(true);
  });
});
