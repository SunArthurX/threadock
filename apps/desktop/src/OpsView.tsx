// CodeAgentOps 治理视图（plan codeagent-ops M3）
// KPI 卡 + Agent 分布 donut + 每日趋势 bar + 模型/工具榜单 + 风险调用列表

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

// provider → 显示名/颜色（与设计系统一致）
const PROVIDER_META: Record<string, { label: string; color: string }> = {
  zcode: { label: "ZCode", color: "#4da3ff" },
  "claude-code": { label: "Claude Code", color: "#ef8b56" },
  cursor: { label: "Cursor", color: "#a78bfa" },
  "minimax-code": { label: "MiniMax", color: "#f478b4" },
  codex: { label: "Codex", color: "#3ddba0" },
};

const meta = (p: string) => PROVIDER_META[p] ?? { label: p, color: "#8b96ad" };

export default function OpsView() {
  const [range, setRange] = useState<number | null>(30);
  const [overview, setOverview] = useState<OpsOverview | null>(null);
  const [byProvider, setByProvider] = useState<ProviderUsage[]>([]);
  const [byModel, setByModel] = useState<ModelUsage[]>([]);
  const [timeseries, setTimeseries] = useState<DailyUsage[]>([]);
  const [topTools, setTopTools] = useState<ToolUsageRow[]>([]);
  const [risky, setRisky] = useState<RiskyCall[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [loading, setLoading] = useState(true);

  const loadAll = async () => {
    setLoading(true);
    try {
      const [ov, bp, bm, ts, tt, rc] = await Promise.all([
        invoke<OpsOverview>("ops_overview", { days: range }),
        invoke<ProviderUsage[]>("ops_by_provider", { days: range }),
        invoke<ModelUsage[]>("ops_by_model", { days: range }),
        invoke<DailyUsage[]>("ops_timeseries", { days: range }),
        invoke<ToolUsageRow[]>("ops_tool_toplist", { days: range, n: 10 }),
        invoke<RiskyCall[]>("ops_risky_calls", { days: range, n: 50 }),
      ]);
      setOverview(ov);
      setByProvider(bp);
      setByModel(bm);
      setTimeseries(ts);
      setTopTools(tt);
      setRisky(rc);
    } catch (e) {
      console.error("ops load failed", e);
    }
    setLoading(false);
  };

  // 首次进入同步一次指标，再加载
  useEffect(() => {
    (async () => {
      setSyncing(true);
      try {
        await invoke("ops_sync");
      } catch {
        /* 正在同步中时静默跳过 */
      }
      setSyncing(false);
      await loadAll();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [range]);

  const kpis = overview
    ? [
        { label: "模型请求", value: overview.total_requests.toLocaleString(), sub: `${overview.session_count} 会话` },
        { label: "总 Tokens", value: formatTokens(overview.total_tokens), sub: `in ${formatTokens(overview.input_tokens)} / out ${formatTokens(overview.output_tokens)}` },
        { label: "估算成本", value: formatCost(overview.cost_usd), sub: overview.cost_usd > 0 ? "来源侧计费" : "待定价" },
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

  return (
    <div className="ops-view">
      {/* 工具行 */}
      <div className="ops-toolbar">
        <div className="ops-range">
          {[
            [7, "7天"],
            [30, "30天"],
            [90, "90天"],
            [null, "全部"],
          ].map(([v, label]) => (
            <button
              key={String(v)}
              className={`filter-chip ${range === v ? "active" : ""}`}
              onClick={() => setRange(v as number | null)}
            >
              {label as string}
            </button>
          ))}
        </div>
        <button className="action-btn" disabled={syncing} onClick={async () => { setSyncing(true); try { await invoke("ops_sync"); } catch {} setSyncing(false); loadAll(); }}>
          {syncing ? "同步指标中…" : "↻ 同步指标"}
        </button>
      </div>

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
                  <tr>
                    <th>模型</th>
                    <th>请求</th>
                    <th>输入</th>
                    <th>输出</th>
                    <th>错误</th>
                  </tr>
                </thead>
                <tbody>
                  {byModel.map((m, i) => (
                    <tr key={i}>
                      <td className="mono">{m.model}</td>
                      <td>{m.requests.toLocaleString()}</td>
                      <td>{formatTokens(m.input_tokens)}</td>
                      <td>{formatTokens(m.output_tokens)}</td>
                      <td className={m.errors > 0 ? "text-danger" : ""}>{m.errors}</td>
                    </tr>
                  ))}
                  {byModel.length === 0 && (
                    <tr>
                      <td colSpan={5} className="ops-table-empty">暂无数据</td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            <div className="ops-card">
              <div className="ops-card-title">工具调用 Top 10</div>
              <div className="ops-tools">
                {topTools.map((t, i) => (
                  <div key={i} className="ops-tool-row">
                    <span className={`ops-tool-name mono ${t.destructive > 0 ? "text-danger" : ""}`}>
                      {t.tool_name}
                    </span>
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
                  {r.destructive && <span className="risk-flag destructive">危险</span>}
                  {r.exit_code != null && r.exit_code !== 0 && (
                    <span className="risk-flag error">exit {r.exit_code}</span>
                  )}
                  {r.approval_status && r.approval_status !== "none" && (
                    <span className="risk-flag approval">{r.approval_status}</span>
                  )}
                  <span className="ops-risky-cmd mono" title={r.command_text ?? ""}>
                    {r.command_text ?? r.source_session_id.slice(0, 18)}
                  </span>
                  <span className="ops-risky-time">{new Date(r.ts).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}</span>
                </div>
              ))}
              {risky.length === 0 && <div className="ops-table-empty">无风险调用 🎉</div>}
            </div>
          </div>

          {/* 汇总脚注 */}
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
