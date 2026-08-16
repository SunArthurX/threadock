// 快捷键速查面板：⌘? / Ctrl+? 唤起。
// 列出所有全局快捷键及作用域，便于用户快速记忆与发现隐藏功能。
import { useEffect } from "react";
import ScrollArea from "./ScrollArea";
interface ShortcutItem {
  keys: string;
  desc: string;
  scope?: "global" | "对话" | "列表" | "详情";
}

interface Group {
  title: string;
  items: ShortcutItem[];
}

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const MOD = isMac ? "⌘" : "Ctrl";

const GROUPS: Group[] = [
  {
    title: "全局",
    items: [
      { keys: `${MOD} K`, desc: "唤起 / 关闭命令面板（搜索会话/跳页）", scope: "global" },
      { keys: `${MOD} ?`, desc: "唤起 / 关闭本快捷键速查", scope: "global" },
      { keys: `${MOD} 1`, desc: "跳到「对话」", scope: "global" },
      { keys: `${MOD} 2`, desc: "跳到「概览」", scope: "global" },
      { keys: `${MOD} 3`, desc: "跳到「成本」", scope: "global" },
      { keys: `${MOD} 4`, desc: "跳到「安全」", scope: "global" },
      { keys: `${MOD} 5`, desc: "跳到「资产」", scope: "global" },
      { keys: `${MOD} 6`, desc: "跳到「知识库」", scope: "global" },
      { keys: `${MOD} 7`, desc: "跳到「活动」", scope: "global" },
      { keys: `${MOD} 8`, desc: "跳到「项目」", scope: "global" },
      { keys: "Esc", desc: "关闭弹窗 / 取消选择", scope: "global" },
    ],
  },
  {
    title: "对话列表",
    items: [
      { keys: "Enter", desc: "打开选中会话", scope: "列表" },
      { keys: "Space", desc: "勾选/取消多选", scope: "列表" },
      { keys: `${MOD} A`, desc: "全选当前页", scope: "列表" },
      { keys: `${MOD} D`, desc: "删除选中（带撤销）", scope: "列表" },
      { keys: `${MOD} E`, desc: "归档/取消归档", scope: "列表" },
      { keys: "F", desc: "收藏/取消收藏", scope: "列表" },
    ],
  },
  {
    title: "会话详情",
    items: [
      { keys: `${MOD} F`, desc: "详情页内搜索", scope: "详情" },
      { keys: `${MOD} K`, desc: "知识提取", scope: "详情" },
      { keys: `${MOD} E`, desc: "导出当前会话", scope: "详情" },
      { keys: "T", desc: "切换时间线模式", scope: "详情" },
      { keys: "A", desc: "归档/取消归档", scope: "详情" },
      { keys: "F", desc: "收藏/取消收藏", scope: "详情" },
    ],
  },
  {
    title: "报告 / 治理",
    items: [
      { keys: "Enter", desc: "打开选中的历史报告", scope: "对话" },
      { keys: "Esc", desc: "关闭报告 / 弹窗", scope: "对话" },
    ],
  },
];

export default function HelpShortcuts({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings-modal help-shortcuts-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>⌨ 快捷键速查</h2>
          <div className="knowledge-modal-actions">
            <span className="settings-hint" style={{ fontSize: 11 }}>{isMac ? "macOS" : "Windows / Linux"} 平台</span>
            <button className="settings-close" onClick={onClose}>✕</button>
          </div>
        </div>
        <ScrollArea className="settings-body help-shortcuts-body">
          {GROUPS.map((g) => (
            <section key={g.title} className="help-shortcuts-group">
              <h3>{g.title}</h3>
              <div className="help-shortcuts-table">
                {g.items.map((it, i) => (
                  <div key={i} className="help-shortcuts-row">
                    <kbd className="kbd-keys">{it.keys}</kbd>
                    <span className="help-shortcuts-desc">{it.desc}</span>
                  </div>
                ))}
              </div>
            </section>
          ))}
          <div className="help-shortcuts-hint">
            提示：{MOD} 在 macOS 是 ⌘ (Command)；其它平台是 Ctrl。
          </div>
        </ScrollArea>
      </div>
    </div>
  );
}
