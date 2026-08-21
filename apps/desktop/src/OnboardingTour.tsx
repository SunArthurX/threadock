// 首次启动引导：6 步串讲核心功能，让新用户 30 秒内知道「这工具能做什么、点哪儿」。
// localStorage 记录是否已走完；走完后只显示「?」悬浮按钮供重新唤起。
// 区分 empty（0 数据新用户）与 existing（已有数据老用户）两种引导内容。
import { useEffect, useState } from "react";
import ScrollArea from "./ScrollArea";
import { Icon } from "./Icon";
const SEEN_KEY = "ch-onboarding-seen";
const STEP_KEY = "ch-onboarding-step";

export function isOnboardingSeen(): boolean {
  try { return localStorage.getItem(SEEN_KEY) === "1"; } catch { return false; }
}
export function markOnboardingSeen() {
  try {
    localStorage.setItem(SEEN_KEY, "1");
    localStorage.removeItem(STEP_KEY);
  } catch { /* 静默 */ }
}
export function resetOnboarding() {
  try {
    localStorage.removeItem(SEEN_KEY);
    localStorage.removeItem(STEP_KEY);
  } catch { /* 静默 */ }
}
export function loadOnboardingStep(): number {
  try {
    const v = Number(localStorage.getItem(STEP_KEY) ?? "0");
    return Number.isFinite(v) && v >= 0 ? v : 0;
  } catch { return 0; }
}
export function saveOnboardingStep(s: number) {
  try { localStorage.setItem(STEP_KEY, String(s)); } catch { /* 静默 */ }
}

interface Step {
  title: string;
  icon: "import" | "command" | "compass" | "library" | "shield" | "settings" | "chart";
  body: string;
  hint?: string;
}

// 「empty」模式（默认）：0 数据新用户，导入是核心动作。
// 「existing」模式：库中已有 N 条对话，跳过导入，直接引导搜索/回顾。
const STEPS_EMPTY: Step[] = [
  {
    icon: "import",
    title: "导入你的 AI 工具对话",
    body: "点左上角的「同步」按钮，把 Cursor / Claude Code / ZCode / Codex 里的历史对话拉进 Threadock，统一管理。",
    hint: "导入完成后左侧会出现「来源筛选」chip，可以按工具单独看。",
  },
  {
    icon: "command",
    title: "命令面板：⌘K 唤起",
    body: "任意页面按 ⌘K（macOS）/ Ctrl+K（其它）唤起命令面板：搜索会话、按关键字跳页、运行操作。",
    hint: "试试输入「成本」「活动」会直接跳到对应页。",
  },
  {
    icon: "compass",
    title: "八个视图各有侧重",
    body: "左侧 8 个 tab：对话 / 概览 / 成本 / 安全 / 资产 / 知识库 / 活动 / 项目。⌘1..⌘8 一键切换。",
    hint: "活动页是热力图 + 24h 柱状图，看你最常在哪个时段用 AI。",
  },
  {
    icon: "library",
    title: "知识提取：把对话变资产",
    body: "打开任意会话，按 ⌘K 选「提取知识」会抽出摘要 / 决策 / TODO / 错误 / 命令 / 文件 6 类，自动进知识库。",
    hint: "知识库支持跨会话引用 —— 在 A 会话提取的结论，能在 B 会话被自动找到。",
  },
  {
    icon: "shield",
    title: "安全审计 + 一键处理",
    body: "安全 tab 会扫描所有会话里的风险操作（删库命令 / 凭据泄露 / 危险路径写入等），可勾选后批量「忽略」或「标记误报」。",
    hint: "支持导出 / 导入策略规则，换台机器也能继承。",
  },
  {
    icon: "settings",
    title: "设置：偏好、备份、关于",
    body: "右上角齿轮进设置：显示偏好（数字格式 / 货币 / 日期）、自动同步间隔、保留策略、预算超支通知、About + 更新日志。",
    hint: "「设置 → 数据」里能一键定位到加密备份目录，也可以导入 / 导出偏好 JSON。",
  },
];

// 「existing」模式：库中已有对话，第 1 步是回顾/搜索，跳过导入。
const STEPS_EXISTING: Step[] = [
  {
    icon: "chart",
    title: "回顾你的历史 / 试试搜索 ⌘F",
    body: "库中已有对话了。先按 ⌘F 焦点搜索框（macOS）/ Ctrl+F（其它）找一条试试，或点开「概览」看整体趋势。",
    hint: "试试按来源筛选 chip：Cursor / Claude Code / ZCode / Codex 单独看。",
  },
  {
    icon: "command",
    title: "命令面板：⌘K 唤起",
    body: "任意页面按 ⌘K（macOS）/ Ctrl+K（其它）唤起命令面板：搜索会话、按关键字跳页、运行操作。",
    hint: "试试输入「成本」「活动」会直接跳到对应页。",
  },
  {
    icon: "compass",
    title: "八个视图各有侧重",
    body: "左侧 8 个 tab：对话 / 概览 / 成本 / 安全 / 资产 / 知识库 / 活动 / 项目。⌘1..⌘8 一键切换。",
    hint: "活动页是热力图 + 24h 柱状图，看你最常在哪个时段用 AI。",
  },
  {
    icon: "library",
    title: "知识提取：把对话变资产",
    body: "打开任意会话，按 ⌘K 选「提取知识」会抽出摘要 / 决策 / TODO / 错误 / 命令 / 文件 6 类，自动进知识库。",
    hint: "知识库支持跨会话引用 —— 在 A 会话提取的结论，能在 B 会话被自动找到。",
  },
  {
    icon: "shield",
    title: "安全审计 + 一键处理",
    body: "安全 tab 会扫描所有会话里的风险操作（删库命令 / 凭据泄露 / 危险路径写入等），可勾选后批量「忽略」或「标记误报」。",
    hint: "支持导出 / 导入策略规则，换台机器也能继承。",
  },
  {
    icon: "settings",
    title: "设置：偏好、备份、关于",
    body: "右上角齿轮进设置：显示偏好（数字格式 / 货币 / 日期）、自动同步间隔、保留策略、预算超支通知、About + 更新日志。",
    hint: "「设置 → 数据」里能一键定位到加密备份目录，也可以导入 / 导出偏好 JSON。",
  },
];

export default function OnboardingTour({
  onClose,
  startStep = 0,
  dataMode,
  existingCount,
}: {
  onClose: () => void;
  startStep?: number;
  /** 数据模式：未传时按 "empty" 兜底。Cluster 1 在 App.tsx 根据 conversations.length 决定。 */
  dataMode?: "empty" | "existing";
  /** 已有对话数（用于动态化「已有 N 条…」提示文案；undefined 时不显示该 hint）。 */
  existingCount?: number;
}) {
  // 决定 step 列表与 step 1 的「已有 N 条」分支
  const effectiveMode: "empty" | "existing" =
    dataMode ?? (typeof existingCount === "number" && existingCount > 0 ? "existing" : "empty");
  const baseSteps = effectiveMode === "existing" ? STEPS_EXISTING : STEPS_EMPTY;
  const steps: Step[] = baseSteps;
  // step 1 上的「已有 N 条对话？跳过前 3 步直接看搜索」分支（existingCount > 0 时显示）
  const showExistingBranch = typeof existingCount === "number" && existingCount > 0;

  const [step, setStep] = useState(() => {
    const v = startStep > 0 ? startStep : loadOnboardingStep();
    return v >= 0 && v < steps.length ? v : 0;
  });

  useEffect(() => { saveOnboardingStep(step); }, [step]);

  const finish = () => { markOnboardingSeen(); onClose(); };

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowRight" || e.key === "Enter") {
        if (step < steps.length - 1) setStep(step + 1);
        else finish();
      } else if (e.key === "ArrowLeft" && step > 0) setStep(step - 1);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, steps.length]);

  const cur = steps[step];
  const isLast = step === steps.length - 1;

  return (
    <div className="settings-backdrop" onClick={finish}>
      <div
        className="settings-modal onboarding-modal"
        onClick={(e) => e.stopPropagation()}
        data-testid="onboarding-tour"
        data-step={step}
        data-mode={effectiveMode}
      >
        <div className="settings-header">
          <h2>
            <span className="onboarding-step-icon"><Icon name={cur.icon} size={15} /></span>
            {effectiveMode === "existing" ? "欢迎回到 Threadock" : "欢迎使用 Threadock"}
          </h2>
          <button className="settings-close" onClick={finish} aria-label="跳过引导"><Icon name="close" size={14} /></button>
        </div>
        <ScrollArea className="settings-body onboarding-body">
          <div className="onboarding-progress" aria-hidden>
            {steps.map((_, i) => (
              <div
                key={i}
                className={`onboarding-dot ${i === step ? "active" : ""} ${i < step ? "done" : ""}`}
                onClick={() => setStep(i)}
              />
            ))}
          </div>
          <div className="onboarding-step-title">
            <span className="onboarding-step-num">{step + 1} / {steps.length}</span>
            <h3>{cur.title}</h3>
          </div>
          <p className="onboarding-text">{cur.body}</p>
          {cur.hint && <div className="onboarding-hint"><Icon name="sparkle" size={12} /><span>{cur.hint}</span></div>}
          {step === 0 && showExistingBranch && (
            <div className="onboarding-hint onboarding-hint-branch" data-testid="onboarding-existing-branch">
              <Icon name="sparkle" size={12} /><span>已有 {existingCount} 条对话？跳过前 3 步直接看搜索</span>
            </div>
          )}
        </ScrollArea>
        <div className="settings-footer onboarding-footer">
          <button
            className="action-btn"
            onClick={() => setStep(Math.max(0, step - 1))}
            disabled={step === 0}
            data-testid="onboarding-prev"
          >
            <Icon name="chevron-left" size={12} /> 上一步
          </button>
          <span className="onboarding-skip-hint" onClick={finish} role="button" tabIndex={0}>
            跳过引导
          </span>
          <div style={{ flex: 1 }} />
          {isLast ? (
            <button className="action-btn primary" onClick={finish} data-testid="onboarding-finish">
              开始使用
            </button>
          ) : (
            <button
              className="action-btn primary"
              onClick={() => setStep(step + 1)}
              data-testid="onboarding-next"
            >
              下一步 <Icon name="chevron-right" size={12} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
