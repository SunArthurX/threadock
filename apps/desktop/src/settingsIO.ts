// 用户配置 / 本地状态导入导出（不含敏感数据：密码、原始对话内容）。
// 覆盖范围：
//   - 偏好：主题 / 数字格式 / 货币 / 日期格式 / 同步间隔 / 保留天数 / 预算通知
//   - 视图：侧边栏折叠 / 上次视图 / 时间范围
//   - 收藏：会话 Pin / 报告收藏 / 提示词收藏 / 自动化关注 / 隐藏卡片
//   - 搜索：搜索历史
//   - 上次：changelog 上次看到版本
// 不覆盖：会话内容、消息、标签、审计发现状态（属于数据库；用备份/恢复走）

const EXPORT_VERSION = 1;
const ALL_KEYS = [
  // 偏好
  "ch-theme", "ch-text-size", "ch-pref-number", "ch-pref-currency", "ch-pref-date",
  "ch-sync-interval", "ch-retention-days", "ch-budget-notify",
  // 视图
  "ch-view", "ch-sidebar", "ch-sort-by", "ch-ops-range",
  // 收藏 / 关注
  "ch-conv-pins", "ch-report-favs", "ch-prompt-favs", "ch-automation-watch", "ch-cards-hidden",
  // 搜索
  "ch-search-history",
  // 上次看到的版本
  "ch-last-seen-version",
];

/** 打包所有用户配置为 JSON 字符串。 */
export function exportAllSettings(): string {
  const prefs: Record<string, string | null> = {};
  for (const k of ALL_KEYS) {
    try { prefs[k] = localStorage.getItem(k); } catch { prefs[k] = null; }
  }
  return JSON.stringify({
    version: EXPORT_VERSION,
    exported_at: new Date().toISOString(),
    prefs,
  }, null, 2);
}

/** 解析并应用配置 JSON。
 *  - mode "merge"（默认）：仅覆盖 bundle 里存在的 key（不删未列出的）
 *  - mode "replace"：先清空所有 ALL_KEYS 后再应用
 *  - 失败抛 Error（含原因） */
export function importAllSettings(json: string, mode: "merge" | "replace" = "merge"): { applied: number; skipped: number } {
  let parsed: unknown;
  try { parsed = JSON.parse(json); } catch { throw new Error("JSON 解析失败"); }
  if (!parsed || typeof parsed !== "object") throw new Error("JSON 根节点不是对象");
  const obj = parsed as { version?: number; prefs?: Record<string, unknown> };
  if (typeof obj.version !== "number") throw new Error("缺少 version 字段");
  if (obj.version > EXPORT_VERSION) throw new Error(`配置版本 ${obj.version} 高于当前支持 ${EXPORT_VERSION}`);
  if (!obj.prefs || typeof obj.prefs !== "object") throw new Error("缺少 prefs 字段");
  const prefs = obj.prefs as Record<string, string | null | undefined>;
  // 防御：只接受白名单 key（防注入）
  const validKeys = new Set<string>(ALL_KEYS);
  let applied = 0; let skipped = 0;
  if (mode === "replace") {
    for (const k of ALL_KEYS) {
      try { localStorage.removeItem(k); } catch { /* 静默 */ }
    }
  }
  for (const [k, v] of Object.entries(prefs)) {
    if (!validKeys.has(k)) { skipped++; continue; }
    if (v === null || v === undefined) { skipped++; continue; }
    if (typeof v !== "string") { skipped++; continue; }
    try { localStorage.setItem(k, v); applied++; } catch { skipped++; }
  }
  return { applied, skipped };
}

/** 默认文件名（含日期）。 */
export function defaultSettingsFilename(now: Date = new Date()): string {
  return `threadock-settings-${now.toISOString().slice(0, 10)}.json`;
}
