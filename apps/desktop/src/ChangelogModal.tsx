// 启动版本更新提示：localStorage 记录上次看到的版本，变化时显示本轮新增内容。
// 也可在「设置 → 关于」手动点击「查看更新日志」唤起。
import { useEffect, useState } from "react";
import { APP_VERSION } from "./SettingsView";
import ScrollArea from "./ScrollArea";
const SEEN_KEY = "ch-last-seen-version";

interface ChangelogEntry {
  version: string;
  date: string;
  highlights: string[];
}

const CHANGELOG: ChangelogEntry[] = [
  {
    version: "0.1.0",
    date: "2026-08-16",
    highlights: [
      "💰 成本：按 Provider 维度 + 按模型 Top10 + 月末超支预测 + 本周 vs 上周对比",
      "🛡 安全：「全部忽略/全部误报」一键 bulk + 策略规则 export/import JSON",
      "📊 报告：历史报告搜索 + 收藏（localStorage 持久化）",
      "🧩 资产：风险资产点击弹详情（路径/版本/风险点）",
      "📖 知识提取：类型筛选 tabs（摘要/决策/TODO/错误/命令/文件）+ MD/JSON 导出",
      "📆 活动：热力图加「按工具」维度切换",
      "💬 会话：列表排序选项（最新/创建/标题）+ Pin 置顶 + 多选批量",
      "🔍 搜索：搜索框 focus 下拉历史（10 条去重）",
      "⚙ 设置：显示偏好（数字格式 / 货币 / 日期格式）+ About 面板",
      "⌨ 快捷键：⌘? 速查面板 + ⌘F 焦点搜索 + ⌘R 手动刷新 + ⌘1..8 跳页",
      "🪟 Window title 反映当前页（OS 任务栏友好）",
      "💾 顶栏加备份按钮（一键定位到加密备份区）",
    ],
  },
];

export function getLastSeenVersion(): string | null {
  try { return localStorage.getItem(SEEN_KEY); } catch { return null; }
}
export function markVersionSeen(v: string) {
  try { localStorage.setItem(SEEN_KEY, v); } catch { /* 静默 */ }
}
export function shouldShowChangelog(): boolean {
  return getLastSeenVersion() !== APP_VERSION;
}

export default function ChangelogModal({ onClose }: { onClose: () => void }) {
  const [idx, setIdx] = useState(0);
  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onClose]);

  const close = () => { markVersionSeen(APP_VERSION); onClose(); };
  const cur = CHANGELOG[idx];

  return (
    <div className="settings-backdrop" onClick={close}>
      <div className="settings-modal changelog-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>🎉 已更新到 v{APP_VERSION}</h2>
          <button className="settings-close" onClick={close}>✕</button>
        </div>
        <ScrollArea className="settings-body changelog-body">
          <div className="changelog-tabs">
            {CHANGELOG.map((c, i) => (
              <button
                key={c.version}
                className={`filter-chip ${i === idx ? "active" : ""}`}
                onClick={() => setIdx(i)}
              >v{c.version} · {c.date}</button>
            ))}
          </div>
          <ul className="changelog-list">
            {cur.highlights.map((h, i) => (
              <li key={i}>{h}</li>
            ))}
          </ul>
          <div className="settings-hint" style={{ marginTop: 12 }}>
            本提示只在新版本首次启动时显示一次，后续可在「设置 → 关于 → 查看更新日志」手动唤起。
          </div>
        </ScrollArea>
        <div className="settings-footer">
          <button className="action-btn" onClick={close}>✓ 知道了</button>
        </div>
      </div>
    </div>
  );
}
