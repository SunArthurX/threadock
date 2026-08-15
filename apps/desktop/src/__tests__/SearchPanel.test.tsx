// 组件冒烟 + XSS 回归：后端已转义的 snippet 在前端只呈现为文本
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import SearchPanel from "../SearchPanel";
import type { SearchResult } from "../types";

const mk = (snippet: string): SearchResult => ({
  message_id: "m1",
  conversation_id: "c1",
  provider: "zcode",
  role: "user",
  title: "测试会话",
  snippet,
});

describe("SearchPanel", () => {
  it("渲染结果计数与标题", () => {
    render(
      <SearchPanel results={[mk("普通片段")]} query="关键词" onJump={vi.fn()} />
    );
    expect(screen.getByText(/搜索结果 \(1\)/)).toBeTruthy();
    expect(screen.getByText("测试会话")).toBeTruthy();
  });

  it("空结果显示「无匹配」", () => {
    render(<SearchPanel results={[]} query="x" onJump={vi.fn()} />);
    expect(screen.getByText("无匹配")).toBeTruthy();
  });

  it("已转义的恶意 snippet 不产生可执行元素（XSS 回归防护）", () => {
    // 后端 snippet 已做 HTML 转义：&lt;img&gt; 应显示为文本，而非渲染 img 元素
    render(
      <SearchPanel
        results={[mk("前文 &lt;img src=x onerror=alert(1)&gt; 后文")]}
        query="x"
        onJump={vi.fn()}
      />
    );
    expect(document.querySelector("img")).toBeNull();
    expect(document.querySelector("script")).toBeNull();
    expect(screen.getByText(/onerror=alert/)).toBeTruthy();
  });

  it("高亮 <b> 标签正常渲染", () => {
    const { container } = render(
      <SearchPanel results={[mk("命中<b>关键词</b>高亮")]} query="关键词" onJump={vi.fn()} />
    );
    expect(container.querySelector(".snippet b")?.textContent).toBe("关键词");
  });
});
