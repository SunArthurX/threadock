// 左栏会话列表横向滚动（v1.3.0）：
// 1) 行区容器 .list-virtual-container 开 overflow-x，标题在行区内不再省略号截断
// 2) 虚拟行 max-content 撑宽 + min-width 100% 兜底
// 3) 横滑导航 hook 不再挂到左栏（左栏回归原生横向滚动行为）
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const CSS = resolve(here, "../styles.css");
const LIST = resolve(here, "../ConversationList.tsx");
const APP = resolve(here, "../App.tsx");

async function src(p: string) { return readFile(p, "utf8"); }

describe("会话列表横向滚动", () => {
  it("行区容器开启 overflow-x:auto，标题在行区内不再省略号截断", async () => {
    const css = await src(CSS);
    expect(/\.list-virtual-container\s*\{\s*overflow-x:\s*auto/.test(css)).toBe(true);
    expect(/\.list-virtual-container \.list-item \.title\s*\{[^}]*text-overflow:\s*clip/.test(css)).toBe(true);
  });

  it("虚拟行用 max-content 撑宽（min-width 100% 兜底）", async () => {
    const s = await src(LIST);
    expect(s).toMatch(/width:\s*"max-content"/);
    expect(s).toMatch(/minWidth:\s*"100%"/);
  });

  it("左栏 ScrollArea 不再挂横滑导航（原生横向滚动接管）", async () => {
    const s = await src(APP);
    expect(s).not.toMatch(/listPaneSwipe/);
    expect(s).toMatch(/<ScrollArea style=\{\{ width: listWidth \}\}>/);
  });
});
