// CodeAgentOps 治理视图容器：状态管理 + 分区加载 + 委托渲染
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import OverviewSection from "./OverviewSection";
import CostSection from "./CostSection";
import SecuritySection from "./SecuritySection";
import AssetsSection from "./AssetsSection";
import type { Section, OpsOverview, ProviderUsage, ModelUsage, DailyUsage, ToolUsageRow, RiskyCall, AssetRow, AutomationRow, DirCost, CacheStat, AnomalyRow, AgentHealth, LatencyStat, TokenWaste, AgentBenchmark, AuditReport, PolicyRule, BudgetSettings } from "./ops-types";

type Props = {
  section: Section;
  onJumpToConversation?: (provider: string, sessionId: string, messageId: string | null) => void;
};

export default function OpsView({ section, onJumpToConversation }: Props) {
  const [range, setRange] = useState<number | null>(30);
  const [loading, setLoading] = useState(true);
  const [syncing, setSyncing] = useState(false);
  const [recalcMsg, setRecalcMsg] = useState<string | null>(null);

  // data
  const [overview, setOverview] = useState<OpsOverview | null>(null);
  const [byProvider, setByProvider] = useState<ProviderUsage[]>([]);
  const [byModel, setByModel] = useState<ModelUsage[]>([]);
  const [timeseries, setTimeseries] = useState<DailyUsage[]>([]);
  const [topTools, setTopTools] = useState<ToolUsageRow[]>([]);
  const [cacheStats, setCacheStats] = useState<CacheStat[]>([]);
  const [health, setHealth] = useState<AgentHealth[]>([]);
  const [latency, setLatency] = useState<LatencyStat[]>([]);
  const [waste, setWaste] = useState<TokenWaste[]>([]);
  const [benchmark, setBenchmark] = useState<AgentBenchmark[]>([]);
  const [risky, setRisky] = useState<RiskyCall[]>([]);
  const [anomalies, setAnomalies] = useState<AnomalyRow[]>([]);
  const [assets, setAssets] = useState<AssetRow[]>([]);
  const [automations, setAutomations] = useState<AutomationRow[]>([]);
  const [dirCosts, setDirCosts] = useState<DirCost[]>([]);
  const [audit, setAudit] = useState<AuditReport | null>(null);
  const [policies, setPolicies] = useState<PolicyRule[]>([]);
  const [budget, setBudget] = useState<BudgetSettings>({ monthly_token_limit: null, monthly_cost_limit: null, notify_on_exceed: true });
  const [monthUsage, setMonthUsage] = useState<{ tokens: number; cost_usd: number } | null>(null);
  const [budgetInput, setBudgetInput] = useState({ tokens: "", cost: "" });

  // ui
  const [auditing, setAuditing] = useState(false);
  const [auditKindFilter, setAuditKindFilter] = useState<"all" | "sensitive" | "dangerous_command">("all");
  const [expandedRisk, setExpandedRisk] = useState<Set<string>>(new Set());
  const [newPolicy, setNewPolicy] = useState({ name: "", pattern: "", kind: "dangerous_command", severity: "high" });

  const loadSection = async (sec: Section) => {
    setLoading(true);
    const reqs: [Promise<unknown>, string][] = [];
    const p = (pr: Promise<unknown>, tag: string) => reqs.push([pr, tag]);
    if (sec === "overview") {
      p(invoke<OpsOverview>("ops_overview", { days: range }).then(setOverview), "overview");
      p(invoke<ProviderUsage[]>("ops_by_provider", { days: range }).then(setByProvider), "prov");
      p(invoke<ModelUsage[]>("ops_by_model", { days: range }).then(setByModel), "model");
      p(invoke<DailyUsage[]>("ops_timeseries", { days: range }).then(setTimeseries), "ts");
      p(invoke<ToolUsageRow[]>("ops_tool_toplist", { days: range, n: 10 }).then(setTopTools), "tools");
      p(invoke<CacheStat[]>("ops_cache_stats", { days: range }).then(setCacheStats), "cache");
      p(invoke<AgentHealth[]>("ops_agent_health", { days: range }).then(setHealth), "health");
      p(invoke<LatencyStat[]>("ops_latency_stats", { days: range }).then(setLatency), "latency");
      p(invoke<TokenWaste[]>("ops_token_waste", { days: range, n: 10 }).then(setWaste), "waste");
      p(invoke<AgentBenchmark[]>("ops_agent_benchmark", { days: range }).then(setBenchmark), "bench");
    } else if (sec === "cost") {
      p(invoke<OpsOverview>("ops_overview", { days: range }).then(setOverview), "overview");
      p(invoke<DirCost[]>("ops_cost_by_dir", { days: range, n: 10 }).then(setDirCosts), "dirCost");
    } else if (sec === "security") {
      p(invoke<RiskyCall[]>("ops_risky_calls", { days: range, n: 50 }).then(setRisky), "risky");
      p(invoke<AnomalyRow[]>("ops_anomalies", { days: range }).then(setAnomalies), "anomaly");
    } else {
      p(invoke<AssetRow[]>("assets_list").then(setAssets), "assets");
      p(invoke<AutomationRow[]>("automations_list").then(setAutomations), "auto");
    }
    await Promise.allSettled(reqs.map(([pr]) => pr));
    setLoading(false);
  };

  const loadBudget = async () => {
    try {
      const [b, mu] = await Promise.all([
        invoke<BudgetSettings>("budget_get"),
        invoke<{ tokens: number; cost_usd: number }>("ops_month_usage"),
      ]);
      setBudget(b); setMonthUsage(mu);
      setBudgetInput({ tokens: b.monthly_token_limit?.toString() ?? "", cost: b.monthly_cost_limit?.toString() ?? "" });
    } catch { }
  };

  const loadPolicies = async () => {
    try { setPolicies(await invoke<PolicyRule[]>("policy_list")); } catch { }
  };

  useEffect(() => {
    const tasks: Promise<void>[] = [loadSection(section)];
    if (section === "cost") tasks.push(loadBudget());
    if (section === "security") tasks.push(loadPolicies());
    (async () => {
      setSyncing(true);
      try {
        await invoke("ops_sync", { force: false });
        await Promise.all([invoke("assets_sync", { force: false }).catch(() => {}), invoke("automations_sync", { force: false }).catch(() => {})]);
      } catch { }
      setSyncing(false);
      loadSection(section);
      if (section === "cost") loadBudget();
    })();
  }, [section]);

  useEffect(() => {
    if (section === "assets") return;
    loadSection(section);
    if (section === "cost") loadBudget();
  }, [range]);

  // ── actions ──
  const runAudit = async () => { setAuditing(true); try { setAudit(await invoke<AuditReport>("audit_scan")); } catch { } setAuditing(false); };
  const exportHtml = async () => {
    try {
      const html = await invoke<string>("audit_export_html");
      const path = await save({ defaultPath: `audit-${new Date().toISOString().slice(0,10)}.html`, filters: [{ name: "HTML", extensions: ["html"] }] });
      if (typeof path === "string") await invoke("save_text_file", { path, content: html });
    } catch { }
  };
  const addPolicy = async () => {
    if (!newPolicy.name.trim() || !newPolicy.pattern.trim()) return;
    try {
      await invoke("policy_upsert", { rule: { id: `pol_${Date.now()}`, name: newPolicy.name.trim(), pattern: newPolicy.pattern.trim(), kind: newPolicy.kind, severity: newPolicy.severity, enabled: true } });
      setNewPolicy({ name: "", pattern: "", kind: "dangerous_command", severity: "high" });
      loadPolicies();
    } catch (e) { alert(`规则无效: ${e}`); }
  };
  const saveBudget = async () => {
    const tokens = budgetInput.tokens.trim() ? parseInt(budgetInput.tokens, 10) : null;
    const cost = budgetInput.cost.trim() ? parseFloat(budgetInput.cost) : null;
    await invoke("budget_set", { settings: { monthly_token_limit: tokens, monthly_cost_limit: cost, notify_on_exceed: budget.notify_on_exceed } });
    await loadBudget();
    setRecalcMsg("预算已保存"); setTimeout(() => setRecalcMsg(null), 2000);
  };
  const recalcCost = async () => {
    try {
      const r = await invoke<{ models_updated: number; total_cost_usd: number }>("ops_cost_recalc");
      setRecalcMsg(`已重算 ${r.models_updated} 个模型，总成本 $${r.total_cost_usd.toFixed(2)}`);
      setTimeout(() => setRecalcMsg(null), 4000);
      loadSection(section); if (section === "cost") loadBudget();
    } catch (e) { setRecalcMsg(`重算失败: ${e}`); }
  };
  const weeklyReport = async () => {
    try {
      const html = await invoke<string>("ops_weekly_report");
      const path = await save({ defaultPath: `weekly-${new Date().toISOString().slice(0,10)}.html`, filters: [{ name: "HTML", extensions: ["html"] }] });
      if (typeof path === "string") await invoke("save_text_file", { path, content: html });
    } catch { }
  };

  return (
    <div className="ops-view">
      <div className="ops-toolbar">
        {section !== "assets" && (
          <div className="ops-range">
            {[[7,"7天"],[30,"30天"],[90,"90天"],[null,"全部"]].map(([v, label]) => (
              <button key={String(v)} className={`filter-chip ${range === v ? "active" : ""}`}
                onClick={() => setRange(v as number | null)}>{label as string}</button>
            ))}
          </div>
        )}
        <div style={{ display: "flex", gap: 8 }}>
          <button onClick={async () => { setSyncing(true); try { await invoke("ops_sync", { force: true }); } catch {} setSyncing(false); loadSection(section); }}>
            {syncing ? "⟳ 同步指标中…" : "↻ 同步指标"}
          </button>
        </div>
      </div>
      {recalcMsg && <div className="recalc-msg">{recalcMsg}</div>}

      {section === "overview" && (
        <OverviewSection overview={overview} byProvider={byProvider} byModel={byModel}
          timeseries={timeseries} topTools={topTools} cacheStats={cacheStats}
          health={health} latency={latency} waste={waste} benchmark={benchmark}
          loading={loading} onWeeklyReport={weeklyReport} />
      )}
      {section === "cost" && (
        <CostSection dirCosts={dirCosts} budget={budget} monthUsage={monthUsage}
          budgetInput={budgetInput} loading={loading}
          onBudgetInput={(f, v) => setBudgetInput((p) => ({ ...p, [f]: v }))}
          onSaveBudget={saveBudget} onRecalc={recalcCost} />
      )}
      {section === "security" && (
        <SecuritySection anomalies={anomalies} audit={audit} auditing={auditing}
          auditKindFilter={auditKindFilter} policies={policies} newPolicy={newPolicy}
          risky={risky} expandedRisk={expandedRisk} loading={loading}
          onScan={runAudit} onExportHtml={exportHtml} onFilter={setAuditKindFilter}
          onAddPolicy={addPolicy} onRemovePolicy={async (n) => { await invoke("policy_delete", { name: n }); loadPolicies(); }}
          onPolicyInput={(f, v) => setNewPolicy((p) => ({ ...p, [f]: v }))}
          onToggleRisk={(id) => setExpandedRisk((p) => { const n = new Set(p); n.has(id) ? n.delete(id) : n.add(id); return n; })}
          onJump={onJumpToConversation ?? (() => {})} />
      )}
      {section === "assets" && (
        <AssetsSection assets={assets} automations={automations} loading={loading} />
      )}
    </div>
  );
}
