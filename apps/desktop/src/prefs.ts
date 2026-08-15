// 用户偏好：数字格式 + 货币 + 日期格式（localStorage 持久化）。
// 这些偏好只影响展示，不影响后端存储。
export type NumberFormat = "raw" | "k" | "wan" | "yi";
export type Currency = "USD" | "CNY";
export type DateFormat = "relative" | "absolute" | "iso";

const NUM_KEY = "ch-pref-number";
const CUR_KEY = "ch-pref-currency";
const DATE_KEY = "ch-pref-date";

function load<T extends string>(key: string, fallback: T, valid: readonly T[]): T {
  try {
    const v = localStorage.getItem(key) as T | null;
    if (v && (valid as readonly string[]).includes(v)) return v;
  } catch { /* 静默 */ }
  return fallback;
}

export function loadNumberFormat(): NumberFormat {
  return load<NumberFormat>(NUM_KEY, "raw", ["raw", "k", "wan", "yi"]);
}
export function saveNumberFormat(v: NumberFormat) {
  try { localStorage.setItem(NUM_KEY, v); } catch { /* 静默 */ }
}

export function loadCurrency(): Currency {
  return load<Currency>(CUR_KEY, "USD", ["USD", "CNY"]);
}
export function saveCurrency(v: Currency) {
  try { localStorage.setItem(CUR_KEY, v); } catch { /* 静默 */ }
}

export function loadDateFormat(): DateFormat {
  return load<DateFormat>(DATE_KEY, "relative", ["relative", "absolute", "iso"]);
}
export function saveDateFormat(v: DateFormat) {
  try { localStorage.setItem(DATE_KEY, v); } catch { /* 静默 */ }
}

/** 货币符号 + 与 USD 的换算系数（粗略实时估值：1 USD ≈ 7.2 CNY，可后续接汇率 API 替换）。 */
const CURRENCY_META: Record<Currency, { symbol: string; perUsd: number; label: string }> = {
  USD: { symbol: "$", perUsd: 1, label: "美元 (USD)" },
  CNY: { symbol: "¥", perUsd: 7.2, label: "人民币 (CNY)" },
};

/** 把美元金额按当前货币偏好换算 + 格式化。 */
export function formatCostPref(usd: number, currency: Currency = loadCurrency()): string {
  const meta = CURRENCY_META[currency];
  const v = usd * meta.perUsd;
  return `${meta.symbol}${v.toFixed(v < 10 ? 3 : 2)}`;
}

/** Token 数量人性化（按数字格式偏好）。 */
export function formatTokensPref(n: number, fmt: NumberFormat = loadNumberFormat()): string {
  if (fmt === "raw") return n.toLocaleString();
  if (fmt === "k") {
    if (Math.abs(n) >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
    if (Math.abs(n) >= 1_000) return (n / 1_000).toFixed(1) + "K";
    return n.toString();
  }
  if (fmt === "wan") {
    if (Math.abs(n) >= 100_000_000) return (n / 100_000_000).toFixed(2) + "亿";
    if (Math.abs(n) >= 10_000) return (n / 10_000).toFixed(1) + "万";
    return n.toLocaleString();
  }
  // yi
  if (Math.abs(n) >= 1_000_000_000) return (n / 1_000_000_000).toFixed(2) + "B";
  if (Math.abs(n) >= 10_000) return (n / 10_000).toFixed(1) + "万";
  return n.toLocaleString();
}

/** 时间戳 → 用户偏好格式。
 *  - relative: "3 分钟前" / "2 小时前" / "3 天前"
 *  - absolute: "2026-08-12 14:23"
 *  - iso: "2026-08-12T14:23:45" */
export function formatTimePref(ms: number | null | undefined, fmt: DateFormat = loadDateFormat()): string {
  if (ms == null) return "—";
  if (fmt === "iso") return new Date(ms).toISOString().slice(0, 19);
  if (fmt === "absolute") {
    const d = new Date(ms);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }
  // relative
  const diff = Date.now() - ms;
  if (diff < 0) return "刚刚";
  const s = Math.floor(diff / 1000);
  if (s < 60) return `${s} 秒前`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分钟前`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前`;
  const d = Math.floor(h / 24);
  if (d < 30) return `${d} 天前`;
  const mo = Math.floor(d / 30);
  if (mo < 12) return `${mo} 个月前`;
  return `${Math.floor(mo / 12)} 年前`;
}
