// CodeAgentOps 治理视图（plan codeagent-ops M3/M4/M5）
// KPI 卡 + Agent 分布 donut + 每日趋势 bar + 模型/工具榜单 + 风险调用
// + 安全审计（M4）+ 预算与成本（M5）

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { BarChart, DonutChart, formatCost, formatDuration, formatTokens } from "./charts";

interface OpsOverview {
  total_requests: number;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
  avg_duration_ms: number;
  error_count: number;
  session_count: number;
  destructive_calls: number;
  total_tool_calls: number;
}

interface ProviderUsage {
  provider: string;
  requests: number;
  total_tokens: number;
  output_tokens: number;
  errors: number;
}

interface ModelUsage {
  model: string;
  provider_id: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  errors: number;
}

interface DailyUsage {
  day: string;
  total_tokens: number;
  requests: number;
}

interface ToolUsageRow {
  tool_name: string;
  calls: number;
  destructive: number;
  errors: number;
  avg_duration_ms: number;
}

interface RiskyCall {
  id: string;
  provider: string;
  source_session_id: string;
  tool_name: string;
  ts: number;
  destructive: boolean | null;
  approval_status: string | null;
  exit_code: number | null;
  duration_ms: number | null;
  command_text: string | null;
}

interface AuditFinding {
  kind: string;
  severity: "low" | "medium" | "high";
  rule: string;
  provider: string;
  source_conversation_id: string;
  conversation_title: string | null;
  message_id: string | null;
  tool_call_id: string | null;
  snippet: string;
}

interface AuditReport {
  generated_at: string;
  scanned_messages: number;
  scanned_tool_calls: number;
  findings: AuditFinding[];
  high: number;
  medium: number;
  low: number;
}

interface PolicyRule {
  id: string;
  name: string;
  pattern: string;
  kind: string;
  severity: string;
  enabled: boolean;
}

interface BudgetSettings {
  monthly_token_limit: number | null;
  monthly_cost_limit: number | null;
  notify_on_exceed: boolean;
}

const PROVIDER_META: Record<string, { label: string; color: string }> = {
  zcode: { label: "ZCode", color: "#4da3ff" },
  "claude-code": { label: "Claude Code", color: "#ef8b56" },
  cursor: { label: "Cursor", color: "#a78bfa" },
  "minimax-code": { label: "MiniMax", color: "#f478b4" },
  codex: { label: "Codex", color: "#3ddba0" },
};

const meta = (p: string) => PROVIDER_META[p] ?? { label: p, color: "#8b96ad" };

const SEV_LABEL: Record<string, string> = { high: "高危", medium: "中危", low: "低危" };

interface Props {
  /** 审计命中 → 跳回对话视图定位（App 提供） */
  onJumpToConversation?: (provider: string, sourceConversationId: string, messageId: string | null) => void;
}

export default function OpsView({ onJumpToConversation }: Props) {
  const [range, setRange] = useState<number | null>(30);
  const [overview, setOverview] = useState<OpsOverview | null>(null);
  const [byProvider, setByProvider] = useState<ProviderUsage[]>([]);
  const [byModel, setByModel] = useState<ModelUsage[]>([]);
  const [timeseries, setTimeseries] = useState<DailyUsage[]>([]);
  const [topTools, setTopTools] = useState<ToolUsageRow[]>([]);
  const [risky, setRisky] = useState<RiskyCall[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [loading, setLoading] = useState(true);

  // M4 审计
  const [audit, setAudit] = useState<AuditReport | null>(null);
  const [auditing, setAuditing] = useState(false);
  const [auditKindFilter, setAuditKindFilter] = useState<"all" | "sensitive" | "dangerous_command">("all");
  const [policies, setPolicies] = useState<PolicyRule[]>([]);
  const [newPolicy, setNewPolicy] = useState({ name: "", pattern: "", kind: "dangerous_command", severity: "high" });

  // M5 预算
  const [budget, setBudget] = useState<BudgetSettings>({ monthly_token_limit: null, monthly_cost_limit: null, notify_on_exceed: true });
  const [monthUsage, setMonthUsage] = useState<{ tokens: number; cost_usd: number } | null>(null);
  const [budgetInput, setBudgetInput] = useState({ tokens: "", cost: "" });
  const [recalcMsg, setRecalcMsg] = useState<string | null>(null);

  const loadAll = async () => {
    setLoading(true);
    // allSettled：单个接口失败只影响对应卡片，不再拖空整页数据
    const [ov, bp, bm, ts, tt, rc] = await Promise.allSettled([
      invoke<OpsOverview>("ops_overview", { days: range }),
      invoke<ProviderUsage[]>("ops_by_provider", { days: range }),
      invoke<ModelUsage[]>("ops_by_model", { days: range }),
      invoke<DailyUsage[]>("ops_timeseries", { days: range }),
      invoke<ToolUsageRow[]>("ops_tool_toplist", { days: range, n: 10 }),
      invoke<RiskyCall[]>("ops_risky_calls", { days: range, n: 50 }),
    ]);
    if (ov.status === "fulfilled") setOverview(ov.value);
    else console.error("ops_overview failed", ov.reason);
    if (bp.status === "fulfilled") setByProvider(bp.value);
    if (bm.status === "fulfilled") setByModel(bm.value);
    if (ts.status === "fulfilled") setTimeseries(ts.value);
    if (tt.status === "fulfilled") setTopTools(tt.value);
    if (rc.status === "fulfilled") setRisky(rc.value);
    setLoading(false);
  };

  const loadBudget = async () => {
    try {
      const [b, mu] = await Promise.all([
        invoke<BudgetSettings>("budget_get"),
        invoke<{ tokens: number; cost_usd: number }>("ops_month_usage"),
      ]);
      setBudget(b);
      setMonthUsage(mu);
      setBudgetInput({
        tokens: b.monthly_token_limit?.toString() ?? "",
        cost: b.monthly_cost_limit?.toString() ?? "",
      });
    } catch (e) {
      console.error("budget load failed", e);
    }
  };

  const loadPolicies = async () => {
    try {
      setPolicies(await invoke<PolicyRule[]>("policy_list"));
    } catch (e) {
      console.error("policy load failed", e);
    }
  };

  // 首次进入：立即加载已有数据（不阻塞），后台节流同步指标，完成后刷新
  useEffect(() => {
    (async () => {
      // 先展示库里已有的指标
      await Promise.all([loadAll(), loadBudget(), loadPolicies()]);
      // 后台同步（5 分钟节流；若正忙静默跳过），完成后刷新数据
      setSyncing(true);
      try {
        await invoke("ops_sync", { force: false });
        await Promise.all([loadAll(), loadBudget()]);
      } catch {
        /* 正在同步中时静默跳过 */
      }
      setSyncing(false);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [range]);

  const runAuditScan = async () => {
    setAuditing(true);
    try {
      const report = await invoke<AuditReport>("audit_scan");
      setAudit(report);
    } catch (e) {
      console.error("audit failed", e);
    }
    setAuditing(false);
  };

  const exportHtml = async () => {
    try {
      const html = await invoke<string>("audit_export_html");
      const path = await save({
        defaultPath: `audit-report-${new Date().toISOString().slice(0, 10)}.html`,
        filters: [{ name: "HTML", extensions: ["html"] }],
      });
      if (typeof path === "string") {
        await invoke("save_text_file", { path, content: html });
      }
    } catch (e) {
      console.error("export failed", e);
    }
  };

  const addPolicy = async () => {
    if (!newPolicy.name.trim() || !newPolicy.pattern.trim()) return;
    try {
      await invoke("policy_upsert", {
        rule: {
          id: `pol_${Date.now()}`,
          name: newPolicy.name.trim(),
          pattern: newPolicy.pattern.trim(),
          kind: newPolicy.kind,
          severity: newPolicy.severity,
          enabled: true,
        },
      });
      setNewPolicy({ name: "", pattern: "", kind: "dangerous_command", severity: "high" });
      await loadPolicies();
    } catch (e) {
      alert(`规则无效: ${e}`);
    }
  };

  const removePolicy = async (name: string) => {
    await invoke("policy_delete", { name });
    loadPolicies();
  };

  const saveBudget = async () => {
    const tokens = budgetInput.tokens.trim() ? parseInt(budgetInput.tokens, 10) : null;
    const cost = budgetInput.cost.trim() ? parseFloat(budgetInput.cost) : null;
    if (tokens != null && Number.isNaN(tokens)) return alert("Token 阈值必须是整数");
    if (cost != null && Number.isNaN(cost)) return alert("成本阈值必须是数字");
    await invoke("budget_set", {
      settings: { monthly_token_limit: tokens, monthly_cost_limit: cost, notify_on_exceed: budget.notify_on_exceed },
    });
    await loadBudget();
    setRecalcMsg("预算已保存");
    setTimeout(() => setRecalcMsg(null), 2000);
  };

  const recalcCost = async () => {
    try {
      const r = await invoke<{ models_updated: number; total_cost_usd: number }>("ops_cost_recalc");
      setRecalcMsg(`已按定价重算 ${r.models_updated} 个模型，总成本 ${formatCost(r.total_cost_usd)}`);
      setTimeout(() => setRecalcMsg(null), 4000);
      await loadAll();
      await loadBudget();
    } catch (e) {
      setRecalcMsg(`重算失败: ${e}`);
    }
  };

  // 预算告警
  const tokenPct =
    budget.monthly_token_limit && monthUsage
      ? Math.min((monthUsage.tokens / budget.monthly_token_limit) * 100, 999)
      : null;
  const costPct =
    budget.monthly_cost_limit && monthUsage
      ? Math.min((monthUsage.cost_usd / budget.monthly_cost_limit) * 100, 999)
      : null;
  const overBudget =
    (tokenPct != null && tokenPct >= 100) || (costPct != null && costPct >= 100);

  const kpis = overview
    ? [
        { label: "模型请求", value: overview.total_requests.toLocaleString(), sub: `${overview.session_count} 会话` },
        { label: "总 Tokens", value: formatTokens(overview.total_tokens), sub: `in ${formatTokens(overview.input_tokens)} / out ${formatTokens(overview.output_tokens)}` },
        { label: "估算成本", value: formatCost(overview.cost_usd), sub: "按 pricing.json 定价" },
        { label: "危险操作", value: String(overview.destructive_calls), sub: `${overview.total_tool_calls.toLocaleString()} 次工具调用`, danger: overview.destructive_calls > 0 },
      ]
    : [];

  const donutSlices = byProvider
    .filter((p) => p.total_tokens > 0)
    .map((p) => ({ label: meta(p.provider).label, value: p.total_tokens, color: meta(p.provider).color }));

  const barData = timeseries.map((d) => ({
    label: d.day,
    value: d.total_tokens,
    title: `${d.day}: ${formatTokens(d.total_tokens)} (${d.requests} 次)`,
  }));

  const maxToolCalls = Math.max(...topTools.map((t) => t.calls), 1);

  const auditFindings = (audit?.findings ?? []).filter(
    (f) => auditKindFilter === "all" || f.kind === auditKindFilter
  );

  return (
    <div className="ops-view">
      {/* 工具行 */}
      <div className="ops-toolbar">
        <div className="ops-range">
          {[[7, "7天"], [30, "30天"], [90, "90天"], [null, "全部"]].map(([v, label]) => (
            <button
              key={String(v)}
              className={`filter-chip ${range === v ? "active" : ""}`}
              onClick={() => setRange(v as number | null)}
            >
              {label as string}
            </button>
          ))}
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button className="action-btn" disabled={syncing} onClick={async () => { setSyncing(true); try { await invoke("ops_sync", { force: true }); } catch {} setSyncing(false); loadAll(); loadBudget(); }}>
            {syncing ? "同步指标中…" : "↻ 同步指标"}
          </button>
          <button className="action-btn" onClick={recalcCost} title="按 pricing.json 重算成本">
            $ 重算成本
          </button>
        </div>
      </div>

      {/* 预算告警横幅 */}
      {overBudget && (
        <div className="budget-alert">
          ⚠ 本月用量已超预算：{tokenPct != null && tokenPct >= 100 && ` tokens ${Math.round(tokenPct)}%`}
          {tokenPct != null && tokenPct >= 100 && costPct != null && costPct >= 100 && " ·"}
          {costPct != null && costPct >= 100 && ` 成本 ${Math.round(costPct)}%`}
        </div>
      )}
      {recalcMsg && <div className="recalc-msg">{recalcMsg}</div>}

      {loading && !overview ? (
        <div className="ops-loading">
          <div className="spinner" />
          <span>采集治理指标中…（首次较慢）</span>
        </div>
      ) : (
        <>
          {/* KPI 行 */}
          <div className="ops-kpis">
            {kpis.map((k, i) => (
              <div key={i} className={`ops-kpi ${k.danger ? "danger" : ""}`}>
                <div className="ops-kpi-value">{k.value}</div>
                <div className="ops-kpi-label">{k.label}</div>
                <div className="ops-kpi-sub">{k.sub}</div>
              </div>
            ))}
          </div>

          {/* 图表行 */}
          <div className="ops-charts">
            <div className="ops-card">
              <div className="ops-card-title">Agent 用量分布</div>
              <div className="ops-donut-wrap">
                <DonutChart slices={donutSlices} />
                <div className="ops-legend">
                  {byProvider.map((p) => (
                    <div key={p.provider} className="ops-legend-item">
                      <span className="legend-dot" style={{ background: meta(p.provider).color }} />
                      <span className="legend-label">{meta(p.provider).label}</span>
                      <span className="legend-value">{formatTokens(p.total_tokens)}</span>
                      <span className="legend-req">{p.requests.toLocaleString()} 次</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
            <div className="ops-card ops-card-wide">
              <div className="ops-card-title">每日 Tokens 趋势</div>
              <BarChart data={barData} />
            </div>
          </div>

          {/* 模型 + 工具榜单 */}
          <div className="ops-tables">
            <div className="ops-card">
              <div className="ops-card-title">模型明细</div>
              <table className="ops-table">
                <thead>
                  <tr><th>模型</th><th>请求</th><th>输入</th><th>输出</th><th>错误</th></tr>
                </thead>
                <tbody>
                  {byModel.map((m, i) => (
                    <tr key={i}>
                      <td className="mono">
                        {m.model === "(unknown)"
                          ? `${meta(m.provider_id.replace("prov_", "")).label} ·默认`
                          : m.model}
                      </td>
                      <td>{m.requests.toLocaleString()}</td>
                      <td>{formatTokens(m.input_tokens)}</td>
                      <td>{formatTokens(m.output_tokens)}</td>
                      <td className={m.errors > 0 ? "text-danger" : ""}>{m.errors}</td>
                    </tr>
                  ))}
                  {byModel.length === 0 && <tr><td colSpan={5} className="ops-table-empty">暂无数据</td></tr>}
                </tbody>
              </table>
            </div>
            <div className="ops-card">
              <div className="ops-card-title">工具调用 Top 10</div>
              <div className="ops-tools">
                {topTools.map((t, i) => (
                  <div key={i} className="ops-tool-row">
                    <span className={`ops-tool-name mono ${t.destructive > 0 ? "text-danger" : ""}`}>{t.tool_name}</span>
                    <div className="ops-tool-bar-bg">
                      <div className="ops-tool-bar" style={{ width: `${(t.calls / maxToolCalls) * 100}%` }} />
                    </div>
                    <span className="ops-tool-calls">{t.calls.toLocaleString()}</span>
                    {t.destructive > 0 && <span className="badge completeness 有限">{t.destructive} 危险</span>}
                  </div>
                ))}
                {topTools.length === 0 && <div className="ops-table-empty">暂无数据</div>}
              </div>
            </div>
          </div>

          {/* ── M5：预算卡片 ── */}
          <div className="ops-card ops-budget">
            <div className="ops-card-title">
              月度预算
              <span className="ops-card-sub">本月（自然月）用量 vs 阈值 · 定价表在 app-data/pricing.json 可编辑</span>
            </div>
            <div className="budget-grid">
              <div className="budget-item">
                <div className="budget-label">本月 Tokens</div>
                <div className="budget-value">{monthUsage ? formatTokens(monthUsage.tokens) : "—"}</div>
                {tokenPct != null && (
                  <div className="budget-progress">
                    <div className={`budget-progress-bar ${tokenPct >= 100 ? "over" : ""}`} style={{ width: `${Math.min(tokenPct, 100)}%` }} />
                  </div>
                )}
                {tokenPct != null && <div className="budget-pct">{Math.round(tokenPct)}% / {formatTokens(budget.monthly_token_limit!)}</div>}
              </div>
              <div className="budget-item">
                <div className="budget-label">本月成本</div>
                <div className="budget-value">{monthUsage ? formatCost(monthUsage.cost_usd) : "—"}</div>
                {costPct != null && (
                  <div className="budget-progress">
                    <div className={`budget-progress-bar ${costPct >= 100 ? "over" : ""}`} style={{ width: `${Math.min(costPct, 100)}%` }} />
                  </div>
                )}
                {costPct != null && <div className="budget-pct">{Math.round(costPct)}% / {formatCost(budget.monthly_cost_limit!)}</div>}
              </div>
              <div className="budget-item budget-edit">
                <div className="budget-label">阈值设置</div>
                <div className="budget-inputs">
                  <input
                    type="text" placeholder="Token 上限（如 100000000）" value={budgetInput.tokens}
                    onChange={(e) => setBudgetInput((p) => ({ ...p, tokens: e.target.value }))}
                  />
                  <input
                    type="text" placeholder="成本上限 $（如 50）" value={budgetInput.cost}
                    onChange={(e) => setBudgetInput((p) => ({ ...p, cost: e.target.value }))}
                  />
                  <button className="action-btn" onClick={saveBudget}>保存预算</button>
                </div>
              </div>
            </div>
          </div>

          {/* ── M4：安全审计卡片 ── */}
          <div className="ops-card">
            <div className="ops-card-title">
              🛡 安全审计
              <span className="ops-card-sub">
                敏感信息（7 内置 + 自定义规则）+ 危险命令（12 内置 + 自定义）
              </span>
              {audit && (
                <span className="audit-stats">
                  扫描 {audit.scanned_messages.toLocaleString()} 消息 / {audit.scanned_tool_calls.toLocaleString()} 命令 ·
                  <b className="text-danger"> 高危 {audit.high}</b> ·
                  <b> 中危 {audit.medium}</b> · 低危 {audit.low}
                </span>
              )}
            </div>
            <div className="audit-toolbar">
              <button className="action-btn" disabled={auditing} onClick={runAuditScan}>
                {auditing ? "扫描中…" : audit ? "↻ 重新扫描" : "▶ 开始全库扫描"}
              </button>
              {audit && audit.findings.length > 0 && (
                <>
                  <button className="action-btn" onClick={exportHtml}>⤓ 导出 HTML 报告</button>
                  <div className="ops-range">
                    {([["all", "全部"], ["sensitive", "敏感信息"], ["dangerous_command", "危险命令"]] as const).map(([v, l]) => (
                      <button key={v} className={`filter-chip ${auditKindFilter === v ? "active" : ""}`} onClick={() => setAuditKindFilter(v)}>{l}</button>
                    ))}
                  </div>
                </>
              )}
            </div>

            {audit && auditFindings.length > 0 && (
              <div className="audit-findings">
                {auditFindings.slice(0, 50).map((f, i) => (
                  <div
                    key={i}
                    className="audit-finding-row"
                    onClick={() => onJumpToConversation?.(f.provider, f.source_conversation_id, f.message_id)}
                    title="点击跳转到对应会话"
                  >
                    <span className={`risk-flag ${f.severity}`}>{SEV_LABEL[f.severity]}</span>
                    <span className={`badge source ${f.provider}`}>{meta(f.provider).label}</span>
                    <span className="audit-finding-rule mono">{f.rule}</span>
                    <span className="audit-finding-snippet mono">{f.snippet}</span>
                    <span className="ops-risky-time">
                      {f.conversation_title?.slice(0, 20) ?? f.source_conversation_id.slice(0, 14)}
                    </span>
                  </div>
                ))}
                {auditFindings.length > 50 && (
                  <div className="ops-table-empty">…共 {auditFindings.length} 条，导出 HTML 查看全部</div>
                )}
              </div>
            )}
            {audit && audit.findings.length === 0 && (
              <div className="ops-table-empty">扫描完成，未发现风险 🎉</div>
            )}

            {/* 自定义策略规则 */}
            <div className="policy-section">
              <div className="budget-label">自定义策略规则（正则）</div>
              <div className="policy-add">
                <input placeholder="规则名（如 no_kubectl_delete）" value={newPolicy.name}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, name: e.target.value }))} />
                <input placeholder="正则（如 kubectl\s+delete）" value={newPolicy.pattern}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, pattern: e.target.value }))} />
                <select value={newPolicy.kind} onChange={(e) => setNewPolicy((p) => ({ ...p, kind: e.target.value }))}>
                  <option value="dangerous_command">危险命令</option>
                  <option value="sensitive">敏感信息</option>
                </select>
                <select value={newPolicy.severity} onChange={(e) => setNewPolicy((p) => ({ ...p, severity: e.target.value }))}>
                  <option value="high">高危</option>
                  <option value="medium">中危</option>
                  <option value="low">低危</option>
                </select>
                <button className="action-btn" onClick={addPolicy}>＋ 添加</button>
              </div>
              {policies.length > 0 && (
                <div className="policy-list">
                  {policies.map((p) => (
                    <div key={p.id} className="policy-row">
                      <span className={`risk-flag ${p.severity}`}>{SEV_LABEL[p.severity] ?? p.severity}</span>
                      <span className="mono">{p.name}</span>
                      <span className="policy-kind">{p.kind === "sensitive" ? "敏感" : "命令"}</span>
                      <span className="mono policy-pattern">{p.pattern}</span>
                      <button className="policy-del" onClick={() => removePolicy(p.name)}>✕</button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* 风险调用 */}
          <div className="ops-card">
            <div className="ops-card-title">
              风险调用 ({risky.length})
              <span className="ops-card-sub">破坏性 / 出错 / 非零退出码</span>
            </div>
            <div className="ops-risky">
              {risky.slice(0, 20).map((r) => (
                <div key={r.id} className="ops-risky-row">
                  <span className={`badge source ${r.provider}`}>{meta(r.provider).label}</span>
                  <span className="mono ops-risky-tool">{r.tool_name}</span>
                  {r.destructive && <span className="risk-flag high">危险</span>}
                  {r.exit_code != null && r.exit_code !== 0 && <span className="risk-flag medium">exit {r.exit_code}</span>}
                  {r.approval_status && r.approval_status !== "none" && <span className="risk-flag approval">{r.approval_status}</span>}
                  <span className="ops-risky-cmd mono" title={r.command_text ?? ""}>
                    {r.command_text ?? r.source_session_id.slice(0, 18)}
                  </span>
                  <span className="ops-risky-time">{new Date(r.ts).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}</span>
                </div>
              ))}
              {risky.length === 0 && <div className="ops-table-empty">无风险调用 🎉</div>}
            </div>
          </div>

          {overview && overview.avg_duration_ms > 0 && (
            <div className="ops-footnote">
              平均请求耗时 {formatDuration(overview.avg_duration_ms)} · 错误 {overview.error_count} 次 ·
              数据口径：input + output + reasoning（cache 不计费）
            </div>
          )}
        </>
      )}
    </div>
  );
}
