// 成本 Section：按项目成本 + 按 Provider 成本 + 按模型成本 + 预算卡 + 重算 + 超支预测 + 周对比
import { useMemo } from "react";
import { formatTokens, formatCost } from "./charts";
import { BarChart } from "./charts";
import { CardTitle } from "./CardTitle";
import { Skeleton } from "./Skeleton";
import { InlineEmpty } from "./EmptyState";
import type { DirCost, BudgetSettings, ProviderUsage, ModelUsage, DailyUsage } from "./ops-types";
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

/** 本周 vs 上周成本对比。
 *  - timeseries 至少 7 天（无上次则 lastWeek=0，对比显示「+∞%」不可靠 → 返回 null）。
 *  - P1-B4: 阈值由 14 天降到 7 天；当只有 7 天数据时（lastWeek=0）需要 UI 给出
 *    "1 周数据 vs 前 1 周（数据较少）" 提示（避免误读为「本周 vs 无」）。
 *  - costRatio: 每 1M tokens 单价（默认 4 美元，对 Claude/Sonnet 中位粗估，仅作对比展示）
 *  - 返回 {thisWeek, lastWeek, costDelta, tokenDelta, costPct, tokenPct} */
export function weekOverWeek(
  timeseries: { day: string; total_tokens: number; requests: number }[] | null | undefined,
  costPerMTokens: number = 4,
): {
  thisWeek: { cost: number; tokens: number; requests: number };
  lastWeek: { cost: number; tokens: number; requests: number };
  costDelta: number; tokenDelta: number;
  costPct: number; tokenPct: number;
} | null {
  if (!timeseries || timeseries.length < 7) return null;
  const last = timeseries.slice(-7);
  const prev = timeseries.slice(-14, -7);
  const t = (arr: typeof timeseries) => arr.reduce((s, x) => s + (x.total_tokens || 0), 0);
  const r = (arr: typeof timeseries) => arr.reduce((s, x) => s + (x.requests || 0), 0);
  const thisT = t(last); const prevT = t(prev);
  const thisR = r(last); const prevR = r(prev);
  const thisC = (thisT / 1_000_000) * costPerMTokens;
  const prevC = (prevT / 1_000_000) * costPerMTokens;
  return {
    thisWeek: { tokens: thisT, requests: thisR, cost: thisC },
    lastWeek: { tokens: prevT, requests: prevR, cost: prevC },
    costDelta: thisC - prevC,
    tokenDelta: thisT - prevT,
    costPct: prevC > 0 ? (thisC - prevC) / prevC : 0,
    tokenPct: prevT > 0 ? (thisT - prevT) / prevT : 0,
  };
}

interface Props {
  dirCosts: DirCost[];
  byProvider: ProviderUsage[];
  byModel?: ModelUsage[];
  timeseries?: DailyUsage[];
  budget: BudgetSettings;
  summary: UsageSummary | null;
  monthUsage: { tokens: number; cost_usd: number } | null;
  budgetInput: { tokens: string; cost: string };
  loading: boolean;
  onBudgetInput: (field: "tokens" | "cost", value: string) => void;
  onSaveBudget: () => void;
  onRecalc: () => void;
  /**
   * 按目录维度跳转会话列表（行点击触发）。
   * TODO: chat view 暂无 dir 维度的原生 filter；目前由 OpsView 退化为
   *       setView("chat") + setSearchQuery("dir:<value>")，后续应替换为
   *       原生 filter 状态。
   */
  onJumpByDir?: (dir: string) => void;
  /**
   * 按模型维度跳转会话列表（行点击触发）。
   * TODO: chat view 暂无 model 维度的原生 filter；目前由 OpsView 退化为
   *       setView("chat") + setSearchQuery("model:<value>")，后续应替换为
   *       原生 filter 状态。
   */
  onJumpByModel?: (model: string) => void;
}

export default function CostSection({
  dirCosts, byProvider, byModel, timeseries, budget, summary, monthUsage, budgetInput, loading,
  onBudgetInput, onSaveBudget, onRecalc, onJumpByDir, onJumpByModel,
}: Props) {
  const tokenPct = budget.monthly_token_limit && monthUsage ? Math.min((monthUsage.tokens / budget.monthly_token_limit) * 100, 999) : null;
  const costPct = budget.monthly_cost_limit && monthUsage ? Math.min((monthUsage.cost_usd / budget.monthly_cost_limit) * 100, 999) : null;

  // 超支预测（逻辑抽到 calcProjection 单测覆盖）
  const projection = useMemo(() => calcProjection(monthUsage), [monthUsage]);

  // 本周 vs 上周对比（纯函数，可单测）
  const wow = useMemo(() => weekOverWeek(timeseries), [timeseries]);
  const projTokenOver = budget.monthly_token_limit && projection ? projection.tokens - budget.monthly_token_limit : null;
  const projCostOver = budget.monthly_cost_limit && projection ? projection.cost - budget.monthly_cost_limit : null;

  const budgetCard = (
      <div className="ops-card ops-budget">
        <CardTitle icon="dollar" sub="按当前速率自动检测超支并提醒">月度预算</CardTitle>
        <div className="budget-grid">
          {/* 左侧两个大数字 — 进度条 + 数字突出 */}
          <div className="budget-stat">
            <div className="budget-stat-label">本月 Tokens</div>
            <div className="budget-stat-value">
              {monthUsage ? formatTokens(monthUsage.tokens) : <span className="budget-stat-empty">—</span>}
            </div>
            {tokenPct != null ? (
              <>
                <div className="budget-progress">
                  <div className={`budget-progress-bar ${tokenPct >= 100 ? "over" : tokenPct >= 80 ? "warn" : ""}`}
                    style={{ width: `${Math.min(tokenPct, 100)}%` }} />
                </div>
                <div className="budget-pct">
                  <span className="budget-pct-num">{Math.round(tokenPct)}%</span>
                  <span className="budget-pct-sep">/</span>
                  <span className="budget-pct-cap">上限 {formatTokens(budget.monthly_token_limit!)}</span>
                </div>
              </>
            ) : (
              <div className="budget-stat-hint">未设置上限</div>
            )}
          </div>
          <div className="budget-stat">
            <div className="budget-stat-label">本月成本</div>
            <div className="budget-stat-value">
              {monthUsage ? formatCost(monthUsage.cost_usd) : <span className="budget-stat-empty">—</span>}
            </div>
            {costPct != null ? (
              <>
                <div className="budget-progress">
                  <div className={`budget-progress-bar ${costPct >= 100 ? "over" : costPct >= 80 ? "warn" : ""}`}
                    style={{ width: `${Math.min(costPct, 100)}%` }} />
                </div>
                <div className="budget-pct">
                  <span className="budget-pct-num">{Math.round(costPct)}%</span>
                  <span className="budget-pct-sep">/</span>
                  <span className="budget-pct-cap">上限 {formatCost(budget.monthly_cost_limit!)}</span>
                </div>
              </>
            ) : (
              <div className="budget-stat-hint">未设置上限</div>
            )}
          </div>
          {/* 右侧：Apple-style Form 字段，label 在上、input 在下 */}
          <div className="budget-form">
            <div className="budget-form-field">
              <label className="budget-form-label" htmlFor="budget-tokens">Token 上限</label>
              <input id="budget-tokens" className="budget-form-input" type="text" inputMode="numeric"
                placeholder="例如 5000000" value={budgetInput.tokens}
                onChange={(e) => onBudgetInput("tokens", e.target.value)} />
            </div>
            <div className="budget-form-field">
              <label className="budget-form-label" htmlFor="budget-cost">成本上限（USD）</label>
              <input id="budget-cost" className="budget-form-input" type="text" inputMode="decimal"
                placeholder="例如 50" value={budgetInput.cost}
                onChange={(e) => onBudgetInput("cost", e.target.value)} />
            </div>
            <button className="budget-form-submit" onClick={onSaveBudget}>
              保存预算
            </button>
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
          <CardTitle icon="trending" sub={`按当前速率（本月 ${projection.dayOfMonth}/${projection.daysInMonth} 天）外推`}>月末预测</CardTitle>
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
        <CardTitle icon="folder" trailing={<button className="action-btn" onClick={onRecalc}>重算</button>}>按项目成本 Top10</CardTitle>
        {dirCosts.length === 0 ? (
          loading ? <Skeleton variant="list" count={5} /> : <InlineEmpty message="暂无项目成本数据" hint="导入并使用 Agent 后将按 source_dir 自动归并" />
        ) : (
          <table className="ops-table">
            <thead><tr><th>项目目录</th><th>Tokens</th><th>成本</th><th>请求</th>{onJumpByDir && <th></th>}</tr></thead>
            <tbody>
              {dirCosts.map((d, i) => (
                <tr key={i} className={onJumpByDir ? "ops-row-clickable" : ""} onClick={onJumpByDir ? () => onJumpByDir(d.dir) : undefined}>
                  <td className="mono" title={d.dir}>{d.dir.split("/").slice(-2).join("/")}</td>
                  <td>{formatTokens(d.tokens)}</td>
                  <td>{formatCost(d.cost_usd)}</td>
                  <td>{d.requests.toLocaleString()}</td>
                  {onJumpByDir && <td><button className="finding-btn" title={`查看目录 ${d.dir} 的会话`} onClick={(e) => { e.stopPropagation(); onJumpByDir(d.dir); }}>→ 列表</button></td>}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* 按 Provider 维度成本对比（与项目维度互补） */}
      {byProvider && byProvider.length > 0 && (
        <div className="ops-card">
          <CardTitle icon="globe">按 Agent（Provider）成本分布</CardTitle>
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

      {/* 按模型成本 Top10（看清哪个模型最烧钱） */}
      {byModel && byModel.length > 0 && (
        <div className="ops-card">
          <CardTitle icon="cpu">按模型成本 Top10</CardTitle>
          <div className="ops-table-wrap">
            <table className="ops-table">
              <thead><tr><th>模型</th><th>Provider</th><th>成本</th><th>Tokens</th><th>请求</th><th>错误</th>{onJumpByModel && <th></th>}</tr></thead>
              <tbody>
                {byModel.slice(0, 10).map((m, i) => (
                  <tr key={i} className={onJumpByModel ? "ops-row-clickable" : ""} onClick={onJumpByModel ? () => onJumpByModel(m.model) : undefined}>
                    <td className="mono" title={m.model}>{m.model.length > 28 ? m.model.slice(0, 28) + "…" : m.model}</td>
                    <td><span className={`badge source ${m.provider_id}`}>{meta(m.provider_id).label}</span></td>
                    <td><b>{formatCost(m.cost_usd)}</b></td>
                    <td>{formatTokens(m.input_tokens + m.output_tokens)}</td>
                    <td>{m.requests.toLocaleString()}</td>
                    <td className={m.errors > 0 ? "text-danger" : ""}>{m.errors}</td>
                    {onJumpByModel && <td><button className="finding-btn" title={`查看模型 ${m.model} 的会话`} onClick={(e) => { e.stopPropagation(); onJumpByModel(m.model); }}>→ 列表</button></td>}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* 本周 vs 上周 对比卡（无数据时隐藏） */}
      {wow && (
        <div className="ops-card">
          <CardTitle icon="calendar" sub={timeseries && timeseries.length < 14 ? "1 周数据 vs 前 1 周（数据较少）" : "成本按 $4/M tokens 中位估算"}>本周 vs 上周</CardTitle>
          <div className="wow-grid">
            <div className="wow-col">
              <div className="wow-label">本周</div>
              <div className="wow-value">{formatCost(wow.thisWeek.cost)}</div>
              <div className="wow-sub">{formatTokens(wow.thisWeek.tokens)} · {wow.thisWeek.requests.toLocaleString()} 请求</div>
            </div>
            <div className="wow-col">
              <div className="wow-label">上周</div>
              <div className="wow-value">{formatCost(wow.lastWeek.cost)}</div>
              <div className="wow-sub">{formatTokens(wow.lastWeek.tokens)} · {wow.lastWeek.requests.toLocaleString()} 请求</div>
            </div>
            <div className="wow-col wow-delta">
              <div className="wow-label">变化</div>
              <div className={`wow-value ${wow.costDelta > 0 ? "up" : wow.costDelta < 0 ? "down" : ""}`}>
                {wow.costDelta >= 0 ? "▲" : "▼"} {formatCost(Math.abs(wow.costDelta))}
              </div>
              <div className={`wow-sub ${wow.tokenDelta > 0 ? "up" : wow.tokenDelta < 0 ? "down" : ""}`}>
                {wow.tokenDelta >= 0 ? "+" : ""}{formatTokens(wow.tokenDelta)}（{(wow.tokenPct * 100).toFixed(1)}%）
              </div>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
