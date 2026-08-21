// 第 12 轮：键盘快捷键契约测试（不直接挂全局 keydown，转测源码契约）
// 1) clipboard.copyToClipboard 模块存在
// 2) App.tsx 包含 ⌘G / ⌘⇧G / ⌘, / ⌘D 快捷键分支
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const APP_TSX = resolve(here, "../App.tsx");


async function loadSrc(p: string): Promise<string> {
  return readFile(p, "utf8");
}

describe("Round 12 快捷键契约", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => localStorage.clear());

  it("clipboard.copyToClipboard 是异步函数（⌘D 用它）", async () => {
    const mod = await import("../clipboard");
    expect(typeof mod.copyToClipboard).toBe("function");
    const r = await mod.copyToClipboard("test-id-123");
    expect(typeof r.ok).toBe("boolean");
  });

  it("App.tsx 引入 copyToClipboard（⌘D 走它，不走 navigator）", async () => {
    const src = await loadSrc(APP_TSX);
    expect(src).toMatch(/import\s*\{[^}]*copyToClipboard[^}]*\}\s*from\s*"\.\/clipboard"/);
    expect(src).toMatch(/copyToClipboard\(id\)/);
  });

  it("App.tsx 包含 ⌘G / ⌘⇧G 命中跳转 handler", async () => {
    const src = await loadSrc(APP_TSX);
    expect(src).toMatch(/e\.key\.toLowerCase\(\)\s*===\s*"g"/);
    expect(src).toMatch(/stepHits\(e\.shiftKey\s*\?\s*-1\s*:\s*1\)/);
  });

  it("App.tsx 包含 ⌘, 打开设置 handler", async () => {
    const src = await loadSrc(APP_TSX);
    expect(src).toMatch(/e\.key\s*===\s*","/);
    expect(src).toMatch(/setSettingsOpen/);
  });

  it("App.tsx 包含 ⌘D 复制会话 ID handler", async () => {
    const src = await loadSrc(APP_TSX);
    expect(src).toMatch(/e\.key\.toLowerCase\(\)\s*===\s*"d"/);
    expect(src).toMatch(/showToast.*已复制会话 ID/);
  });
});
