// CodeAgentOps 治理视图（plan codeagent-ops M3/M4/M5）
// KPI 卡 + Agent 分布 donut + 每日趋势 bar + 模型/工具榜单 + 风险调用
// + 安全审计（M4）+ 预算与成本（M5）

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { BarChart, DonutChart, formatCost, formatDuration, formatTokens, useCountUp } from "./charts";

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
  ts_ms: number;
  read_only: boolean | null;
  destructive: boolean | null;
  approval_status: string | null;
  exit_code: number | null;
  duration_ms: number | null;
  status: string;
  command_text: string | null;
}

interface AssetRow {
  provider: string;
  kind: string;
  name: string;
  version: string | null;
  description: string | null;
  risky_hits: number;
  installed_at: string | null;
  path: string | null;
}

interface AutomationRow {
  provider: string;
  name: string;
  kind: string;
  schedule: string | null;
  status: string | null;
  detail: string | null;
}

interface DirCost {
  dir: string;
  tokens: number;
  cost_usd: number;
  requests: number;
}

interface CacheStat {
  provider: string;
  input_tokens: number;
  cache_read_tokens: number;
  hit_rate: number;
}

interface AnomalyRow {
  kind: string;
  agent: string;
  detail: string;
  severity: string;
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

/** KPI 数字滚动动画：0 → 目标（easeOut 800ms） */
function AnimatedKpi({
  label, num, fmt, sub, danger,
}: {
  label: string;
  num: number;
  fmt: (v: number) => string;
  sub: string;
  danger?: boolean;
}) {
  const v = useCountUp(num);
  return (
    <div className={`ops-kpi ${danger ? "danger" : ""}`}>
      <div className="ops-kpi-value">{fmt(v)}</div>
      <div className="ops-kpi-label">{label}</div>
      <div className="ops-kpi-sub">{sub}</div>
    </div>
  );
}

export default function OpsView({ onJumpToConversation }: Props) {
  const [range, setRange] = useState<number | null>(30);
  const [overview, setOverview] = useState<OpsOverview | null>(null);
  const [byProvider, setByProvider] = useState<ProviderUsage[]>([]);
  const [byModel, setByModel] = useState<ModelUsage[]>([]);
  const [timeseries, setTimeseries] = useState<DailyUsage[]>([]);
  const [topTools, setTopTools] = useState<ToolUsageRow[]>([]);
  const [risky, setRisky] = useState<RiskyCall[]>([]);
  const [expandedRisk, setExpandedRisk] = useState<Set<string>>(new Set());
  // M6-M9
  const [assets, setAssets] = useState<AssetRow[]>([]);
  const [automations, setAutomations] = useState<AutomationRow[]>([]);
  const [dirCosts, setDirCosts] = useState<DirCost[]>([]);
  const [cacheStats, setCacheStats] = useState<CacheStat[]>([]);
  const [anomalies, setAnomalies] = useState<AnomalyRow[]>([]);
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
    const [ov, bp, bm, ts, tt, rc, dc, cs, an] = await Promise.allSettled([
      invoke<OpsOverview>("ops_overview", { days: range }),
      invoke<ProviderUsage[]>("ops_by_provider", { days: range }),
      invoke<ModelUsage[]>("ops_by_model", { days: range }),
      invoke<DailyUsage[]>("ops_timeseries", { days: range }),
      invoke<ToolUsageRow[]>("ops_tool_toplist", { days: range, n: 10 }),
      invoke<RiskyCall[]>("ops_risky_calls", { days: range, n: 50 }),
      invoke<DirCost[]>("ops_cost_by_dir", { days: range, n: 10 }),
      invoke<CacheStat[]>("ops_cache_stats", { days: range }),
      invoke<AnomalyRow[]>("ops_anomalies", { days: range }),
    ]);
    if (ov.status === "fulfilled") setOverview(ov.value);
    else console.error("ops_overview failed", ov.reason);
    if (bp.status === "fulfilled") setByProvider(bp.value);
    if (bm.status === "fulfilled") setByModel(bm.value);
    if (ts.status === "fulfilled") setTimeseries(ts.value);
    if (tt.status === "fulfilled") setTopTools(tt.value);
    if (rc.status === "fulfilled") setRisky(rc.value);
    if (dc.status === "fulfilled") setDirCosts(dc.value);
    if (cs.status === "fulfilled") setCacheStats(cs.value);
    if (an.status === "fulfilled") setAnomalies(an.value);
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
      // 先展示库里已有的指标 + 资产/自动化
      invoke<AssetRow[]>("assets_list").then(setAssets).catch(() => {});
      invoke<AutomationRow[]>("automations_list").then(setAutomations).catch(() => {});
      await Promise.all([loadAll(), loadBudget(), loadPolicies()]);
      // 后台同步（均 30 分钟节流；若正忙静默跳过），完成后刷新数据
      setSyncing(true);
      try {
        await invoke("ops_sync", { force: false });
        await Promise.all([
          invoke("assets_sync", { force: false }).catch(() => {}),
          invoke("automations_sync", { force: false }).catch(() => {}),
        ]);
        invoke<AssetRow[]>("assets_list").then(setAssets).catch(() => {});
        invoke<AutomationRow[]>("automations_list").then(setAutomations).catch(() => {});
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
        { label: "模型请求", num: overview.total_requests, fmt: (v: number) => Math.round(v).toLocaleString(), sub: `${overview.session_count} 会话` },
        { label: "总 Tokens", num: overview.total_tokens, fmt: (v: number) => formatTokens(v), sub: `in ${formatTokens(overview.input_tokens)} / out ${formatTokens(overview.output_tokens)}` },
        { label: "估算成本", num: overview.cost_usd, fmt: (v: number) => formatCost(v), sub: "按 pricing.json 定价" },
        { label: "危险操作", num: overview.destructive_calls, fmt: (v: number) => String(Math.round(v)), sub: `${overview.total_tool_calls.toLocaleString()} 次工具调用`, danger: overview.destructive_calls > 0 },
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

      {/* 页面即刻完整渲染：每块数据独立 skeleton，不整页等待 */}
      {overview ? (
        <div className="ops-kpis">
          {kpis.map((k, i) => (
            <AnimatedKpi key={i} label={k.label} num={k.num} fmt={k.fmt} sub={k.sub} danger={k.danger} />
          ))}
        </div>
      ) : (
        <div className="ops-kpis">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="ops-kpi skeleton">
              <div className="sk-line sk-lg" />
              <div className="sk-line" />
              <div className="sk-line sk-sm" />
            </div>
          ))}
        </div>
      )}

      {/* 图表行（数据未到时 skeleton） */}
          <div className="ops-charts">
            <div className="ops-card">
              <div className="ops-card-title">Agent 用量分布</div>
              {byProvider.length === 0 ? (
                <div className="chart-skeleton donut">
                  <div className="sk-circle" />
                  <div className="sk-lines">
                    {[0, 1, 2, 3].map((i) => <div key={i} className="sk-line" />)}
                  </div>
                </div>
              ) : (
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
              )}
            </div>
            <div className="ops-card ops-card-wide">
              <div className="ops-card-title">每日 Tokens 趋势</div>
              {timeseries.length === 0 ? (
                <div className="chart-skeleton bars">
                  <div className="sk-bars">
                    {Array.from({ length: 14 }).map((_, i) => (
                      <div key={i} className="sk-bar" style={{ height: `${20 + ((i * 37) % 70)}%` }} />
                    ))}
                  </div>
                </div>
              ) : (
                <BarChart data={barData} />
              )}
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
                  {byModel.length === 0 && loading && [0,1,2,3].map((i) => (
                    <tr key={i}><td colSpan={5} style={{ padding: 0 }}><div className="sk-line" style={{ margin: "10px 14px" }} /></td></tr>
                  ))}
                  {byModel.length === 0 && !loading && <tr><td colSpan={5} className="ops-table-empty">暂无数据</td></tr>}
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
                {topTools.length === 0 && loading && [0,1,2,3].map((i) => (
                  <div key={i} className="sk-line" style={{ margin: "10px 0" }} />
                ))}
                {topTools.length === 0 && !loading && <div className="ops-table-empty">暂无数据</div>}
              </div>
            </div>
          </div>

          {/* ── M7：成本洞察 ── */}
          <div className="ops-tables">
            <div className="ops-card">
              <div className="ops-card-title">
                缓存命中率
                <span className="ops-card-sub">cache_read / (input + cache_read)</span>
              </div>
              {cacheStats.length === 0 ? (
                loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">暂无数据</div>
              ) : cacheStats.map((c) => (
                <div key={c.provider} className="ops-tool-row">
                  <span className="ops-tool-name">{meta(c.provider).label}</span>
                  <div className="ops-tool-bar-bg">
                    <div
                      className="ops-tool-bar"
                      style={{ width: `${(c.hit_rate * 100).toFixed(1)}%`, background: meta(c.provider).color }}
                    />
                  </div>
                  <span className="ops-tool-calls">{(c.hit_rate * 100).toFixed(1)}%</span>
                  <span className="legend-req">{formatTokens(c.cache_read_tokens)} 缓存</span>
                </div>
              ))}
            </div>
            <div className="ops-card">
              <div className="ops-card-title">
                按项目成本 Top10
                <span className="ops-card-sub">来源侧工作目录归因</span>
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
          </div>

          {/* ── M6：资产清单 ── */}
          <div className="ops-card">
            <div className="ops-card-title">
              🧩 资产清单（{assets.length}）
              <span className="ops-card-sub">skills / plugins / 内置技能 · 红框含危险模式</span>
            </div>
            {assets.length === 0 ? (
              loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">暂无数据（首次进入后台同步中）</div>
            ) : (
              <div className="assets-grid">
                {assets.map((a, i) => (
                  <div key={i} className={`asset-item ${a.risky_hits > 0 ? "risky" : ""}`}>
                    <span className={`badge source ${a.provider}`}>{meta(a.provider).label}</span>
                    <span className="asset-kind">{a.kind === "builtin_skill" ? "内置" : a.kind}</span>
                    <span className="asset-name mono" title={a.path ?? ""}>{a.name}</span>
                    {a.version && <span className="asset-ver mono">v{a.version}</span>}
                    {a.risky_hits > 0 && <span className="risk-flag high">⚠ {a.risky_hits}</span>}
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* ── M8：自动化任务 ── */}
          <div className="ops-card">
            <div className="ops-card-title">
              ⏱ 自动化任务（{automations.length}）
              <span className="ops-card-sub">cron / workflow / 后台任务</span>
            </div>
            {automations.length === 0 ? (
              loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">暂无数据</div>
            ) : (
              <div className="ops-risky">
                {automations.map((a, i) => (
                  <div key={i} className="ops-risky-row">
                    <span className={`badge source ${a.provider}`}>{meta(a.provider).label}</span>
                    <span className="asset-name mono">{a.name}</span>
                    <span className="policy-kind">{a.kind}</span>
                    {a.schedule && <span className="mono" style={{ fontSize: 10.5, color: "var(--text-muted)" }}>{a.schedule}</span>}
                    {a.status && <span className={`risk-flag ${a.status.includes("completed") || a.status.includes("trusted") || a.status.includes("configured") ? "low" : "medium"}`}>{a.status}</span>}
                    <span className="ops-risky-cmd mono">{a.detail ?? ""}</span>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* ── M9：异常检测 ── */}
          <div className="ops-card">
            <div className="ops-card-title">
              🚨 异常检测（{anomalies.length}）
              <span className="ops-card-sub">错误尖峰 / 重试风暴 / context 超限</span>
            </div>
            {anomalies.length === 0 ? (
              loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">未检测到异常 🎉</div>
            ) : (
              <div className="ops-risky">
                {anomalies.map((a, i) => (
                  <div key={i} className="ops-risky-row">
                    <span className={`risk-flag ${a.severity}`}>
                      {a.kind === "error_spike" ? "错误尖峰" : a.kind === "retry_storm" ? "重试风暴" : "context超限"}
                    </span>
                    <span className="mono" style={{ fontSize: 11.5 }}>{a.detail}</span>
                  </div>
                ))}
              </div>
            )}
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
              {risky.slice(0, 20).map((r) => {
                const open = expandedRisk.has(r.id);
                return (
                  <div key={r.id} className={`ops-risky-item ${open ? "open" : ""}`}>
                    <div
                      className="ops-risky-row"
                      onClick={() =>
                        setExpandedRisk((prev) => {
                          const next = new Set(prev);
                          if (next.has(r.id)) next.delete(r.id);
                          else next.add(r.id);
                          return next;
                        })
                      }
                    >
                      <span className="risk-caret">{open ? "▾" : "▸"}</span>
                      <span className={`badge source ${r.provider}`}>{meta(r.provider).label}</span>
                      <span className="mono ops-risky-tool">{r.tool_name}</span>
                      {r.destructive && <span className="risk-flag high">危险</span>}
                      {r.exit_code != null && r.exit_code !== 0 && <span className="risk-flag medium">exit {r.exit_code}</span>}
                      {r.approval_status && r.approval_status !== "none" && <span className="risk-flag approval">{r.approval_status}</span>}
                      <span className="ops-risky-cmd mono" title={r.command_text ?? ""}>
                        {r.command_text ?? r.source_session_id.slice(0, 18)}
                      </span>
                      <span className="ops-risky-time">
                        {new Date(r.ts_ms).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
                      </span>
                    </div>
                    {open && (
                      <div className="ops-risky-detail">
                        <div className="risk-detail-grid">
                          <div><b>时间：</b>{new Date(r.ts_ms).toLocaleString("zh-CN")}</div>
                          <div><b>状态：</b>{r.status}</div>
                          <div><b>退出码：</b>{r.exit_code ?? "—"}</div>
                          <div><b>耗时：</b>{r.duration_ms != null ? formatDuration(r.duration_ms) : "—"}</div>
                          <div><b>只读：</b>{r.read_only == null ? "—" : r.read_only ? "是" : "否"}</div>
                          <div><b>审批：</b>{r.approval_status ?? "—"}</div>
                        </div>
                        {r.command_text && <pre className="risk-detail-cmd mono">{r.command_text}</pre>}
                        <button
                          className="action-btn"
                          onClick={() => onJumpToConversation?.(r.provider, r.source_session_id, null)}
                        >
                          → 跳转到对应会话
                        </button>
                      </div>
                    )}
                  </div>
                );
              })}
              {risky.length === 0 && <div className="ops-table-empty">无风险调用 🎉</div>}
            </div>
          </div>

      {overview && overview.avg_duration_ms > 0 && (
        <div className="ops-footnote">
          平均请求耗时 {formatDuration(overview.avg_duration_ms)} · 错误 {overview.error_count} 次 ·
          数据口径：input + output + reasoning（cache 不计费）
        </div>
      )}
    </div>
  );
}
