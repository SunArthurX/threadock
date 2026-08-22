// 概览 tab 的纯展示卡片组件
import { useCountUp } from "./charts";
import { Icon, type IconName } from "./Icon";
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

export function AnimatedKpi({ label, num, fmt, sub, danger, icon, onClick }: {
  label: string; num: number; fmt: (v: number) => string; sub: string; danger?: boolean; icon?: IconName; onClick?: () => void;
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
      {icon && <span className="ops-kpi-icon"><Icon name={icon} size={14} /></span>}
      <div className="ops-kpi-value">{fmt(v)}</div>
      <div className="ops-kpi-label">{label}</div>
      <div className="ops-kpi-sub">{sub}</div>
    </div>
  );
}

