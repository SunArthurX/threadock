// CodeAgentOps 治理视图容器：状态管理 + 分区加载 + 委托渲染
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import OverviewSection from "./OverviewSection";
import CostSection from "./CostSection";
import SecuritySection from "./SecuritySection";
import AssetsSection from "./AssetsSection";
import type { Section, OpsOverview, ProviderUsage, ModelUsage, DailyUsage, ToolUsageRow, RiskyCall, AssetRow, AutomationRow, DirCost, CacheStat, AnomalyRow, AgentHealth, LatencyStat, TokenWaste, AgentBenchmark, AuditReport, PolicyRule, BudgetSettings } from "./ops-types";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";

type Props = {
  section: Section;
  onJumpToConversation?: (provider: string, sessionId: string, messageId: string | null) => void;
  /** 打开报告中心（概览页「报告中心」入口）。 */
  onOpenReports?: () => void;
  /**
   * 切换主视图（如 KPI 卡片点击后跳转到 chat/security/cost 等）。
   * 由 App.tsx 注入；未注入时 KPI 点击不响应（保持可访问性优雅降级）。
   */
  onChangeView?: (v: "chat" | "overview" | "cost" | "security" | "assets") => void;
  /**
   * 设置 chat 视图的搜索 query（用于按 dir/model 过滤的退化方案）。
   * TODO: chat view 暂无原生 dir/model filter；目前通过 search query 退化，
   *       后续应替换为专用 filter 状态。
   */
  onSetSearchQuery?: (q: string) => void;
};

export default function OpsView({ section, onJumpToConversation, onOpenReports, onChangeView, onSetSearchQuery }: Props) {
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
  const [usageSummary, setUsageSummary] = useState<import("./CostSection").UsageSummary | null>(null);
  const [cacheTrend, setCacheTrend] = useState<{ day: string; total_input: number; cache_read: number }[]>([]);
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
  /** 上次成功同步指标的时间戳（ms）。null = 尚未同步 / 同步失败。 */
  const [lastSyncedAt, setLastSyncedAt] = useState<number | null>(null);

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
      p(invoke<import("./CostSection").UsageSummary>("ops_usage_summary", {}).then(setUsageSummary), "summary");
      p(invoke<{ day: string; total_input: number; cache_read: number }[]>("ops_cache_trend", { days: range }).then(setCacheTrend), "cacheTrend");
      p(invoke<AgentHealth[]>("ops_agent_health", { days: range }).then(setHealth), "health");
      p(invoke<LatencyStat[]>("ops_latency_stats", { days: range }).then(setLatency), "latency");
      p(invoke<TokenWaste[]>("ops_token_waste", { days: range, n: 10 }).then(setWaste), "waste");
      p(invoke<AgentBenchmark[]>("ops_agent_benchmark", { days: range }).then(setBenchmark), "bench");
    } else if (sec === "cost") {
      // P1-B1: 之前 byProvider 永远不会在 cost 分支加载，导致按 Agent 成本分布柱状图空。
      p(invoke<ProviderUsage[]>("ops_by_provider", { days: range }).then(setByProvider), "prov");
      p(invoke<DirCost[]>("ops_cost_by_dir", { days: range, n: 10 }).then(setDirCosts), "dirCost");
      p(invoke<ModelUsage[]>("ops_by_model", { days: range }).then(setByModel), "model");
      // P1-B4: 之前写死 14 天；当 range=7 时后续周对比数据被截断。
      // 改为 max(range, 14) 保证周对比可用；range 越大越完整（不受 14 限制）。
      p(invoke<DailyUsage[]>("ops_timeseries", { days: Math.max(range ?? 90, 14) }).then(setTimeseries), "ts");
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
    } catch { /* 失败静默：后台/可选操作 */ }
  };

  const loadPolicies = async () => {
    try { setPolicies(await invoke<PolicyRule[]>("policy_list")); } catch { /* 失败静默：后台/可选操作 */ }
  };

  // section 切换：立即加载该 section + 后台同步后再刷新（effect 数据加载模式，
  // loadSection/loadBudget/loadPolicies 每次渲染重建，加入依赖会重复触发，有意省略）
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- loadSection 内部同步 setLoading(true)
    const tasks: Promise<void>[] = [loadSection(section)];
    if (section === "cost") tasks.push(loadBudget());
    if (section === "security") tasks.push(loadPolicies());
    (async () => {
      setSyncing(true);
      let syncOk = false;
      try {
        await invoke("ops_sync", { force: false });
        syncOk = true;
        await Promise.all([invoke("assets_sync", { force: false }).catch(() => { /* 后台任务失败不打断 UI */ }), invoke("automations_sync", { force: false }).catch(() => { /* 后台任务失败不打断 UI */ })]);
      } catch { /* 失败静默：后台/可选操作 */ }
      // P1-B2: 同步成功后才更新时间戳；失败保留旧值或 null
      if (syncOk) setLastSyncedAt(Date.now());
      setSyncing(false);
      loadSection(section);
      if (section === "cost") loadBudget();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [section]);

  useEffect(() => {
    if (section === "assets") return;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- range 变化后重载当前 section
    loadSection(section);
    if (section === "cost") loadBudget();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [range]);

  // ── actions ──
  const runAudit = async () => { setAuditing(true); try { setAudit(await invoke<AuditReport>("audit_scan")); } catch { /* 失败静默：后台/可选操作 */ } setAuditing(false); };
  const exportHtml = async () => {
    try {
      const html = await invoke<string>("audit_export_html");
      const path = await save({ defaultPath: `audit-${new Date().toISOString().slice(0,10)}.html`, filters: [{ name: "HTML", extensions: ["html"] }] });
      if (typeof path === "string") await invoke("save_text_file", { path, content: html });
    } catch { /* 失败静默：后台/可选操作 */ }
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
    } catch { /* 失败静默：后台/可选操作 */ }
  };

  return (
    <ScrollArea className="ops-view">
      <div className="ops-toolbar">
        {section !== "assets" && (
          <div className="ops-range">
            {[[7,"7天"],[30,"30天"],[90,"90天"],[null,"全部"]].map(([v, label]) => (
              <button key={String(v)} className={`filter-chip ${range === v ? "active" : ""}`}
                onClick={() => setRange(v as number | null)}>{label as string}</button>
            ))}
          </div>
        )}
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button onClick={async () => { setSyncing(true); let ok = false; try { await invoke("ops_sync", { force: true }); ok = true; } catch { /* 失败静默：后台/可选操作 */ } if (ok) setLastSyncedAt(Date.now()); setSyncing(false); loadSection(section); }}>
            {syncing ? "⟳ 同步指标中…" : "↻ 同步指标"}
          </button>
          {/* P1-B2: 数据新鲜度指示 — 超过 1 小时变 stale（黄） */}
          {lastSyncedAt != null && (() => {
            // 取最新时间显示「同步于 X 分钟前」，父级 setSyncing 触发重渲染。
            // eslint-disable-next-line react-hooks/purity
            const ageMs = Date.now() - lastSyncedAt;
            const stale = ageMs > 60 * 60_000;
            const min = Math.floor(ageMs / 60_000);
            const label = min < 1 ? "刚刚" : min < 60 ? `${min} 分钟前` : min < 1440 ? `${Math.floor(min / 60)} 小时前` : `${Math.floor(min / 1440)} 天前`;
            return <span className={`ops-freshness ${stale ? "stale" : "fresh"}`} title={`上次同步：${new Date(lastSyncedAt).toLocaleString("zh-CN")}`}>· 同步于 {label}</span>;
          })()}
        </div>
      </div>
      {recalcMsg && <div className="recalc-msg">{recalcMsg}</div>}

      {section === "overview" && (
        <OverviewSection cacheTrend={cacheTrend} overview={overview} byProvider={byProvider} byModel={byModel}
          timeseries={timeseries} topTools={topTools} cacheStats={cacheStats}
          health={health} latency={latency} waste={waste} benchmark={benchmark}
          loading={loading} onWeeklyReport={weeklyReport} onOpenReports={onOpenReports}
          onJump={(provider, sessionId) => onJumpToConversation?.(provider, sessionId, null)}
          onKpiJump={(kpi) => {
            // P0-4: KPI 卡片点击跳转目标视图
            if (kpi === "requests") onChangeView?.("chat");
            else if (kpi === "dangerous") onChangeView?.("security");
            else if (kpi === "cost") onChangeView?.("cost");
            else if (kpi === "tokens") onChangeView?.("overview");
          }} />
      )}
      {section === "cost" && (
        <CostSection summary={usageSummary} dirCosts={dirCosts} byProvider={byProvider}
          byModel={byModel} timeseries={timeseries}
          budget={budget} monthUsage={monthUsage}
          budgetInput={budgetInput} loading={loading}
          onBudgetInput={(f, v) => setBudgetInput((p) => ({ ...p, [f]: v }))}
          onSaveBudget={saveBudget} onRecalc={recalcCost}
          // P1-A1: 按目录/模型跳转 → 退化到 chat view + search query
          // TODO: chat view 暂无原生 dir/model filter，后续应替换为专用 filter
          onJumpByDir={(dir) => { onChangeView?.("chat"); onSetSearchQuery?.(`dir:${dir}`); }}
          onJumpByModel={(model) => { onChangeView?.("chat"); onSetSearchQuery?.(`model:${model}`); }} />
      )}
      {section === "security" && (
        <SecuritySection anomalies={anomalies} audit={audit} auditing={auditing}
          auditKindFilter={auditKindFilter} policies={policies} newPolicy={newPolicy}
          risky={risky} expandedRisk={expandedRisk} loading={loading}
          onScan={runAudit} onExportHtml={exportHtml} onFilter={setAuditKindFilter}
          onAddPolicy={addPolicy} onRemovePolicy={async (n) => { await invoke("policy_delete", { name: n }); loadPolicies(); }}
          onPolicyInput={(f, v) => setNewPolicy((p) => ({ ...p, [f]: v }))}
          onTogglePolicyEnabled={async (rule) => {
            await invoke("policy_upsert", { rule: { ...rule, enabled: !rule.enabled } });
            loadPolicies();
          }}
          onDisposeFinding={async (fingerprint, status) => {
            try { await invoke("audit_finding_set_state", { fingerprint, status }); } catch { /* 失败静默：后台/可选操作 */ }
          }}
          onBulkDisposeFindings={async (fingerprints, status) => {
            for (const fp of fingerprints) {
              try { await invoke("audit_finding_set_state", { fingerprint: fp, status }); } catch { /* 单条失败不影响整体 */ }
            }
          }}
          onRefreshAfterDispose={runAudit}
          onImportPolicies={async (json) => {
            try {
              const arr = JSON.parse(json);
              if (!Array.isArray(arr)) throw new Error("JSON 不是数组");
              let n = 0;
              for (const r of arr) {
                try { await invoke("policy_upsert", { rule: r }); n++; } catch { /* 单条失败 */ }
              }
              await loadPolicies();
              showToast(`✓ 已导入 ${n} 条策略规则`, "info");
            } catch (e) { showToast(`导入失败：${String(e)}`, "error"); }
          }}
          onToggleRisk={(id) => setExpandedRisk((p) => { const n = new Set(p); if (n.has(id)) { n.delete(id); } else { n.add(id); } return n; })}
          onJump={onJumpToConversation ?? (() => {})} />
      )}
      {section === "assets" && (
        <AssetsSection assets={assets} automations={automations} loading={loading} />
      )}
    </ScrollArea>
  );
}
