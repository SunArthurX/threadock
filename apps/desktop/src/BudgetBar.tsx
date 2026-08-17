// 顶栏全局预算条：当月实际 + 日均外推月底（超限变红，外推超限变黄提示）
import { formatCost, formatTokens } from "./charts";

export interface BudgetBarProps {
  /** 当月实际成本（USD）。 */
  costSoFar: number;
  /** 当月实际 tokens。 */
  tokensSoFar: number;
  /** 外推月底成本；null = 无预算限制。 */
  projectedCost: number | null;
  projectedTokens: number | null;
  costLimit: number | null;
  tokenLimit: number | null;
  /**
   * P2-3: 预算条点击回调（用于超限时引导跳转到成本页）。
   * 仅在 state 为 "warn" / "over" 时建议传入；未传则 bar 不可点。
   */
  onClick?: () => void;
}

/** 预算状态：ok / warning（外推超限）/ over（已超限）。 */
export function budgetState(props: BudgetBarProps): "ok" | "warning" | "over" {
  const { costSoFar, projectedCost, costLimit, tokensSoFar, projectedTokens, tokenLimit } = props;
  if (costLimit != null && costLimit > 0) {
    if (costSoFar >= costLimit) return "over";
    if (projectedCost != null && projectedCost >= costLimit) return "warning";
  }
  if (tokenLimit != null && tokenLimit > 0) {
    if (tokensSoFar >= tokenLimit) return "over";
    if (projectedTokens != null && projectedTokens >= tokenLimit) return "warning";
  }
  return "ok";
}

export default function BudgetBar(props: BudgetBarProps) {
  const { costSoFar, projectedCost, costLimit, tokensSoFar, tokenLimit } = props;
  if ((costLimit == null || costLimit <= 0) && (tokenLimit == null || tokenLimit <= 0)) {
    return null; // 未设预算不显示
  }
  const state = budgetState(props);
  const pct = costLimit && costLimit > 0 ? Math.min(100, (costSoFar / costLimit) * 100) : 0;
  // P2-3: 仅在 warn/over 时把 bar 渲染为可点击（避免误触）
  const interactive = !!props.onClick && (state === "warning" || state === "over");
  const titleText = projectedCost != null && costLimit
    ? `${interactive ? "点击跳转到成本页 · " : ""}当月 ${formatCost(costSoFar)} / 预算 ${formatCost(costLimit)} · 外推月底 ${formatCost(projectedCost)}`
    : `${interactive ? "点击跳转到成本页 · " : ""}当月 ${formatTokens(tokensSoFar)} tokens`;
  return (
    <div
      className={`budget-bar ${state} ${interactive ? "clickable" : ""}`}
      title={titleText}
      onClick={interactive ? props.onClick : undefined}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onKeyDown={interactive ? (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); props.onClick?.(); } } : undefined}
    >
      <div className="budget-bar-fill" style={{ width: `${pct}%` }} />
      <span className="budget-bar-label">
        {state === "over" ? "⚠ " : state === "warning" ? "◔ " : ""}
        {costLimit && costLimit > 0
          ? `${formatCost(costSoFar)} / ${formatCost(costLimit)}`
          : `${formatTokens(tokensSoFar)}`}
      </span>
    </div>
  );
}
