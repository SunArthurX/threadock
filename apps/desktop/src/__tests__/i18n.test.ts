// i18n 核心：中文回退 / 词典命中 / 插值 / 切换持久化与订阅。
import { describe, it, expect, beforeEach, vi } from "vitest";
import { t, setLang, getLang, subscribeLang, type Lang } from "../i18n";

describe("i18n", () => {
  beforeEach(() => {
    localStorage.clear();
    setLang("zh");
  });

  it("zh 模式原文直返", () => {
    expect(t("设置")).toBe("设置");
  });

  it("en 模式命中词典", () => {
    setLang("en");
    // 词典在 SettingsView 已有真实 key（语言设置行）
    expect(t("语言")).toBe("Language");
  });

  it("en 模式缺失回退中文（增量迁移期 UI 不破）", () => {
    setLang("en");
    expect(t("__不存在的键__")).toBe("__不存在的键__");
  });

  it("插值 {n} 支持原文与词典值", () => {
    expect(t("共 {n} 条", { n: 3 })).toBe("共 3 条");
    setLang("en");
    // 已收录：词典值插值
    expect(t("共 {n} 条", { n: 3 })).toBe("3 total");
    // 未收录：插值仍生效于回退原文
    expect(t("__回退{x}__", { x: 7 })).toBe("__回退7__");
  });

  it("setLang 持久化并通知订阅者", () => {
    const fn = vi.fn();
    const unsub = subscribeLang(fn);
    setLang("en");
    expect(getLang()).toBe<Lang>("en");
    expect(localStorage.getItem("ch-pref-lang")).toBe("en");
    expect(fn).toHaveBeenCalledTimes(1);
    // 幂等：同语言不重复通知
    setLang("en");
    expect(fn).toHaveBeenCalledTimes(1);
    unsub();
    setLang("zh");
    expect(fn).toHaveBeenCalledTimes(1);
  });
});

// ── 词典覆盖率守护：源码所有 t() key 必须有英文译文 ──────────────────
describe("i18n 词典覆盖率", () => {
  it("源码全部 t() key 均已收录（防新增文案漏翻译）", async () => {
    const { EN } = await import("../i18n.en");
    const { readdirSync, readFileSync } = await import("node:fs");
    const { join } = await import("node:path");
    // 递归扫描 src 下 t("...") 调用点（排除测试与词典自身）
    const collect = (dir: string): string[] =>
      readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
        const p = join(dir, e.name);
        if (e.isDirectory()) return e.name === "__tests__" ? [] : collect(p);
        if (!/\.(tsx|ts)$/.test(e.name) || e.name === "i18n.ts" || e.name === "i18n.en.ts") return [];
        return [...readFileSync(p, "utf-8").matchAll(/(?<![.\w])t\("([^"]+)"/g)].map((m) => m[1]);
      });
    const keys = [...new Set(collect(join(process.cwd(), "src")))];
    expect(keys.length).toBeGreaterThan(500); // 规模护栏：漏扫时测试报警
    const missing = keys.filter((k) => !(k in EN));
    expect(missing).toEqual([]);
  });
});
