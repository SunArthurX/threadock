// 共享类型定义（与 Rust serde 对应）

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

export const COLLAPSE_THRESHOLD = 600;

export const sourceLabel = (p: string): string => {
  const map: Record<string, string> = {
    "claude-code": "Claude Code", zcode: "ZCode", codex: "Codex",
    cursor: "Cursor", "minimax-code": "MiniMax", opencode: "OpenCode", generic: "导入",
  };
  return map[p] ?? p;
};

export const eventTypeLabel = (t: string): string => {
  const map: Record<string, string> = {
    command_started: "命令", command_completed: "命令完成", diff_generated: "变更",
    tool_call_started: "工具", tool_call_completed: "工具完成",
    file_read: "读取文件", file_created: "新建文件", file_updated: "修改文件", file_deleted: "删除文件",
    approval_requested: "请求审批", approval_granted: "批准", approval_denied: "拒绝",
    error: "错误", artifact_created: "产物",
  };
  return map[t] ?? t;
};

export const formatTime = (ms: number | null): string => {
  if (!ms) return "";
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}:${String(d.getSeconds()).padStart(2, "0")}`;
};
