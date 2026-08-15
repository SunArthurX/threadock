// 成本 Section：按项目成本 + 预算卡 + 重算按钮
import { formatTokens, formatCost } from "./charts";
import type { DirCost, BudgetSettings } from "./ops-types";


interface Props {
  dirCosts: DirCost[];
  budget: BudgetSettings;
  monthUsage: { tokens: number; cost_usd: number } | null;
  budgetInput: { tokens: string; cost: string };
  loading: boolean;
  onBudgetInput: (field: "tokens" | "cost", value: string) => void;
  onSaveBudget: () => void;
  onRecalc: () => void;
}

export default function CostSection({
  dirCosts, budget, monthUsage, budgetInput, loading,
  onBudgetInput, onSaveBudget, onRecalc,
}: Props) {
  const tokenPct = budget.monthly_token_limit && monthUsage ? Math.min((monthUsage.tokens / budget.monthly_token_limit) * 100, 999) : null;
  const costPct = budget.monthly_cost_limit && monthUsage ? Math.min((monthUsage.cost_usd / budget.monthly_cost_limit) * 100, 999) : null;

  return (
    <>
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
      </div>
    </>
  );
}
