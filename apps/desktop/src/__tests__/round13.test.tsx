// 第 13 轮：会话页 5 个微调
// 1) 搜索下拉 hover 改用 accent-bg
// 2) 删除"☆ 保存"按钮 + 孤立 saveCurrentSearch 函数
// 3) 导入按钮 → 同步（顶钮 + 下拉第一项 + 全局文案）
// 4) 设置链接改用 button + openUrl，不再用 <a target="_blank">
// 5) 清理设置关于的 emoji，改用 Icon
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const APP_TSX = resolve(here, "../App.tsx");
const SETTINGS = resolve(here, "../SettingsView.tsx");
const IMPORT_MENU = resolve(here, "../ImportMenu.tsx");
const ONBOARDING = resolve(here, "../OnboardingTour.tsx");
const CSS = resolve(here, "../styles.css");

async function src(p: string) { return readFile(p, "utf8"); }

describe("Round 13.1 搜索下拉样式（v1.3.0 升级为悬浮卡片）", () => {
  it("下拉用 backdrop 模糊 + elevated 背景 + 中性行 hover，取代 13 轮的 accent 蓝整行高亮", async () => {
    const css = await src(CSS);
    // 悬浮卡片：backdrop 模糊 + 高层背景，与下方会话列表拉开层次
    expect(/\.search-history-dropdown\s*\{[^}]*backdrop-filter/.test(css)).toBe(true);
    expect(/\.search-history-dropdown\s*\{[^}]*--bg-elevated/.test(css)).toBe(true);
    // 行 hover：中性底色 + 主文字色（不再 accent 蓝整行）
    expect(/\.search-history-item:hover\s*\{[^}]*--bg-hover/.test(css)).toBe(true);
    // 单条删除 × 的 hover 反馈
    expect(/\.search-history-del:hover/.test(css)).toBe(true);
  });
});

describe("Round 13.2 删除保存按钮", () => {
  it("App.tsx 不再渲染 ☆ 保存按钮", async () => {
    const s = await src(APP_TSX);
    expect(s.includes("☆ 保存")).toBe(false);
    expect(/saveCurrentSearch\s*=/.test(s)).toBe(false);
  });
});

describe("Round 13.3 导入按钮 → 同步", () => {
  it("ImportMenu 顶钮 text 改为「同步」+ Icon sync", async () => {
    const s = await src(IMPORT_MENU);
    expect(s).toMatch(/<Icon\s+name="sync"\s+size=\{12\}\s*\/>\s*同步/);
    expect(s).toMatch(/待同步/);
  });
  it("ImportMenu 下拉第一项改为「立即同步全部」", async () => {
    const s = await src(IMPORT_MENU);
    expect(s).toMatch(/立即同步全部/);
    // JSX 文案不再含"增量同步"（注释/类型注释里的不算）
    const jsxSection = s.split("// ")[0]; // 截掉前面注释
    expect(jsxSection.includes("增量同步")).toBe(false);
  });
  it("全局文案同步：onboarding + empty state", async () => {
    const ob = await src(ONBOARDING);
    expect(ob).toMatch(/「同步」按钮/);
    const ap = await src(APP_TSX);
    expect(ap).toMatch(/<Icon\s+name="sync"\s+size=\{12\}\s*\/>\s*立即同步/);
  });
});

describe("Round 13.4 设置链接改用 button + openUrl", () => {
  it("SettingsView 引入 @tauri-apps/plugin-opener 的 openUrl", async () => {
    const s = await src(SETTINGS);
    expect(s).toMatch(/import\s*\{[^}]*openUrl[^}]*\}\s*from\s*"@tauri-apps\/plugin-opener"/);
  });
  it("SettingsView 提供 openExternal helper", async () => {
    const s = await src(SETTINGS);
    expect(s).toMatch(/function\s+openExternal\s*\(\s*url:\s*string\s*\)/);
  });
  it("相关链接是 <button>，不是 <a target=\"_blank\">", async () => {
    const s = await src(SETTINGS);
    // 不再用 href={l.url}
    expect(/href=\{l\.url\}/.test(s)).toBe(false);
    // 不再用 target="_blank"
    expect(s.includes('target="_blank"')).toBe(false);
    // 用 button 调用 openExternal
    expect(/onClick=\{\(\)\s*=>\s*openExternal\(l\.url\)\}/.test(s)).toBe(true);
  });
});

describe("Round 13.5 设置关于 emoji 全清", () => {
  it("links 列表的 label 不再有 emoji 前缀", async () => {
    const s = await src(SETTINGS);
    // 不再含 📖 🐛 📝 💬
    expect(s).not.toMatch(/📖|🐛|📝|💬/);
  });
  it("links 列表使用 Icon 组件（globe / bug / history / chat）", async () => {
    const s = await src(SETTINGS);
    expect(/icon:\s*"globe"/.test(s)).toBe(true);
    expect(/icon:\s*"bug"/.test(s)).toBe(true);
    expect(/icon:\s*"history"/.test(s)).toBe(true);
    expect(/icon:\s*"chat"/.test(s)).toBe(true);
  });
});
