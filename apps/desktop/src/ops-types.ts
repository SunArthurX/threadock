// CodeAgentOps 共享类型定义
export interface OpsOverview {
  total_requests: number; total_tokens: number; input_tokens: number; output_tokens: number;
  cost_usd: number; avg_duration_ms: number; error_count: number; session_count: number;
  destructive_calls: number; total_tool_calls: number;
}
export interface ProviderUsage { provider: string; requests: number; total_tokens: number; output_tokens: number; errors: number; cost_usd: number; }
export interface ModelUsage { model: string; provider_id: string; requests: number; input_tokens: number; output_tokens: number; errors: number; }
export interface DailyUsage { day: string; total_tokens: number; requests: number; }
export interface ToolUsageRow { tool_name: string; calls: number; destructive: number; errors: number; avg_duration_ms: number; }
export interface RiskyCall {
  id: string; provider: string; source_session_id: string; tool_name: string; ts_ms: number;
  read_only: boolean | null; destructive: boolean | null; approval_status: string | null;
  exit_code: number | null; duration_ms: number | null; status: string; command_text: string | null;
}
export interface AssetRow {
  provider: string; kind: string; name: string; version: string | null;
  description: string | null; risky_hits: number; installed_at: string | null; path: string | null;
}
export interface AutomationRow { provider: string; name: string; kind: string; schedule: string | null; status: string | null; detail: string | null; }
export interface DirCost { dir: string; tokens: number; cost_usd: number; requests: number; }
export interface CacheStat { provider: string; input_tokens: number; cache_read_tokens: number; hit_rate: number; }
export interface AnomalyRow {
  kind: string; agent: string; detail: string; severity: string;
  provider?: string | null; source_session_id?: string | null;
}
export interface AgentHealth {
  provider: string; total_requests: number; errors: number; completed: number;
  retries: number; sessions: number; success_rate: number; error_rate: number;
  retry_rate: number; stability_score: number;
}
export interface LatencyStat { provider: string; sample_count: number; p50_ms: number; p95_ms: number; avg_ms: number; }
export interface TokenWaste {
  provider: string; session_id: string; input_tokens: number; output_tokens: number;
  ratio: number; requests: number; cache_read: number; waste_score: number;
}
export interface AgentBenchmark {
  provider: string; total_requests: number; total_tokens: number; cost_usd: number;
  sessions: number; success_rate: number; cache_hit_rate: number; avg_duration_ms: number;
  cost_per_session: number; tokens_per_session: number;
}
export interface AuditFinding {
  fingerprint: string;
  kind: string; severity: "low" | "medium" | "high"; rule: string; provider: string;
  source_conversation_id: string; conversation_title: string | null;
  message_id: string | null; tool_call_id: string | null; snippet: string;
}
export interface AuditReport {
  generated_at: string; scanned_messages: number; scanned_tool_calls: number;
  findings: AuditFinding[]; high: number; medium: number; low: number;
}
export interface PolicyRule { id: string; name: string; pattern: string; kind: string; severity: string; enabled: boolean; }
export interface BudgetSettings { monthly_token_limit: number | null; monthly_cost_limit: number | null; notify_on_exceed: boolean; }

export type Section = "overview" | "cost" | "security" | "assets";

export const PROVIDER_META: Record<string, { label: string; color: string }> = {
  zcode: { label: "ZCode", color: "#4da3ff" },
  "claude-code": { label: "Claude Code", color: "#ef8b56" },
  cursor: { label: "Cursor", color: "#a78bfa" },
  "minimax-code": { label: "MiniMax", color: "#f478b4" },
  codex: { label: "Codex", color: "#3ddba0" },
};
export const meta = (p: string) => PROVIDER_META[p] ?? { label: p, color: "#8b96ad" };

export const SEV_LABEL: Record<string, string> = { high: "高危", medium: "中危", low: "低危" };
