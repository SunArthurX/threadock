// 轻量 i18n：中文为 key，英文查词典、缺失回退中文（保证任何覆盖度下 UI 不破）。
// 切换语言：setLang 更新模块级 currentLang 并通知订阅者；App 根部 key={lang}
// 整树重挂载，使所有 t() 以新语言重新执行（memo 组件也随之重建）。
import { useEffect, useReducer } from "react";
import { EN } from "./i18n.en";

export type Lang = "zh" | "en";

const LANG_KEY = "ch-pref-lang";

function loadLang(): Lang {
  try {
    const v = localStorage.getItem(LANG_KEY);
    if (v === "zh" || v === "en") return v;
  } catch { /* 静默（隐私模式等） */ }
  return "zh";
}

let currentLang: Lang = loadLang();
const listeners = new Set<() => void>();

export function getLang(): Lang {
  return currentLang;
}

export function setLang(l: Lang) {
  if (l === currentLang) return;
  currentLang = l;
  try { localStorage.setItem(LANG_KEY, l); } catch { /* 静默 */ }
  document.documentElement.lang = l === "zh" ? "zh-CN" : "en";
  for (const fn of listeners) fn();
}

/** 订阅语言变化（App 根部用来触发整树重挂载）。 */
export function subscribeLang(fn: () => void): () => void {
  listeners.add(fn);
  return () => { listeners.delete(fn); };
}

/**
 * 翻译：中文原文为 key。en 模式下查词典，缺失回退原文。
 * params 支持 `{n}` 插值（词典值与原文均可含占位符）。
 */
export function t(key: string, params?: Record<string, string | number>): string {
  let s = currentLang === "en" ? (EN[key] ?? key) : key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      s = s.split(`{${k}}`).join(String(v));
    }
  }
  return s;
}

/** 组件内使用：订阅语言变化触发重渲染（根组件用；子组件经 key={lang} 重挂载覆盖）。 */
export function useLang(): { lang: Lang; setLang: (l: Lang) => void } {
  const [, force] = useReducer((x: number) => x + 1, 0);
  useEffect(() => subscribeLang(force), []);
  return { lang: currentLang, setLang };
}
