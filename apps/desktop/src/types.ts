// 共享类型定义（与 Rust serde 对应）
import { t } from "./i18n";

export interface Conversation {
  favorite?: boolean;
  archived?: boolean;
  id: string; provider: string; source_conversation_id: string;
  title: string | null; user_title: string | null; status: string | null;
  model: string | null; completeness_score: number | null;
  workspace_id: string | null; started_at_ms: number | null; updated_at_ms: number | null;
  source_parent_id: string | null; child_count: number;
}
export interface Message {
  id: string; role: string; content_text: string | null;
  sequence_number: number; created_at_ms: number | null;
}
export interface SearchResult {
  message_id: string; conversation_id: string; provider: string;
  role: string; title: string | null; snippet: string;
}
/** 搜索命中按「主对话」分组的聚合行（左栏搜索模式）：
 *  root_* 是主对话信息；conversation_id 是实际命中的会话（is_child=true 时为子任务）。 */
export interface SearchHitGroup {
  root_conversation_id: string;
  root_title: string | null;
  root_updated_at_ms: number | null;
  provider: string;
  conversation_id: string;
  title: string | null;
  is_child: boolean;
  hit_count: number;
  best_message_id: string;
  best_role: string;
  snippet: string;
}
export interface ImportResultDto {
  conversation_id: string; workspace_id: string | null;
  messages: number; events: number; completeness: string;
}
export interface EventDto {
  created_at_ms?: number | null;
  id: string; event_type: string; summary: string | null; sequence_number: number;
  /** 事件状态（completed/failed 等，若有）。 */
  status?: string | null;
  /** 完成时间（Unix 毫秒；与 created_at_ms 相减得耗时）。 */
  completed_at_ms?: number | null;
  /** payload JSON 字符串（事件详情；超 8KB 后端已截断）。 */
  payload_json?: string | null;
}
export interface ConversationDetailDto {
  tags?: string[];
  conversation: Conversation; messages: Message[]; events: EventDto[]; completeness_label: string;
}
/** 一次 AI 提取运行记录（成功/失败均留痕，时间倒序）。 */
export interface LlmRunRecord {
  id: string;
  conversation_id: string;
  status: "success" | "failed" | string;
  error: string | null;
  extractor: string;
  input_messages: number;
  input_chars: number;
  items_total: number;
  duration_ms: number;
  created_at_ms: number;
}
export interface ExtractionResult {
  summary: string;
  decisions: { decision: string }[];
  todos: { text: string; status?: "pending" | "done" | "stale" }[];
  errors: { error: string; solution?: string | null }[];
  commands: string[];
  files: { path: string }[];
  extractor: string;
}
export interface ExportOutput { content: string; format: string; filename: string; }

/** 交接目标清单（handoff_targets）：本机是否检测到该 Agent 的会话存储。 */
export interface HandoffTargetInfo { target: string; available: boolean; destination: string; }
/** 单条会话交接成功结果（目标侧新会话 ID + 落点）。 */
export interface HandoffOutcome {
  target: string; conversation_id: string; title: string; session_id: string;
  destination: string; items_written: number; dry_run: boolean;
}
export interface HandoffFailure { conversation_id: string; title: string; error: string; }
export interface HandoffTargetResult { target: string; ok: HandoffOutcome[]; failed: HandoffFailure[]; }
export interface HandoffSummary { matched: number; missing: string[]; dry_run: boolean; results: HandoffTargetResult[]; }
/** 可交接目标（与后端 HandoffTarget::all 顺序一致）。 */
export const HANDOFF_TARGETS = ["claude-code", "codex", "zcode", "minimax-code", "cursor"] as const;
export type HandoffTarget = typeof HANDOFF_TARGETS[number];

/** LLM 提取配置视图（后端 llm_config_get 返回）：永远不含密钥明文/密文。 */
export interface LlmConfigView {
  enabled: boolean;
  base_url: string;
  model: string;
  timeout_secs: number;
  max_input_chars: number;
  has_api_key: boolean;
  /** 打码提示（如 sk-***1234）；密文损坏时为 null。 */
  api_key_masked: string | null;
  /** 本地推理端点（数据不出本机）。 */
  is_local: boolean;
  /** 密文存在但解不开（如换设备）：提示重新录入。 */
  api_key_broken: boolean;
}

/** 知识提取引擎：规则（默认，离线确定性）/ 大模型（需在设置中启用）。 */
export type KnowledgeEngine = "rule" | "llm";

/** 单会话结构化摘要（conversation_digest 命令，确定性解析上下文）。 */
export interface ConversationDigest {
  title: string | null;
  provider: string;
  model: string | null;
  started_at_ms: number | null;
  updated_at_ms: number | null;
  user_messages: number;
  assistant_messages: number;
  system_messages: number;
  requests: number;
  context_total_tokens: number;
  context_window_tokens: number;
  topic_opening: string | null;
  topic_latest: string | null;
  closing_reply: string | null;
  commands_run: number;
  files_read: number;
  files_created: number;
  files_updated: number;
  errors: number;
  top_files: string[];
  recent_errors: string[];
  milestones: string[];
  thinking_events: number;
  system_block_events: number;
}

export const COLLAPSE_THRESHOLD = 600;

/** 单会话上下文窗口用量分类（conversation_context_usage 命令）。 */
export interface ContextUsageCategory {
  key: "messages" | "tool_outputs" | "thinking" | "system_blocks" | "system_other";
  tokens: number;
}

export interface ContextUsageSummary {
  /** 上下文占用 token：实测（末条 usage 的 input+cache_read+cache_write）或文本估算。 */
  total_tokens: number;
  measured: boolean;
  window_tokens: number;
  model: string | null;
  cache_hit_rate: number | null;
  categories: ContextUsageCategory[];
  /** 明细：请求数 / 缓存绝对量 / 输出与推理 token / 末次请求时间。 */
  turn_count: number;
  fresh_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  last_turn_at_ms: number | null;
  /** 每 turn 上下文规模（升序，最多 40 点），sparkline 数据。 */
  trend: number[];
}

export const sourceLabel = (p: string): string => {
  const map: Record<string, string> = {
    "claude-code": "Claude Code", zcode: "ZCode", codex: "Codex",
    cursor: "Cursor", "minimax-code": "MiniMax", "deepseek-harness": "dsh", opencode: "OpenCode", generic: t("导入"),
  };
  return map[p] ?? p;
};

export const eventTypeLabel = (et: string): string => {
  const map: Record<string, string> = {
    command_started: t("命令"), command_completed: t("命令完成"), diff_generated: t("变更"),
    tool_call_started: t("工具"), tool_call_completed: t("工具完成"),
    file_read: t("读取文件"), file_created: t("新建文件"), file_updated: t("修改文件"), file_deleted: t("删除文件"),
    approval_requested: t("请求审批"), approval_granted: t("批准"), approval_denied: t("拒绝"),
    error: t("错误"), artifact_created: t("产物"),
    thinking: t("思考过程"), system_block: t("系统注入"),
  };
  return map[et] ?? et;
};

export const formatTime = (ms: number | null): string => {
  if (!ms) return "";
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}:${String(d.getSeconds()).padStart(2, "0")}`;
};
