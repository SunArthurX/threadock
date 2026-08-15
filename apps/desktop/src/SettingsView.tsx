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
  onNavigate: (view: GovernanceView) => void;
  onReset: () => Promise<void>;
  resetting: boolean;
  onClose: () => void;
}

export default function SettingsView({
  theme, onThemeChange, syncIntervalMin, onSyncIntervalChange,
  onNavigate, onReset, resetting, onClose,
}: Props) {
  const [confirmText, setConfirmText] = useState("");
  const [lastConvSync, setLastConvSync] = useState<number | null>(null);
  const [lastOpsSync, setLastOpsSync] = useState<number | null>(null);
  const [opsSyncing, setOpsSyncing] = useState(false);
  const [opsMsg, setOpsMsg] = useState<string | null>(null);

  // 打开时读取只读的同步时间戳
  useEffect(() => {
    (async () => {
      try {
        const conv = await invoke<string | null>("app_setting_get", { key: "last_conv_sync_ms" });
        const ops = await invoke<string | null>("app_setting_get", { key: "last_ops_sync_ms" });
        setLastConvSync(conv ? Number(conv) : null);
        setLastOpsSync(ops ? Number(ops) : null);
      } catch { /* 只读展示，失败忽略 */ }
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
              手动入口在「📥 导入 → 增量同步」；指标采集另有 30 分钟节流（防止重复全量扫描）。
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
                {opsSyncing ? "⟳ 同步中…" : "立即全量同步指标"}
              </button>
              {opsMsg && <span className="settings-value">{opsMsg}</span>}
            </div>
          </section>

          <section className="settings-section">
            <h3>治理</h3>
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
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
