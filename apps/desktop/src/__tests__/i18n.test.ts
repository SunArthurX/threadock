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
    // 未收录时插值仍生效于回退原文
    expect(t("共 {n} 条", { n: 3 })).toBe("共 3 条");
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
