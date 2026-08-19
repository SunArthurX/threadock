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
export interface SourceSession {
  session_id: string; title: string; detail: string;
  message_count: number | null; imported: boolean;
}
export interface EventDto {
  created_at_ms?: number | null;
  id: string; event_type: string; summary: string | null; sequence_number: number;
}
export interface ConversationDetailDto {
  tags?: string[];
  conversation: Conversation; messages: Message[]; events: EventDto[]; completeness_label: string;
}
export interface ExtractionResult {
  summary: string;
  decisions: { decision: string }[];
  todos: { text: string }[];
  errors: { error: string }[];
  commands: string[];
  files: { path: string }[];
  extractor: string;
}
export interface ExportOutput { content: string; format: string; filename: string; }

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
