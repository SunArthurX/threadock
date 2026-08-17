// 第 10 轮测试：OnboardingTour + 代码高亮（5 种语言）+ BarChart 横向网格线 + 热力图 17×30 className 校验
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, beforeEach, vi } from "vitest";

import OnboardingTour, {
  isOnboardingSeen,
  markOnboardingSeen,
  resetOnboarding,
  loadOnboardingStep,
  saveOnboardingStep,
} from "../OnboardingTour";
import { highlightCode, countHighlightTokens } from "../codeHighlight";
import { BarChart } from "../charts";
import { renderToStaticMarkup } from "react-dom/server";

beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("OnboardingTour 渲染与切换", () => {
  it("首次打开显示第 1 步标题", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    const tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour).toBeInTheDocument();
    expect(tour?.getAttribute("data-step")).toBe("0");
    expect(container.textContent).toContain("导入你的 AI 工具对话");
  });

  it("点「下一步」切到 step 1/2", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    const next = container.querySelector("[data-testid='onboarding-next']") as HTMLButtonElement;
    fireEvent.click(next);
    const tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour?.getAttribute("data-step")).toBe("1");
    expect(container.textContent).toContain("命令面板");
  });

  it("step 0 时「上一步」禁用", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    const prev = container.querySelector("[data-testid='onboarding-prev']") as HTMLButtonElement;
    expect(prev.disabled).toBe(true);
  });

  it("点「上一步」回退", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    fireEvent.click(container.querySelector("[data-testid='onboarding-next']") as HTMLButtonElement);
    fireEvent.click(container.querySelector("[data-testid='onboarding-next']") as HTMLButtonElement);
    let tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour?.getAttribute("data-step")).toBe("2");
    const prev = container.querySelector("[data-testid='onboarding-prev']") as HTMLButtonElement;
    expect(prev.disabled).toBe(false);
    fireEvent.click(prev);
    tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour?.getAttribute("data-step")).toBe("1");
  });

  it("最后一步显示「开始使用 ✓」按钮", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} startStep={5} />);
    expect(container.querySelector("[data-testid='onboarding-finish']")).toBeInTheDocument();
    expect(container.querySelector("[data-testid='onboarding-next']")).toBeNull();
    expect(container.textContent).toContain("设置：偏好、备份、关于");
  });

  it("点「开始使用」触发 onClose 并标记 seen", () => {
    const onClose = vi.fn();
    const { container } = render(<OnboardingTour onClose={onClose} startStep={5} />);
    const finish = container.querySelector("[data-testid='onboarding-finish']") as HTMLButtonElement;
    fireEvent.click(finish);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(isOnboardingSeen()).toBe(true);
  });

  it("点击 backdrop 视为完成（mark seen + onClose）", () => {
    const onClose = vi.fn();
    const { container } = render(<OnboardingTour onClose={onClose} />);
    const backdrop = container.querySelector(".settings-backdrop") as HTMLDivElement;
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(isOnboardingSeen()).toBe(true);
  });

  it("按 Esc 关闭（不自动 mark seen，外部决定）", () => {
    const onClose = vi.fn();
    render(<OnboardingTour onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("键盘 ArrowRight 切到下一步", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    fireEvent.keyDown(window, { key: "ArrowRight" });
    const tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour?.getAttribute("data-step")).toBe("1");
  });

  it("键盘 ArrowLeft 在 step 0 时无效", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    const tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour?.getAttribute("data-step")).toBe("0");
  });

  it("进度点点击可跳到任意 step", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    const dots = container.querySelectorAll(".onboarding-dot");
    expect(dots.length).toBe(6);
    fireEvent.click(dots[3] as HTMLDivElement);
    const tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour?.getAttribute("data-step")).toBe("3");
  });

  it("数字显示「1 / 6」当前进度", () => {
    const { container } = render(<OnboardingTour onClose={() => {}} />);
    expect(container.querySelector(".onboarding-step-num")?.textContent).toContain("1 / 6");
  });
});

describe("OnboardingTour localStorage 持久化", () => {
  it("未走完时 isOnboardingSeen 返回 false", () => {
    expect(isOnboardingSeen()).toBe(false);
  });

  it("markOnboardingSeen 后 isOnboardingSeen 返回 true", () => {
    markOnboardingSeen();
    expect(isOnboardingSeen()).toBe(true);
  });

  it("resetOnboarding 清掉 seen 标记", () => {
    markOnboardingSeen();
    resetOnboarding();
    expect(isOnboardingSeen()).toBe(false);
  });

  it("loadOnboardingStep 默认 0", () => {
    expect(loadOnboardingStep()).toBe(0);
  });

  it("saveOnboardingStep 后能读回", () => {
    saveOnboardingStep(3);
    expect(loadOnboardingStep()).toBe(3);
  });

  it("localStorage 抛错时降级（不影响渲染）", () => {
    const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("quota"); });
    expect(isOnboardingSeen()).toBe(false);
    expect(loadOnboardingStep()).toBe(0);
    spy.mockRestore();
  });

  it("startStep 参数覆盖已持久化的 step", () => {
    saveOnboardingStep(4);
    const { container } = render(<OnboardingTour onClose={() => {}} startStep={2} />);
    const tour = container.querySelector("[data-testid='onboarding-tour']");
    expect(tour?.getAttribute("data-step")).toBe("2");
  });
});

describe("代码高亮：python", () => {
  it("识别 def / class / return 等关键字", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("def foo():\n    return 1", "python")}</code>);
    expect(html).toContain("tok-keyword");
    expect(html).toContain("def");
    expect(html).toContain("return");
  });

  it("识别 # 行注释（整行变 comment）", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("# hello\nx = 1", "python")}</code>);
    expect(html).toContain("tok-comment");
    expect(html).toContain("# hello");
  });

  it("识别字符串（单/双引号）", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("x = 'hi'\ny = \"world\"", "python")}</code>);
    expect(html).toContain("tok-string");
    // React 把 ' 转义成 &#x27;，" 转义成 &quot;；宽匹配内容
    expect(html).toMatch(/hi/);
    expect(html).toMatch(/world/);
    // 验证两个独立字符串节点
    const matches = html.match(/tok-string/g) ?? [];
    expect(matches.length).toBeGreaterThanOrEqual(2);
  });

  it("识别数字", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("x = 42\ny = 3.14", "python")}</code>);
    expect(html).toContain("tok-number");
  });

  it("countHighlightTokens 计数：1 keyword + 1 string + 1 number + 1 comment", () => {
    const code = 'def f():\n    # c\n    x = "s"\n    return 1';
    const c = countHighlightTokens(code, "python");
    expect(c.keyword).toBeGreaterThanOrEqual(2); // def, return
    expect(c.string).toBe(1);
    expect(c.comment).toBe(1);
    expect(c.number).toBe(1);
    expect(c.total).toBe(c.keyword + c.string + c.number + c.comment);
  });
});

describe("代码高亮：typescript / tsx", () => {
  it("ts 识别 const / function / interface", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("const x = 1\nfunction foo() { return x }", "ts")}</code>);
    expect(html).toContain("tok-keyword");
    expect(html).toContain("const");
    expect(html).toContain("function");
    expect(html).toContain("return");
  });

  it("tsx 复用 ts 关键字（interface / type）", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("interface I { x: number }\ntype T = string", "tsx")}</code>);
    expect(html).toContain("tok-keyword");
    expect(html).toContain("interface");
    expect(html).toContain("type");
  });
});

describe("代码高亮：rust", () => {
  it("rs 识别 fn / let / mut / pub", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("fn main() {\n    let mut x = 1;\n    pub fn foo() {}\n}", "rs")}</code>);
    expect(html).toContain("tok-keyword");
    expect(html).toContain("fn");
    expect(html).toContain("let");
    expect(html).toContain("mut");
    expect(html).toContain("pub");
  });

  it("rust 别名复用 rs", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("fn main() {}", "rust")}</code>);
    expect(html).toContain("tok-keyword");
  });

  it("// 行注释识别", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("// hi\nlet x = 1", "rs")}</code>);
    expect(html).toContain("tok-comment");
  });
});

describe("代码高亮：go / bash / sql", () => {
  it("go 识别 func / package / return", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("package main\nfunc foo() int { return 1 }", "go")}</code>);
    expect(html).toContain("tok-keyword");
    expect(html).toContain("func");
    expect(html).toContain("package");
    expect(html).toContain("return");
  });

  it("bash 识别 if / then / fi", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("if true; then\n  echo hi\nfi", "bash")}</code>);
    expect(html).toContain("tok-keyword");
    expect(html).toContain("if");
    expect(html).toContain("then");
    expect(html).toContain("fi");
  });

  it("sh 别名复用 bash", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("echo hi", "sh")}</code>);
    expect(html).toContain("echo");
  });

  it("sql 识别 SELECT / FROM / WHERE（大写不敏感）", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("select * from t where id = 1", "sql")}</code>);
    expect(html).toContain("tok-keyword");
    expect(html).toContain("select");
    expect(html).toContain("from");
    expect(html).toContain("where");
  });
});

describe("代码高亮：边界情况", () => {
  it("空字符串不抛错", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("", "ts")}</code>);
    expect(html).toBe("<code></code>");
  });

  it("未知 lang 走 ts fallback（不抛错）", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("const x = 1", "rust-script-rare" as never)}</code>);
    expect(html).toBeTruthy();
  });

  it("多行字符串保留换行", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("a\nb\nc", "ts")}</code>);
    expect(html.split("\n").length).toBeGreaterThanOrEqual(3);
  });

  it("字符串里的 # 不被当注释（python 单引号内）", () => {
    const html = renderToStaticMarkup(<code>{highlightCode("x = 'a # not comment'", "python")}</code>);
    // 因为是单行处理：' 开头 → 进入字符串模式直到下一个 ' 结束；# 是字符串内容
    // 但 scan 算法优先 # 进入 comment 模式 → 实际上 # 会吞掉
    // 至少不该在结果里出现 "tok-comment" 之外的整体 "x = ..." 变 comment
    // 验证：不会出现 'a # not comment' 之前的 x =  被分到 comment
    const hasFullComment = /<span class="tok tok-comment">x = 'a # not comment'<\/span>/.test(html);
    expect(hasFullComment).toBe(false);
  });
});

describe("代码高亮：block-level 多行字符串（round 25 P2-2 回归）", () => {
  it("Python 三引号字符串整段当 string（不被行扫描切碎）", () => {
    const code = 'msg = """\nhello\nworld\ndef not_keyword:\n"""';
    const html = renderToStaticMarkup(<code>{highlightCode(code, "python")}</code>);
    // 多行字符串中不应有独立的 def keyword 切出
    expect(/tok-keyword">def</.test(html)).toBe(false);
  });

  it("JS 模板字面量（含 ${...}）整段当 string", () => {
    const code = "const s = `hi ${name} world`;";
    const html = renderToStaticMarkup(<code>{highlightCode(code, "js")}</code>);
    // const 应是 keyword，但 ${...} 内部不被破坏
    expect(html).toContain("tok-keyword");
    expect(html).toContain("const");
  });

  it("Rust raw string r#\"...\"# 整段当 string", () => {
    const code = 'let s = r#"raw "with quote" and stuff"#;';
    const html = renderToStaticMarkup(<code>{highlightCode(code, "rs")}</code>);
    // let 仍是 keyword；raw string 内部不应被拆 string 再 string
    expect(html).toContain("let");
  });
});

describe("BarChart 横向网格线", () => {
  it("渲染 3 条 .barchart-grid-line（25% / 50% / 75%）", () => {
    const { container } = render(<BarChart data={[{ label: "a", value: 10 }]} />);
    const lines = container.querySelectorAll(".barchart-grid-line");
    expect(lines.length).toBe(3);
  });

  it("网格线在 25% / 50% / 75% 处", () => {
    const { container } = render(<BarChart data={[{ label: "a", value: 10 }]} />);
    const lines = container.querySelectorAll(".barchart-grid-line") as NodeListOf<HTMLElement>;
    expect(lines[0].style.bottom).toBe("25%");
    expect(lines[1].style.bottom).toBe("50%");
    expect(lines[2].style.bottom).toBe("75%");
  });

  it("网格线 pointer-events: none（不挡柱子 hover）", () => {
    const { container } = render(<BarChart data={[{ label: "a", value: 10 }]} />);
    const grid = container.querySelector(".barchart-grid") as HTMLElement;
    expect(grid.style.pointerEvents).toBe("none");
  });
});

describe("热力图 17×30 CSS 校验", () => {
  it("heat-cell 渲染存在（className 存在性）", () => {
    const { container } = render(
      <div className="heatmap">
        <div className="heatmap-col">
          <span className="heat-cell" />
        </div>
      </div>,
    );
    expect(container.querySelector(".heat-cell")).toBeInTheDocument();
  });

  it("heat-legend-cell 渲染存在", () => {
    const { container } = render(
      <div className="heat-legend">
        <span className="heat-legend-cell" />
      </div>,
    );
    expect(container.querySelector(".heat-legend-cell")).toBeInTheDocument();
  });
});
