// 设置面板：汇总全项目设置项。
//
// 设置项清单（分析自代码库）：
// - 外观：主题（localStorage ch-theme）
// - 同步：自动同步间隔（localStorage + DB app_settings.sync_interval_min）；
//   上次会话/指标同步时间（DB last_conv_sync_ms / last_ops_sync_ms，只读展示）；
//   指标节流 30 分钟为内置行为（ops_sync force 可手动强刷）
// - 治理（在各治理页管理，此处导航）：预算 budget_settings（成本页）、
//   模型定价 pricing（成本页）、脱敏规则与命令黑名单 policy_rules（安全页）
// - 数据：重置所有数据（危险操作，输入「重置」确认，防误触）
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save, open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { showToast } from "./toast";
import { formatTime } from "./types";
import type { LlmConfigView } from "./types";
import { exportAllSettings, importAllSettings, defaultSettingsFilename } from "./settingsIO";
import ScrollArea from "./ScrollArea";
import WorkspaceSection from "./WorkspaceSection";
import { Icon } from "./Icon";
/** 重置确认词：输入完全一致才允许执行（防误触）。 */
export const RESET_CONFIRM_TEXT = "重置";

/** 桌面端版本（与 package.json 对齐）。 */
import pkg from "../package.json";
import cargoTomlRaw from "../../../Cargo.toml?raw";

/** 应用版本：构建期从 package.json 派生（此前硬编码 0.1.0 忘更新，GUI 真人测试发现）。 */
export const APP_VERSION: string = pkg.version;
/** 核心库版本：构建期从 workspace Cargo.toml 派生，与 Rust 侧永远一致。 */
export const CORE_VERSION: string =
  cargoTomlRaw.match(/^version\s*=\s*"([^"]+)"/m)?.[1] ?? "unknown";

/** UI 降级用的 earliest（库为空时 fallback 到今天）。 */
export function resetDateBoundsSync(): { earliest: string; today: string } {
  const today = new Date().toISOString().slice(0, 10);
  return { earliest: today, today };
}

/** 拉取库中最早数据时间戳（毫秒）→ 格式化成 YYYY-MM-DD。
 * 库为空时返回今天。失败时也降级到今天（让用户至少能操作）。 */
export async function fetchResetDateBounds(): Promise<{ earliest: string; today: string }> {
  try {
    const r = await invoke<{ earliest_ms: number; latest_ms: number }>("reset_range_bounds", {});
    const today = new Date(r.latest_ms).toISOString().slice(0, 10);
    // earliest_ms=0 表示空库（命令层 unwrap_or(0) 兜底），fallback 到 today
    const earliest = r.earliest_ms > 0
      ? new Date(r.earliest_ms).toISOString().slice(0, 10)
      : today;
    return { earliest, today };
  } catch {
    return resetDateBoundsSync();
  }
}

const INTERVAL_OPTIONS: [number, string][] = [
  [0, "关闭"],
  [5, "每 5 分钟"],
  [10, "每 10 分钟"],
  [30, "每 30 分钟"],
];

type GovernanceView = "overview" | "cost" | "security";

interface Props {
  theme: "dark" | "light";
  onThemeChange: (t: "dark" | "light") => void;
  textSize: "sm" | "md" | "lg" | "xl";
  onTextSizeChange: (s: "sm" | "md" | "lg" | "xl") => void;
  syncIntervalMin: number;
  onSyncIntervalChange: (min: number) => void;
  retentionDays: number;
  onRetentionDaysChange: (days: number) => void;
  notifyOnExceed: boolean;
  onNotifyOnExceedChange: (v: boolean) => void;
  numberFormat: "raw" | "k" | "wan" | "yi";
  onNumberFormatChange: (f: "raw" | "k" | "wan" | "yi") => void;
  currency: "USD" | "CNY";
  onCurrencyChange: (c: "USD" | "CNY") => void;
  dateFormat: "relative" | "absolute" | "iso";
  onDateFormatChange: (f: "relative" | "absolute" | "iso") => void;
  onNavigate: (view: GovernanceView) => void;
  onReset: () => Promise<void>;
  resetting: boolean;
  onClose: () => void;
  onShowChangelog: () => void;
  /** 重新查看新手引导（round 25：从右下角 fab 移入设置）。 */
  onShowOnboarding?: () => void;
  /** 通知 App 层从 localStorage 重新读偏好并应用（替代刷新页面）。 */
  onReapplyImportedPrefs?: () => void;
}

const RETENTION_OPTIONS: [number, string][] = [
  [0, "关闭"],
  [30, "30 天"],
  [90, "90 天"],
  [180, "180 天"],
];

/** 弹窗内迷你进度条（与后端 sync_progress 事件联动）。 */
function MiniProgress({ p }: { p: { current: number; total: number; detail: string } | null }) {
  if (!p || p.total === 0) return null;
  return (
    <span className="mini-progress" title={`${p.detail} ${p.current}/${p.total}`}>
      <span className="mini-progress-fill" style={{ width: `${Math.min(100, (p.current / p.total) * 100)}%` }} />
      <span className="mini-progress-label">{p.detail === "done" ? "完成" : `${p.detail} ${p.current}/${p.total}`}</span>
    </span>
  );
}

/** 在系统默认浏览器里打开 URL（Tauri WebView 的 target=_blank 不工作，必须走 plugin-opener）。 */
function openExternal(url: string) {
  openUrl(url).catch((e) => {
    // 非 Tauri 环境（纯 web dev server）走兜底
    try { window.open(url, "_blank", "noopener,noreferrer"); }
    catch { showToast(`✗ 无法打开链接：${String(e)}`, "error", 3000); }
  });
}

/** 字节数人性化。 */
export function formatBytes(n: number): string {
  if (n >= 1e9) return (n / 1e9).toFixed(2) + " GB";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + " MB";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + " KB";
  return `${n} B`;
}

/** 治理动作名 → 中文。 */
export const GOVERNANCE_LABELS: Record<string, string> = {
  reset_all_data: "重置全部数据",
  gc_raw_store: "清理孤儿数据",
  rebuild_search_index: "重建搜索索引",
  retention_archive: "保留策略归档",
  hard_delete_conversation: "彻底删除会话",
  soft_delete_conversation: "删除会话（回收站）",
  archive_conversation: "归档会话",
  unarchive_conversation: "取消归档",
  audit_finding_disposition: "审计发现处置",
};

export default function SettingsView({
  theme, onThemeChange, textSize, onTextSizeChange, syncIntervalMin, onSyncIntervalChange,
  retentionDays, onRetentionDaysChange, notifyOnExceed, onNotifyOnExceedChange,
  numberFormat, onNumberFormatChange, currency, onCurrencyChange, dateFormat, onDateFormatChange,
  onNavigate, onReset, resetting, onClose, onShowChangelog, onShowOnboarding, onReapplyImportedPrefs,
}: Props) {
  const [storage, setStorage] = useState<{ db_bytes: number; raw_count: number; raw_bytes: number; index_bytes: number } | null>(null);
  const [gcResult, setGcResult] = useState<string | null>(null);
  const [gcRunning, setGcRunning] = useState(false);
  const [rebuildMsg, setRebuildMsg] = useState<string | null>(null);
  const [govLog, setGovLog] = useState<{ id: string; action: string; created_at: number }[]>([]);
  const [lastWeekly, setLastWeekly] = useState<number | null>(null);
  // 弹窗内迷你进度（sync_progress 事件：重置后重导 / 指标同步 / 重建索引共用）
  const [mini, setMini] = useState<{ current: number; total: number; detail: string; finished: boolean } | null>(null);
  useEffect(() => {
    const un = listen<{ current: number; total: number; detail: string; finished: boolean }>("sync_progress", (e) => {
      setMini(e.payload);
      if (e.payload.finished) window.setTimeout(() => setMini(null), 2000);
    });
    return () => { un.then((f) => f()); };
  }, []);
  const [confirmText, setConfirmText] = useState("");
  // 时间范围重置：bounds 来自后端（库中最早数据时间戳），无 31 天硬限制
  const [resetDate, setResetDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [bounds, setBounds] = useState<{ earliest: string; today: string }>(() => resetDateBoundsSync());
  const [rangePreview, setRangePreview] = useState<{ conversations: number; messages: number; usage_records: number } | null>(null);
  const loadRangePreview = async () => {
    if (!resetDate) return;
    try {
      setRangePreview(await invoke("reset_range_preview", { startMs: new Date(resetDate + "T00:00:00").getTime() }));
    } catch { /* 静默 */ }
  };
  const [lastConvSync, setLastConvSync] = useState<number | null>(null);
  const [lastOpsSync, setLastOpsSync] = useState<number | null>(null);
  const [opsSyncing, setOpsSyncing] = useState(false);
  const [opsMsg, setOpsMsg] = useState<string | null>(null);

  // 打开时读取只读的同步时间戳 + 存储看板 + 治理流水 + 周报时间 + 重置 bounds
  useEffect(() => {
    (async () => {
      try {
        const conv = await invoke<string | null>("app_setting_get", { key: "last_conv_sync_ms" });
        const ops = await invoke<string | null>("app_setting_get", { key: "last_ops_sync_ms" });
        const weekly = await invoke<string | null>("app_setting_get", { key: "last_weekly_ms" });
        setLastConvSync(conv ? Number(conv) : null);
        setLastOpsSync(ops ? Number(ops) : null);
        setLastWeekly(weekly ? Number(weekly) : null);
      } catch { /* 只读展示，失败忽略 */ }
      try { setStorage(await invoke("storage_stats", {})); } catch { /* 空库忽略 */ }
      try {
        setGovLog(await invoke<{ id: string; action: string; created_at: number }[]>("governance_log_list", { limit: 8 }));
      } catch { /* 空表忽略 */ }
      try {
        setBounds(await fetchResetDateBounds());
      } catch { /* fallback 用 sync 兜底（已是今天） */ }
    })();
  }, []);

  const forceOpsSync = async () => {
    setOpsSyncing(true); setOpsMsg(null);
    try {
      const r = await invoke<{ usage_written: number }>("ops_sync", { force: true });
      setOpsMsg(`已写入 ${r.usage_written} 条用量记录`);
    } catch (e) { setOpsMsg(typeof e === "string" ? e : String(e)); }
    setOpsSyncing(false);
  };

  const canReset = confirmText === RESET_CONFIRM_TEXT && !resetting && !!resetDate;

  const doReset = async () => {
    if (!canReset || !resetDate) return;
    try {
      const r = await invoke<{ conversations: number; messages: number }>("reset_range", {
        startMs: new Date(resetDate + "T00:00:00").getTime(),
      });
      setConfirmText("");
      setRangePreview(null);
      showToast(`✓ 已重置 ${resetDate} 之后的数据（${r.conversations} 会话 / ${r.messages} 消息），正在从源重新刷入…`, "info", 8000);
    } catch (e) {
      showToast(`重置失败：${typeof e === "string" ? e : String(e)}`, "error");
    }
    await onReset();
  };

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>⚙ 设置</h2>
          <button className="settings-close" onClick={onClose}>✕</button>
        </div>

        <ScrollArea className="settings-body">
          <section className="settings-section">
            <h3>外观</h3>
            <div className="settings-row">
              <span>外观</span>
              <div className="settings-segment">
                <button className={theme === "light" ? "active" : ""} onClick={() => onThemeChange("light")}>浅色</button>
                <button className={theme === "dark" ? "active" : ""} onClick={() => onThemeChange("dark")}>深色</button>
              </div>
            </div>
            <div className="settings-row">
              <span>
                字号
                <small style={{ display: "block", fontSize: 11, color: "var(--text-faint)", fontWeight: 400, marginTop: 2 }}>
                  macOS "Larger Text" 体验
                </small>
              </span>
              <div className="text-size-control" role="radiogroup" aria-label="字号">
                {(["sm", "md", "lg", "xl"] as const).map((s, i) => (
                  <button
                    key={s}
                    type="button"
                    role="radio"
                    aria-checked={textSize === s}
                    className={`text-size-btn ${textSize === s ? "active" : ""}`}
                    onClick={() => onTextSizeChange(s)}
                    title={["默认 (13.5px)", "稍大 (14.5px)", "大 (15.5px)", "特大 (16.5px)"][i]}
                  >
                    <span className="text-size-letter">A</span>
                    <span className="text-size-letter-size" style={{ fontSize: 8 + i * 1.5 }}>A</span>
                  </button>
                ))}
              </div>
            </div>
          </section>

          <section className="settings-section">
            <h3>显示偏好</h3>
            <div className="settings-row">
              <span>数字格式</span>
              <div className="settings-segment">
                <button className={numberFormat === "raw" ? "active" : ""} onClick={() => onNumberFormatChange("raw")}>1,234,567</button>
                <button className={numberFormat === "k" ? "active" : ""} onClick={() => onNumberFormatChange("k")}>1.2M</button>
                <button className={numberFormat === "wan" ? "active" : ""} onClick={() => onNumberFormatChange("wan")}>123.4万</button>
                <button className={numberFormat === "yi" ? "active" : ""} onClick={() => onNumberFormatChange("yi")}>1.2B / 万</button>
              </div>
            </div>
            <div className="settings-row">
              <span>货币</span>
              <div className="settings-segment">
                <button className={currency === "USD" ? "active" : ""} onClick={() => onCurrencyChange("USD")}>$ USD</button>
                <button className={currency === "CNY" ? "active" : ""} onClick={() => onCurrencyChange("CNY")}>¥ CNY (1 USD ≈ 7.2)</button>
              </div>
            </div>
            <div className="settings-row">
              <span>时间显示</span>
              <div className="settings-segment">
                <button className={dateFormat === "relative" ? "active" : ""} onClick={() => onDateFormatChange("relative")}>3 分钟前</button>
                <button className={dateFormat === "absolute" ? "active" : ""} onClick={() => onDateFormatChange("absolute")}>2026-08-12 14:23</button>
                <button className={dateFormat === "iso" ? "active" : ""} onClick={() => onDateFormatChange("iso")}>ISO</button>
              </div>
            </div>
            <div className="settings-hint">
              偏好仅影响展示与导出列宽，不影响后端存储；换算 1 USD ≈ 7.2 CNY 后续可接实时汇率 API 替换。
            </div>
          </section>

          <section className="settings-section">
            <h3>同步</h3>
            <div className="settings-row">
              <span>自动增量同步</span>
              <select
                value={syncIntervalMin}
                onChange={(e) => onSyncIntervalChange(Number(e.target.value))}
              >
                {INTERVAL_OPTIONS.map(([v, label]) => (
                  <option key={v} value={v}>{label}</option>
                ))}
              </select>
            </div>
            <div className="settings-hint">
              手动入口在「同步 → 立即同步全部」；指标采集另有 30 分钟节流（防止重复全量扫描）。
            </div>
            <div className="settings-row">
              <span>上次会话同步</span>
              <span className="settings-value">{formatTime(lastConvSync) || "从未"}</span>
            </div>
            <div className="settings-row">
              <span>上次指标同步</span>
              <span className="settings-value">{formatTime(lastOpsSync) || "从未"}</span>
            </div>
            <div className="settings-row">
              <span>指标数据</span>
              <button className="action-btn" disabled={opsSyncing} onClick={forceOpsSync}>
                {opsSyncing ? "同步中…" : "立即全量同步指标"}
              </button>
              <MiniProgress p={mini} />
              {opsMsg && <span className="settings-value">{opsMsg}</span>}
            </div>
          </section>

          <section className="settings-section">
            <h3>治理</h3>
            <div className="settings-row">
              <span>预算超限通知</span>
              <label className="settings-segment">
                <input type="checkbox" checked={notifyOnExceed} onChange={(e) => onNotifyOnExceedChange(e.target.checked)} />
                超预算时弹窗提醒（顶部预算条常驻显示）
              </label>
            </div>
            <div className="settings-hint">预算、定价与安全策略在对应治理页管理：</div>
            <div className="settings-row">
              <span>预算 / 定价</span>
              <button className="action-btn" onClick={() => { onClose(); onNavigate("cost"); }}>前往 成本 页 →</button>
            </div>
            <div className="settings-row">
              <span>脱敏规则 / 命令黑名单</span>
              <button className="action-btn" onClick={() => { onClose(); onNavigate("security"); }}>前往 安全 页 →</button>
            </div>
          </section>

          <WorkspaceSection />

          <section className="settings-section">
            <h3>AI 提取（大模型）</h3>
            <LlmSection />
          </section>

          <section className="settings-section">
            <h3>存储与维护</h3>
            {storage && (
              <div className="storage-rows">
                <div className="settings-row">
                  <span>数据库</span>
                  <span className="settings-value">{formatBytes(storage.db_bytes)}</span>
                </div>
                <div className="settings-row">
                  <span>原始归档（{storage.raw_count} 个）</span>
                  <span className="settings-value">{formatBytes(storage.raw_bytes)}</span>
                </div>
                <div className="settings-row">
                  <span>搜索索引</span>
                  <span className="settings-value">{formatBytes(storage.index_bytes)}</span>
                </div>
              </div>
            )}
            <div className="settings-row">
              <span>孤儿数据清理</span>
              <button className="action-btn" disabled={gcRunning} onClick={async () => {
                setGcRunning(true); setGcResult(null);
                try {
                  const r = await invoke<{ scanned: number; deleted: number; freed_bytes: number }>("gc_raw_store", {});
                  setGcResult(`扫描 ${r.scanned} · 删除 ${r.deleted} · 释放 ${formatBytes(r.freed_bytes)}`);
                  setStorage(await invoke("storage_stats", {}));
                } catch (e) { setGcResult(String(e)); }
                setGcRunning(false);
              }}>{gcRunning ? "⟳ 清理中…" : "🧹 清理未引用归档"}</button>
              {gcResult && <span className="settings-value">{gcResult}</span>}
            </div>
            <div className="settings-row">
              <span>重建搜索索引</span>
              <button className="action-btn" onClick={async () => {
                setRebuildMsg("重建中…");
                try {
                  const r = await invoke<{ messages: number }>("rebuild_search_index", {});
                  setRebuildMsg(`已重建 ${r.messages} 条消息的索引`);
                } catch (e) { setRebuildMsg(String(e)); }
              }}>♻ 重建</button>
              <MiniProgress p={mini} />
              {rebuildMsg && <span className="settings-value">{rebuildMsg}</span>}
            </div>
            <div className="settings-row">
              <span>保留策略（自动归档）</span>
              <select value={retentionDays} onChange={(e) => onRetentionDaysChange(Number(e.target.value))}>
                {RETENTION_OPTIONS.map(([v, label]) => (
                  <option key={v} value={v}>{label}</option>
                ))}
              </select>
            </div>
            <div className="settings-hint">
              开启后每次启动自动归档超过 N 天未更新的会话（可在会话列表「已归档」视图查看）。
            </div>
            <div className="settings-row">
              <span>周报</span>
              <span className="settings-value">{formatTime(lastWeekly) || "从未生成"}</span>
              <button className="action-btn" onClick={async () => {
                try {
                  const r = await invoke<{ generated: boolean; path: string | null }>("weekly_report_auto", {});
                  setLastWeekly(Date.now());
                  setRebuildMsg(r.generated && r.path ? `已生成：${r.path}` : "未到 7 天间隔，未生成");
                } catch (e) { setRebuildMsg(String(e)); }
              }}>立即生成</button>
            </div>
          </section>

          <section className="settings-section">
            <h3>治理操作流水（最近 8 条）</h3>
            {govLog.length === 0
              ? <div className="settings-hint">暂无记录</div>
              : govLog.map((l) => (
                <div key={l.id} className="settings-row">
                  <span>{GOVERNANCE_LABELS[l.action] ?? l.action}</span>
                  <span className="settings-value">{formatTime(l.created_at)}</span>
                </div>
              ))}
          </section>

          <section className="settings-section">
            <h3>加密备份</h3>
            <BackupSection />
          </section>

          <AboutSection
            onShowChangelog={onShowChangelog}
            onShowOnboarding={onShowOnboarding}
            onReapplyImportedPrefs={onReapplyImportedPrefs}
          />

          <section className="settings-section danger">
            <h3>按时间重置（并重新刷入）</h3>
            <div className="settings-hint">
              删除所选日期之后的所有会话、消息与指标，并<strong>自动从源（Claude Code / Codex / ZCode / MiniMax / Cursor）重新刷入</strong>该时间范围之后的数据。
              最早可选日期为库中现存最早数据（{bounds.earliest}），不再硬限一个月。
              范围删除走时间索引 + 单事务，秒级完成；随后触发一轮全量同步。
            </div>
            <div className="settings-row">
              <span>开始日期（最早 {bounds.earliest}）</span>
              <input
                type="date"
                value={resetDate}
                min={bounds.earliest}
                max={bounds.today}
                onChange={(e) => { setResetDate(e.target.value); setRangePreview(null); }}
              />
            </div>
            {resetDate && (
              <div className="settings-row">
                <span>将删除</span>
                {rangePreview ? (
                  <span className="settings-value">
                    {rangePreview.conversations} 会话 · {rangePreview.messages} 消息 · {rangePreview.usage_records} 指标记录
                  </span>
                ) : (
                  <button className="action-btn" onClick={loadRangePreview}>预览影响范围</button>
                )}
              </div>
            )}
            <div className="settings-row">
              <span>输入「{RESET_CONFIRM_TEXT}」以确认</span>
              <input
                className="settings-confirm-input"
                type="text"
                value={confirmText}
                placeholder={`请输入 ${RESET_CONFIRM_TEXT}`}
                onChange={(e) => setConfirmText(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doReset()}
              />
              <button
                className="reset-confirm-btn"
                disabled={!canReset || resetting}
                onClick={doReset}
                style={resetting ? { cursor: "not-allowed" } : undefined}
              >
                {resetting ? "重置并重新刷入中…" : `重置并重新刷入 ${resetDate || ""} 之后的数据`}
              </button>
              <MiniProgress p={mini} />
            </div>
          </section>
        </ScrollArea>
      </div>
    </div>
  );
}

/** 关于：版本号 + 关键依赖 + 文档链接。 */
function AboutSection({
  onShowChangelog,
  onShowOnboarding,
  onReapplyImportedPrefs,
}: {
  onShowChangelog: () => void;
  onShowOnboarding?: () => void;
  onReapplyImportedPrefs?: () => void;
}) {
  // 依赖版本（与 package.json / Cargo.toml 对齐；本面板帮助用户/客服快速核对环境）
  const deps: { name: string; version: string; role: string }[] = [
    { name: "Tauri", version: "2.x", role: "桌面壳（Rust + WebView）" },
    { name: "React", version: "18.3.x", role: "UI 框架" },
    { name: "Vite", version: "5.4.x", role: "前端构建" },
    { name: "TypeScript", version: "5.5.x", role: "类型系统" },
    { name: "vitest", version: "3.2.7", role: "前端测试" },
    { name: "Rust 工具链", version: "stable", role: "后端运行时" },
  ];
  const links: { label: string; url: string; hint: string; icon: "globe" | "bug" | "history" | "chat" }[] = [
    { label: "项目主页", url: "https://github.com/sunqingguang/threadock", hint: "README / 路线图", icon: "globe" },
    { label: "报告问题", url: "https://github.com/sunqingguang/threadock/issues", hint: "Bug 反馈与功能建议", icon: "bug" },
    { label: "更新日志", url: "https://github.com/sunqingguang/threadock/releases", hint: "各版本变更说明", icon: "history" },
    { label: "讨论", url: "https://github.com/sunqingguang/threadock/discussions", hint: "使用交流与最佳实践", icon: "chat" },
  ];
  return (
    <section className="settings-section">
      <h3>关于</h3>
      <div className="settings-row">
        <span>应用名</span>
        <span className="settings-value">Threadock Desktop</span>
      </div>
      <div className="settings-row">
        <span>桌面版版本</span>
        <span className="settings-value">v{APP_VERSION}</span>
      </div>
      <div className="settings-row">
        <span>核心库版本</span>
        <span className="settings-value">v{CORE_VERSION}</span>
      </div>
      <div className="settings-hint">关键依赖：</div>
      <div className="about-deps">
        {deps.map((d) => (
          <div key={d.name} className="about-dep-row">
            <span className="about-dep-name">{d.name}</span>
            <span className="about-dep-ver mono">{d.version}</span>
            <span className="about-dep-role">{d.role}</span>
          </div>
        ))}
      </div>
      <div className="settings-hint">相关链接：</div>
      <div className="about-links">
        {links.map((l) => (
          <button
            key={l.url}
            type="button"
            className="about-link"
            onClick={() => openExternal(l.url)}
            title={l.hint}
          >
            <Icon name={l.icon} size={12} />
            <span>{l.label}</span>
            <Icon name="external" size={10} className="about-link-ext" />
          </button>
        ))}
      </div>
      <div className="settings-row" style={{ marginTop: 8 }}>
        <span>查看本版本更新日志</span>
        <button className="action-btn" onClick={onShowChangelog}>📋 查看更新日志</button>
      </div>
      {onShowOnboarding && (
        <div className="settings-row" style={{ marginTop: 8 }}>
          <span>新手引导</span>
          <button className="action-btn" onClick={onShowOnboarding} data-testid="settings-show-onboarding">❓ 重新查看新手引导</button>
        </div>
      )}
      <div className="settings-row" style={{ marginTop: 8 }}>
        <span>配置导入/导出</span>
        <button className="action-btn" onClick={async () => {
          try {
            const path = await save({ defaultPath: defaultSettingsFilename(), filters: [{ name: "JSON", extensions: ["json"] }] });
            if (typeof path !== "string") return;
            const json = exportAllSettings();
            await invoke("save_text_file", { path, content: json });
            showToast(`✓ 已导出配置（${(json.length / 1024).toFixed(1)} KB）`, "info", 4000);
          } catch (e) {
            showToast(`导出失败：${typeof e === "string" ? e : String(e)}`, "error");
          }
        }} title="导出主题/偏好/Pin/收藏/搜索历史 等本地配置（不含会话内容与密码）">⤓ 导出配置</button>
        <button className="action-btn" onClick={async () => {
          try {
            const path = await open({ multiple: false, filters: [{ name: "JSON", extensions: ["json"] }] });
            if (typeof path !== "string") return;
            const content = await invoke<string>("read_text_file", { path });
            const mode = window.confirm("选择导入模式：\n确定 = 合并（仅覆盖文件中的 key，保留其他）\n取消 = 完全替换（清空所有现有偏好）") ? "merge" : "replace";
            const { applied, skipped } = importAllSettings(content, mode);
            onReapplyImportedPrefs?.(); // 立即热应用（不需刷新）
            showToast(`✓ 已导入 ${applied} 项配置${skipped > 0 ? `（跳过 ${skipped} 项无效 key）` : ""}，已立即生效`, "info", 5000);
          } catch (e) {
            showToast(`导入失败：${typeof e === "string" ? e : String(e)}`, "error");
          }
        }} title="从 JSON 文件导入偏好（合并 / 替换 两种模式）">⤒ 导入配置</button>
      </div>
    </section>
  );
}

/** 加密备份/恢复（本地，密码仅进程内使用）。 */
function BackupSection() {  const [pw, setPw] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <>
      <div className="settings-row">
        <span>备份密码（≥8 位）</span>
        <input
          className="settings-confirm-input"
          type="password"
          value={pw}
          placeholder="备份加密密码"
          onChange={(e) => setPw(e.target.value)}
        />
      </div>
      <div className="settings-row">
        <span>创建备份</span>
        <button
          className="action-btn"
          disabled={busy || pw.length < 8}
          onClick={async () => {
            const { save } = await import("@tauri-apps/plugin-dialog");
            const path = await save({
              defaultPath: `threadock-backup-${new Date().toISOString().slice(0, 10)}.chbak`,
              filters: [{ name: "Threadock 备份", extensions: ["chbak"] }],
            });
            if (typeof path !== "string") return;
            setBusy(true); setMsg("备份中…");
            try {
              const r = await invoke<{ db_size: number; raw_count: number }>("backup_create", { path, password: pw });
              setMsg(`✓ 已备份（库 ${(r.db_size / 1048576).toFixed(1)}MB · ${r.raw_count} 个归档）`);
            } catch (e) { setMsg(String(e)); }
            setBusy(false);
          }}
        >⤓ 备份全部数据</button>
        {msg && <span className="settings-value">{msg}</span>}
      </div>
      <div className="settings-hint">
        备份含数据库与原始归档（Argon2id 加密）。恢复在 CLI：ch restore &lt;file&gt; &lt;dir&gt;（恢复为副本，不影响当前数据）。
      </div>
    </>
  );
}

/** LLM 预设：一键填入常见云端/本地端点。 */
const LLM_PRESETS: { label: string; baseUrl: string; model: string; local?: boolean }[] = [
  { label: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { label: "DeepSeek", baseUrl: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { label: "GLM", baseUrl: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash" },
  { label: "MiniMax", baseUrl: "https://api.minimax.io/v1", model: "MiniMax-M3" },
  { label: "Ollama 本地", baseUrl: "http://127.0.0.1:11434/v1", model: "qwen2.5:7b", local: true },
];

/** AI 提取（大模型）：端点配置 + API Key 加密存储 + 连接测试。
 * 前端永远拿不到密钥明文/密文（后端只回 masked 提示）。 */
function LlmSection() {
  const [form, setForm] = useState({ enabled: false, base_url: "", model: "" });
  const [meta, setMeta] = useState<Pick<LlmConfigView, "has_api_key" | "api_key_masked" | "is_local" | "api_key_broken"> | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState<"save" | "test" | null>(null);

  const applyView = (v: LlmConfigView) => {
    setForm({ enabled: v.enabled, base_url: v.base_url, model: v.model });
    setMeta({ has_api_key: v.has_api_key, api_key_masked: v.api_key_masked, is_local: v.is_local, api_key_broken: v.api_key_broken });
  };

  useEffect(() => {
    invoke<LlmConfigView>("llm_config_get", {}).then(applyView).catch(() => setMsg("配置读取失败"));
  }, []);

  const save = async (clearKey = false) => {
    setBusy("save"); setMsg(null);
    try {
      const v = await invoke<LlmConfigView>("llm_config_set", {
        input: {
          enabled: form.enabled,
          base_url: form.base_url.trim(),
          model: form.model.trim(),
          api_key: clearKey ? null : (apiKey.trim() ? apiKey.trim() : null),
          clear_api_key: clearKey,
        },
      });
      applyView(v);
      setApiKey("");
      setMsg("✓ 已保存（API Key 已本地加密存储）");
    } catch (e) { setMsg(`✗ ${typeof e === "string" ? e : String(e)}`); }
    setBusy(null);
  };

  /** 勾选「启用」即时落库生效（无需点保存）；校验失败（如端点未填）回滚并提示。 */
  const toggleEnabled = async (next: boolean) => {
    const prev = form.enabled;
    setForm({ ...form, enabled: next }); // 乐观更新，失败回滚
    setBusy("save"); setMsg(null);
    try {
      const v = await invoke<LlmConfigView>("llm_config_set", {
        input: {
          enabled: next,
          base_url: form.base_url.trim(),
          model: form.model.trim(),
          api_key: null, // 仅切开关：不覆盖已存密钥（空值=保持）
          clear_api_key: false,
        },
      });
      applyView(v);
      setMsg(next ? "✓ 已启用，立即生效" : "✓ 已停用，立即生效");
    } catch (e) {
      setForm((f) => ({ ...f, enabled: prev }));
      setMsg(`✗ ${typeof e === "string" ? e : String(e)}`);
    }
    setBusy(null);
  };

  const test = async () => {
    setBusy("test"); setMsg(null);
    try {
      const r = await invoke<{ ok: boolean; latency_ms: number; model: string }>("llm_test_connection", {});
      setMsg(`✓ 连接成功（${r.model} · ${r.latency_ms}ms）`);
    } catch (e) { setMsg(`✗ ${typeof e === "string" ? e : String(e)}`); }
    setBusy(null);
  };

  const isLocal = (() => {
    const rest = form.base_url.split("://")[1] ?? "";
    const host = rest.startsWith("[") ? rest.split("]")[0].slice(1) : rest.split(/[:/?#]/)[0] ?? "";
    const h = host.toLowerCase();
    return h === "localhost" || h === "::1" || h === "0.0.0.0" || h.startsWith("127.");
  })();

  return (
    <>
      <div className="settings-row">
        <span>启用大模型提取</span>
        <label className="settings-segment">
          <input
            type="checkbox"
            checked={form.enabled}
            disabled={busy !== null}
            onChange={(e) => toggleEnabled(e.target.checked)}
          />
          显式开启（默认关闭，勾选立即生效，规则引擎不受影响）
        </label>
      </div>
      {form.enabled && (
        <div className="settings-hint">
          开启后，在会话的「知识提取」弹窗可切换 ✨AI 引擎：会把<strong>当前会话的对话文本</strong>发送到所配端点做提取。
          {isLocal ? " 当前端点为本机地址，数据不出本机。" : " 云端端点请注意会话内容的保密性。"}
        </div>
      )}
      <div className="settings-row">
        <span>
          端点预设
          {isLocal && <span className="badge" style={{ marginLeft: 6 }} title="本地推理端点，数据不出本机">本地</span>}
        </span>
        <div className="settings-segment">
          {LLM_PRESETS.map((p) => (
            <button
              key={p.label}
              className="action-btn"
              title={`${p.baseUrl} · ${p.model}`}
              onClick={() => setForm({ ...form, base_url: p.baseUrl, model: p.model })}
            >{p.label}</button>
          ))}
        </div>
      </div>
      <div className="settings-row">
        <span>Base URL（OpenAI 兼容）</span>
        <input
          className="settings-confirm-input"
          style={{ flex: 1 }}
          type="text"
          value={form.base_url}
          placeholder="GLM: https://open.bigmodel.cn/api/paas/v4 · OpenAI: https://api.openai.com/v1 · 本地: http://127.0.0.1:11434/v1"
          onChange={(e) => setForm({ ...form, base_url: e.target.value })}
        />
      </div>
      <div className="settings-row">
        <span>模型名</span>
        <input
          className="settings-confirm-input"
          style={{ flex: 1 }}
          type="text"
          value={form.model}
          placeholder="gpt-4o-mini / deepseek-chat / qwen2.5:7b"
          onChange={(e) => setForm({ ...form, model: e.target.value })}
        />
      </div>
      <div className="settings-row">
        <span>API Key</span>
        <input
          className="settings-confirm-input"
          style={{ flex: 1 }}
          type="password"
          value={apiKey}
          placeholder={meta?.has_api_key ? `已存储（${meta.api_key_masked ?? "无法解密"}）——输入新值覆盖` : "本地推理可留空"}
          onChange={(e) => setApiKey(e.target.value)}
          autoComplete="off"
        />
      </div>
      {meta?.api_key_broken && (
        <div className="settings-hint">
          ⚠ 已存储密钥无法解密（可能更换过设备），请重新录入后保存。
        </div>
      )}
      <div className="settings-row">
        <span>操作</span>
        <button className="action-btn" disabled={busy !== null} onClick={() => save(false)}>
          {busy === "save" ? "保存中…" : "💾 保存配置"}
        </button>
        {meta?.has_api_key && (
          <button className="action-btn" disabled={busy !== null} onClick={() => save(true)} title="清除已存储的加密密钥">
            🗑 清除密钥
          </button>
        )}
        <button className="action-btn" disabled={busy !== null} onClick={test} title="对已保存的配置发起最小请求">
          {busy === "test" ? "测试中…" : "🔌 测试连接"}
        </button>
        {msg && <span className="settings-value">{msg}</span>}
      </div>
      <div className="settings-hint">
        API Key 以 XChaCha20-Poly1305 加密后存本地数据库，主密钥为应用数据目录下 0600 权限的密钥文件（仅本用户可读）；
        明文永不落盘、不出现在日志。测试连接使用已保存的配置。
      </div>
    </>
  );
}
