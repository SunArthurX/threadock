// 概览 Section：KPI + 图表 + 模型/工具榜 + 缓存 + 健康 + 延迟 + 浪费 + 对比
import { BarChart, DonutChart, formatTokens, formatCost, formatDuration } from "./charts";
import { AnimatedKpi } from "./OverviewCards";
import type { OpsOverview, ProviderUsage, ModelUsage, DailyUsage, ToolUsageRow, CacheStat, AgentHealth, LatencyStat, TokenWaste, AgentBenchmark } from "./ops-types";
import { meta } from "./ops-types";

interface Props {
  overview: OpsOverview | null;
  byProvider: ProviderUsage[];
  byModel: ModelUsage[];
  timeseries: DailyUsage[];
  topTools: ToolUsageRow[];
  cacheStats: CacheStat[];
  health: AgentHealth[];
  latency: LatencyStat[];
  waste: TokenWaste[];
  benchmark: AgentBenchmark[];
  loading: boolean;
  onWeeklyReport: () => void;
}

export default function OverviewSection({
  overview, byProvider, byModel, timeseries, topTools, cacheStats,
  health, latency, waste, benchmark, loading, onWeeklyReport,
}: Props) {
  const donutSlices = byProvider.filter((p) => p.total_tokens > 0)
    .map((p) => ({ label: meta(p.provider).label, value: p.total_tokens, color: meta(p.provider).color }));
  const barData = timeseries.map((d) => ({ label: d.day, value: d.total_tokens }));
  const maxToolCalls = Math.max(...topTools.map((t) => t.calls), 1);
  const kpis = overview ? [
    { label: "模型请求", num: overview.total_requests, fmt: (v: number) => Math.round(v).toLocaleString(), sub: `${overview.session_count} 会话` },
    { label: "总 Tokens", num: overview.total_tokens, fmt: formatTokens, sub: `in ${formatTokens(overview.input_tokens)} / out ${formatTokens(overview.output_tokens)}` },
    { label: "估算成本", num: overview.cost_usd, fmt: formatCost, sub: "按定价" },
    { label: "危险操作", num: overview.destructive_calls, fmt: (v: number) => String(Math.round(v)), sub: `${overview.total_tool_calls.toLocaleString()} 工具`, danger: overview.destructive_calls > 0 },
  ] : [];

  return (
    <>
      {overview ? <div className="ops-kpis">{kpis.map((k, i) => <AnimatedKpi key={i} {...k} />)}</div>
      : <div className="ops-kpis">{[0,1,2,3].map((i) => (
          <div key={i} className="ops-kpi skeleton"><div className="sk-line sk-lg" /><div className="sk-line" /><div className="sk-line sk-sm" /></div>
        ))}</div>}

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

      <div className="ops-tables">
        <div className="ops-card">
          <div className="ops-card-title">模型明细</div>
          <table className="ops-table">
            <thead><tr><th>模型</th><th>请求</th><th>输入</th><th>输出</th><th>错误</th></tr></thead>
            <tbody>
              {byModel.map((m, i) => (
                <tr key={i}>
                  <td className="mono">{m.model === "(unknown)" ? `${meta(m.provider_id.replace("prov_","")).label} ·默认` : m.model}</td>
                  <td>{m.requests.toLocaleString()}</td>
                  <td>{formatTokens(m.input_tokens)}</td>
                  <td>{formatTokens(m.output_tokens)}</td>
                  <td className={m.errors > 0 ? "text-danger" : ""}>{m.errors}</td>
                </tr>
              ))}
              {byModel.length === 0 && <tr><td colSpan={5} className="ops-table-empty">{loading ? "加载中…" : "暂无数据"}</td></tr>}
            </tbody>
          </table>
        </div>
        <div className="ops-card">
          <div className="ops-card-title">工具调用 Top 10</div>
          <div className="ops-tools">
            {topTools.map((t, i) => (
              <div key={i} className="ops-tool-row">
                <span className={`ops-tool-name mono ${t.destructive > 0 ? "text-danger" : ""}`}>{t.tool_name}</span>
                <div className="ops-tool-bar-bg"><div className="ops-tool-bar" style={{ width: `${(t.calls / maxToolCalls) * 100}%` }} /></div>
                <span className="ops-tool-calls">{t.calls.toLocaleString()}</span>
                {t.destructive > 0 && <span className="badge completeness 有限">{t.destructive} 危险</span>}
              </div>
            ))}
            {topTools.length === 0 && <div className="ops-table-empty">{loading ? "加载中…" : "暂无数据"}</div>}
          </div>
        </div>
      </div>

      {benchmark.length > 1 && (
        <div className="ops-card">
          <div className="ops-card-title">
            📐 Agent 横向对比
            <button className="action-btn" style={{ marginLeft: "auto", fontSize: 11 }} onClick={onWeeklyReport}>📄 周报</button>
          </div>
          <div style={{ overflowX: "auto" }}>
            <table className="ops-table">
              <thead><tr><th>指标</th>{benchmark.map((b, i) => <th key={i}>{meta(b.provider).label}</th>)}</tr></thead>
              <tbody>
                <tr><td style={{ fontWeight: 600 }}>请求</td>{benchmark.map((b, i) => <td key={i}>{b.total_requests.toLocaleString()}</td>)}</tr>
                <tr><td style={{ fontWeight: 600 }}>Tokens</td>{benchmark.map((b, i) => <td key={i}>{formatTokens(b.total_tokens)}</td>)}</tr>
                <tr><td style={{ fontWeight: 600 }}>成本</td>{benchmark.map((b, i) => <td key={i}>{formatCost(b.cost_usd)}</td>)}</tr>
                <tr><td style={{ fontWeight: 600 }}>成功率</td>{benchmark.map((b, i) => <td key={i} style={{ color: b.success_rate > 95 ? "var(--c-codex)" : b.success_rate > 80 ? "var(--warn)" : "var(--danger)" }}>{b.success_rate.toFixed(1)}%</td>)}</tr>
                <tr><td style={{ fontWeight: 600 }}>缓存命中</td>{benchmark.map((b, i) => <td key={i}>{b.cache_hit_rate.toFixed(1)}%</td>)}</tr>
                <tr><td style={{ fontWeight: 600 }}>$/会话</td>{benchmark.map((b, i) => <td key={i}>${b.cost_per_session.toFixed(2)}</td>)}</tr>
              </tbody>
            </table>
          </div>
        </div>
      )}

      <div className="ops-card">
        <div className="ops-card-title">🏥 Agent 健康度</div>
        {health.length === 0 ? <div className="ops-table-empty">{loading ? "加载中…" : "暂无数据"}</div> : (
          <table className="ops-table">
            <thead><tr><th>Agent</th><th>请求</th><th>成功率</th><th>错误率</th><th>重试率</th><th>稳定性</th></tr></thead>
            <tbody>
              {health.map((h, i) => (
                <tr key={i}>
                  <td><span className={`badge source ${h.provider}`}>{meta(h.provider).label}</span></td>
                  <td>{h.total_requests.toLocaleString()}</td>
                  <td style={{ color: h.success_rate > 95 ? "var(--c-codex)" : h.success_rate > 80 ? "var(--warn)" : "var(--danger)" }}>{h.success_rate.toFixed(1)}%</td>
                  <td>{h.error_rate.toFixed(1)}%</td>
                  <td>{h.retry_rate.toFixed(1)}%</td>
                  <td>
                    <div className="ops-tool-bar-bg" style={{ width: 50, display: "inline-block", marginRight: 4 }}>
                      <div className="ops-tool-bar" style={{ width: `${h.stability_score}%`, background: h.stability_score > 80 ? "var(--c-codex)" : h.stability_score > 50 ? "var(--warn)" : "var(--danger)" }} />
                    </div>{h.stability_score.toFixed(0)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {latency.length > 0 && (
        <div className="ops-card">
          <div className="ops-card-title">⚡ 延迟 P50 / P95</div>
          <div className="ops-risky">
            {latency.map((l, i) => (
              <div key={i} className="ops-tool-row">
                <span className="ops-tool-name">{meta(l.provider).label}</span>
                <span className="mono" style={{ fontSize: 11 }}>P50 {formatDuration(l.p50_ms)}</span>
                <span className="mono" style={{ fontSize: 11, color: l.p95_ms > 30000 ? "var(--danger)" : "var(--text-muted)" }}>P95 {formatDuration(l.p95_ms)}</span>
                <span className="legend-req">{l.sample_count.toLocaleString()} 样本</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {waste.length > 0 && (
        <div className="ops-card">
          <div className="ops-card-title">🔥 Token 浪费检测（{waste.length}）</div>
          <div className="ops-risky">
            {waste.map((w, i) => (
              <div key={i} className="ops-risky-row">
                <span className={`badge source ${w.provider}`}>{meta(w.provider).label}</span>
                <span className="mono" style={{ fontSize: 10.5 }}>{w.session_id.slice(0, 16)}…</span>
                <span className="risk-flag medium">{w.ratio.toFixed(0)}×</span>
                <span className="mono" style={{ fontSize: 10.5 }}>in {formatTokens(w.input_tokens)} / out {formatTokens(w.output_tokens)}</span>
                <span className="legend-req">缓存 {formatTokens(w.cache_read)}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="ops-card">
        <div className="ops-card-title">缓存命中率</div>
        {cacheStats.length === 0 ? <div className="ops-table-empty">{loading ? "加载中…" : "暂无数据"}</div> : cacheStats.map((c) => (
          <div key={c.provider} className="ops-tool-row">
            <span className="ops-tool-name">{meta(c.provider).label}</span>
            <div className="ops-tool-bar-bg">
              <div className="ops-tool-bar" style={{ width: `${(c.hit_rate * 100).toFixed(1)}%`, background: meta(c.provider).color }} />
            </div>
            <span className="ops-tool-calls">{(c.hit_rate * 100).toFixed(1)}%</span>
            <span className="legend-req">{formatTokens(c.cache_read_tokens)} 缓存</span>
          </div>
        ))}
      </div>
    </>
  );
}
