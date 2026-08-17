// 概览 tab 的纯展示卡片组件
import { BarChart, DonutChart, formatTokens, formatCost, useCountUp } from "./charts";
export interface OpsOverview { total_requests:number; total_tokens:number; input_tokens:number; output_tokens:number; cost_usd:number; avg_duration_ms:number; error_count:number; session_count:number; destructive_calls:number; total_tool_calls:number; }
export interface ProviderUsage { provider:string; requests:number; total_tokens:number; output_tokens:number; errors:number; }
export interface DailyUsage { day:string; total_tokens:number; requests:number; }

export const PROVIDER_META: Record<string, { label: string; color: string }> = {
  zcode: { label: "ZCode", color: "#4da3ff" },
  "claude-code": { label: "Claude Code", color: "#ef8b56" },
  cursor: { label: "Cursor", color: "#a78bfa" },
  "minimax-code": { label: "MiniMax", color: "#f478b4" },
  codex: { label: "Codex", color: "#3ddba0" },
};
export const meta = (p: string) => PROVIDER_META[p] ?? { label: p, color: "#8b96ad" };

export function AnimatedKpi({ label, num, fmt, sub, danger, onClick }: {
  label: string; num: number; fmt: (v: number) => string; sub: string; danger?: boolean; onClick?: () => void;
}) {
  const v = useCountUp(num);
  const interactive = !!onClick;
  return (
    <div
      className={`ops-kpi ${danger ? "danger" : ""} ${interactive ? "clickable" : ""}`}
      onClick={onClick}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onKeyDown={interactive ? (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onClick?.(); } } : undefined}
    >
      <div className="ops-kpi-value">{fmt(v)}</div>
      <div className="ops-kpi-label">{label}</div>
      <div className="ops-kpi-sub">{sub}</div>
    </div>
  );
}

export function KpiRow({ overview }: { overview: OpsOverview | null }) {
  if (!overview) return null;
  const kpis = [
    { label: "模型请求", num: overview.total_requests, fmt: (v: number) => Math.round(v).toLocaleString(), sub: `${overview.session_count} 会话` },
    { label: "总 Tokens", num: overview.total_tokens, fmt: formatTokens, sub: `in ${formatTokens(overview.input_tokens)} / out ${formatTokens(overview.output_tokens)}` },
    { label: "估算成本", num: overview.cost_usd, fmt: formatCost, sub: "按 pricing.json 定价" },
    { label: "危险操作", num: overview.destructive_calls, fmt: (v: number) => String(Math.round(v)), sub: `${overview.total_tool_calls.toLocaleString()} 次工具调用`, danger: overview.destructive_calls > 0 },
  ];
  return <div className="ops-kpis">{kpis.map((k, i) => <AnimatedKpi key={i} {...k} />)}</div>;
}

export function ChartsRow({ byProvider, timeseries }: { byProvider: ProviderUsage[]; timeseries: DailyUsage[] }) {
  const slices = byProvider.filter((p) => p.total_tokens > 0)
    .map((p) => ({ label: meta(p.provider).label, value: p.total_tokens, color: meta(p.provider).color }));
  const barData = timeseries.map((d) => ({ label: d.day, value: d.total_tokens, title: `${d.day}: ${formatTokens(d.total_tokens)}` }));
  return (
    <div className="ops-charts">
      <div className="ops-card">
        <div className="ops-card-title">Agent 用量分布</div>
        <div className="ops-donut-wrap">
          <DonutChart slices={slices} />
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
  );
}
