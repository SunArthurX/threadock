// 成本 Section：按项目成本 + 按 Provider 成本 + 预算卡 + 重算 + 超支预测
import { useMemo } from "react";
import { formatTokens, formatCost } from "./charts";
import { BarChart } from "./charts";
import type { DirCost, BudgetSettings, ProviderUsage } from "./ops-types";
import { meta } from "./ops-types";


export interface UsageSummary {
  month_tokens: number; month_cost: number;
  year_tokens: number; year_cost: number;
  all_tokens: number; all_cost: number;
}

/** 超支预测：按本月已用 × (本月总天数 / 已过天数) 推算全月消耗。
 *  - 输入 null → null（无数据）
 *  - dayOfMonth < 2 → null（数据不足）
 *  - 否则返回预测 tokens / cost + 上下文天数 */
export function calcProjection(
  monthUsage: { tokens: number; cost_usd: number } | null,
  now: Date = new Date(),
): { tokens: number; cost: number; dayOfMonth: number; daysInMonth: number } | null {
  if (!monthUsage) return null;
  const dayOfMonth = now.getDate();
  const daysInMonth = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
  if (dayOfMonth < 2) return null;
  const rate = dayOfMonth / daysInMonth;
  return {
    tokens: monthUsage.tokens / rate,
    cost: monthUsage.cost_usd / rate,
    dayOfMonth,
    daysInMonth,
  };
}

interface Props {
  dirCosts: DirCost[];
  byProvider: ProviderUsage[];
  budget: BudgetSettings;
  summary: UsageSummary | null;
  monthUsage: { tokens: number; cost_usd: number } | null;
  budgetInput: { tokens: string; cost: string };
  loading: boolean;
  onBudgetInput: (field: "tokens" | "cost", value: string) => void;
  onSaveBudget: () => void;
  onRecalc: () => void;
}

export default function CostSection({
  dirCosts, byProvider, budget, summary, monthUsage, budgetInput, loading,
  onBudgetInput, onSaveBudget, onRecalc,
}: Props) {
  const tokenPct = budget.monthly_token_limit && monthUsage ? Math.min((monthUsage.tokens / budget.monthly_token_limit) * 100, 999) : null;
  const costPct = budget.monthly_cost_limit && monthUsage ? Math.min((monthUsage.cost_usd / budget.monthly_cost_limit) * 100, 999) : null;

  // 超支预测（逻辑抽到 calcProjection 单测覆盖）
  const projection = useMemo(() => calcProjection(monthUsage), [monthUsage]);
  const projTokenOver = budget.monthly_token_limit && projection ? projection.tokens - budget.monthly_token_limit : null;
  const projCostOver = budget.monthly_cost_limit && projection ? projection.cost - budget.monthly_cost_limit : null;

  const budgetCard = (
      <div className="ops-card ops-budget">
        <div className="ops-card-title">月度预算</div>
        <div className="budget-grid">
          <div className="budget-item">
            <div className="budget-label">本月 Tokens</div>
            <div className="budget-value">{monthUsage ? formatTokens(monthUsage.tokens) : "—"}</div>
            {tokenPct != null && (<>
              <div className="budget-progress"><div className={`budget-progress-bar ${tokenPct >= 100 ? "over" : ""}`} style={{ width: `${Math.min(tokenPct, 100)}%` }} /></div>
              <div className="budget-pct">{Math.round(tokenPct)}% / {formatTokens(budget.monthly_token_limit!)}</div>
            </>)}
          </div>
          <div className="budget-item">
            <div className="budget-label">本月成本</div>
            <div className="budget-value">{monthUsage ? formatCost(monthUsage.cost_usd) : "—"}</div>
            {costPct != null && (<>
              <div className="budget-progress"><div className={`budget-progress-bar ${costPct >= 100 ? "over" : ""}`} style={{ width: `${Math.min(costPct, 100)}%` }} /></div>
              <div className="budget-pct">{Math.round(costPct)}% / {formatCost(budget.monthly_cost_limit!)}</div>
            </>)}
          </div>
          <div className="budget-item budget-edit">
            <div className="budget-label">阈值设置</div>
            <div className="budget-inputs">
              <input type="text" placeholder="Token 上限" value={budgetInput.tokens}
                onChange={(e) => onBudgetInput("tokens", e.target.value)} />
              <input type="text" placeholder="成本上限 $" value={budgetInput.cost}
                onChange={(e) => onBudgetInput("cost", e.target.value)} />
              <button className="action-btn" onClick={onSaveBudget}>保存预算</button>
            </div>
          </div>
        </div>
        {summary && (
          <div className="summary-strip">
            <span>本年 <b>{formatTokens(summary.year_tokens)}</b> · <b>{formatCost(summary.year_cost)}</b></span>
            <span>全部 <b>{formatTokens(summary.all_tokens)}</b> · <b>{formatCost(summary.all_cost)}</b></span>
            <span>本月 <b>{formatTokens(summary.month_tokens)}</b> · <b>{formatCost(summary.month_cost)}</b></span>
          </div>
        )}
      </div>
  );

  return (
    <>
      {budgetCard}
      {/* 超支预测（基于本月速率外推） */}
      {projection && (projTokenOver != null || projCostOver != null) && (
        <div className={`ops-card projection-card ${(projTokenOver ?? 0) > 0 || (projCostOver ?? 0) > 0 ? "over" : "ok"}`}>
          <div className="ops-card-title">
            🔮 月末预测
            <span className="ops-card-sub">按当前速率（本月 {projection.dayOfMonth}/{projection.daysInMonth} 天）外推</span>
          </div>
          <div className="projection-grid">
            {projTokenOver != null && (
              <div className="projection-item">
                <span className="projection-label">预测 Tokens</span>
                <span className="projection-value">{formatTokens(projection.tokens)}</span>
                {budget.monthly_token_limit && (
                  <span className={`projection-delta ${projTokenOver > 0 ? "over" : "ok"}`}>
                    {projTokenOver > 0
                      ? `超 ${formatTokens(projTokenOver)} （${((projTokenOver / budget.monthly_token_limit) * 100).toFixed(0)}%）`
                      : `剩余 ${formatTokens(-projTokenOver)}`}
                  </span>
                )}
              </div>
            )}
            {projCostOver != null && (
              <div className="projection-item">
                <span className="projection-label">预测成本</span>
                <span className="projection-value">{formatCost(projection.cost)}</span>
                {budget.monthly_cost_limit && (
                  <span className={`projection-delta ${projCostOver > 0 ? "over" : "ok"}`}>
                    {projCostOver > 0
                      ? `超 ${formatCost(projCostOver)}`
                      : `剩余 ${formatCost(-projCostOver)}`}
                  </span>
                )}
              </div>
            )}
          </div>
        </div>
      )}
      <div className="ops-card">
        <div className="ops-card-title">
          按项目成本 Top10
          <button className="action-btn" style={{ marginLeft: "auto", fontSize: 11 }} onClick={onRecalc}>$ 重算</button>
        </div>
        {dirCosts.length === 0 ? (
          loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">暂无数据</div>
        ) : (
          <table className="ops-table">
            <thead><tr><th>项目目录</th><th>Tokens</th><th>成本</th><th>请求</th></tr></thead>
            <tbody>
              {dirCosts.map((d, i) => (
                <tr key={i}>
                  <td className="mono" title={d.dir}>{d.dir.split("/").slice(-2).join("/")}</td>
                  <td>{formatTokens(d.tokens)}</td>
                  <td>{formatCost(d.cost_usd)}</td>
                  <td>{d.requests.toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* 按 Provider 维度成本对比（与项目维度互补） */}
      {byProvider && byProvider.length > 0 && (
        <div className="ops-card">
          <div className="ops-card-title">按 Agent（Provider）成本分布</div>
          <BarChart
            data={byProvider.map((p) => ({ label: meta(p.provider).label, value: p.cost_usd }))}
            height={120}
            axisLabel={(d) => d.label}
            renderTooltip={(d, max) => {
              const prov = byProvider.find((p) => meta(p.provider).label === d.label);
              return (
                <>
                  <div className="tooltip-title">{d.label}</div>
                  <div className="tooltip-row">
                    <span className="tooltip-dot" style={{ background: "var(--accent)" }} />
                    <span>成本 <b style={{ marginLeft: 4 }}>{formatCost(d.value)}</b>（{((d.value / max) * 100).toFixed(0)}% of peak）</span>
                  </div>
                  {prov && (
                    <>
                      <div className="tooltip-row" style={{ marginTop: 2 }}>
                        <span>Tokens <b style={{ marginLeft: 4 }}>{formatTokens(prov.total_tokens)}</b></span>
                      </div>
                      <div className="tooltip-row">
                        <span>请求 <b style={{ marginLeft: 4 }}>{prov.requests.toLocaleString()}</b></span>
                      </div>
                    </>
                  )}
                </>
              );
            }}
          />
        </div>
      )}
    </>
  );
}
