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
import { formatTime } from "./types";

/** 重置确认词：输入完全一致才允许执行（防误触）。 */
export const RESET_CONFIRM_TEXT = "重置";

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
  syncIntervalMin: number;
  onSyncIntervalChange: (min: number) => void;
  retentionDays: number;
  onRetentionDaysChange: (days: number) => void;
  notifyOnExceed: boolean;
  onNotifyOnExceedChange: (v: boolean) => void;
  onNavigate: (view: GovernanceView) => void;
  onReset: () => Promise<void>;
  resetting: boolean;
  onClose: () => void;
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
  theme, onThemeChange, syncIntervalMin, onSyncIntervalChange,
  retentionDays, onRetentionDaysChange, notifyOnExceed, onNotifyOnExceedChange,
  onNavigate, onReset, resetting, onClose,
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
  const [lastConvSync, setLastConvSync] = useState<number | null>(null);
  const [lastOpsSync, setLastOpsSync] = useState<number | null>(null);
  const [opsSyncing, setOpsSyncing] = useState(false);
  const [opsMsg, setOpsMsg] = useState<string | null>(null);

  // 打开时读取只读的同步时间戳 + 存储看板 + 治理流水 + 周报时间
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

  const canReset = confirmText === RESET_CONFIRM_TEXT && !resetting;

  const doReset = async () => {
    if (!canReset) return;
    await onReset();
    setConfirmText("");
  };

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>⚙ 设置</h2>
          <button className="settings-close" onClick={onClose}>✕</button>
        </div>

        <div className="settings-body">
          <section className="settings-section">
            <h3>外观</h3>
            <div className="settings-row">
              <span>主题</span>
              <div className="settings-segment">
                <button className={theme === "dark" ? "active" : ""} onClick={() => onThemeChange("dark")}>☾ 深色</button>
                <button className={theme === "light" ? "active" : ""} onClick={() => onThemeChange("light")}>☀ 浅色</button>
              </div>
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
              手动入口在「⬇ 导入 → 增量同步」；指标采集另有 30 分钟节流（防止重复全量扫描）。
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

          <section className="settings-section danger">
            <h3>数据</h3>
            <div className="settings-hint">
              重置将清空所有会话、消息、指标与搜索索引（保留数据库结构与自定义脱敏规则），不可撤销。
            </div>
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
              <button className="reset-confirm-btn" disabled={!canReset} onClick={doReset}>
                {resetting ? "重置中…" : "重置所有数据"}
              </button>
              <MiniProgress p={mini} />
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

/** 加密备份/恢复（本地，密码仅进程内使用）。 */
function BackupSection() {
  const [pw, setPw] = useState("");
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
