// 会话结构化摘要面板：从已入库上下文确定性解析（诉求/动作/文件/里程碑/
// 错误/结尾/规模），离线可用不依赖 LLM；「复制摘要」导出 Markdown 文本，
// 可直接喂给 LLM 做叙述式总结。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { copyToClipboard } from "./clipboard";
import { showToast } from "./toast";
import { formatTime, sourceLabel } from "./types";
import type { ConversationDigest } from "./types";

/**
 * 后端 DTO 兜底：数组/数字字段缺失时补默认值——面板不应因单个字段缺失
 * 把整个详情页打崩（IPC 异常或后端结构变更时降级为空摘要）。
 */
function normalize(d: ConversationDigest): ConversationDigest {
  return {
    ...d,
    user_messages: d.user_messages ?? 0,
    assistant_messages: d.assistant_messages ?? 0,
    system_messages: d.system_messages ?? 0,
    requests: d.requests ?? 0,
    context_total_tokens: d.context_total_tokens ?? 0,
    context_window_tokens: d.context_window_tokens ?? 0,
    commands_run: d.commands_run ?? 0,
    files_read: d.files_read ?? 0,
    files_created: d.files_created ?? 0,
    files_updated: d.files_updated ?? 0,
    errors: d.errors ?? 0,
    thinking_events: d.thinking_events ?? 0,
    system_block_events: d.system_block_events ?? 0,
    top_files: d.top_files ?? [],
    recent_errors: d.recent_errors ?? [],
    milestones: d.milestones ?? [],
  };
}

/** 把结构化摘要拼成 Markdown 文本（复制导出 / LLM 提示素材）。 */
function digestToMarkdown(d: ConversationDigest): string {
  const lines: string[] = [];
  lines.push(`## ${t("会话摘要")}：${d.title ?? t("（无标题）")}`);
  const src = [sourceLabel(d.provider), d.model, d.updated_at_ms ? formatTime(d.updated_at_ms) : null]
    .filter(Boolean)
    .join(" · ");
  lines.push(`- ${src}`);
  lines.push(
    `- ${t("规模")}：${t("用户")} ${d.user_messages} ${t("轮")} · ${t("助手")} ${d.assistant_messages} · ${t("请求")} ${d.requests} · ${t("上下文")} ${d.context_total_tokens}/${d.context_window_tokens}`,
  );
  if (d.topic_opening) lines.push(`- ${t("诉求（开头）")}：${d.topic_opening}`);
  if (d.topic_latest) lines.push(`- ${t("诉求（最近）")}：${d.topic_latest}`);
  const actions = [
    `${t("命令")} ${d.commands_run}`,
    `${t("读文件")} ${d.files_read}`,
    `${t("新建")} ${d.files_created}`,
    `${t("修改")} ${d.files_updated}`,
    `${t("错误")} ${d.errors}`,
    `${t("思考")} ${d.thinking_events}`,
    `${t("系统注入")} ${d.system_block_events}`,
  ].join(" · ");
  lines.push(`- ${t("动作")}：${actions}`);
  if (d.top_files.length > 0) lines.push(`- ${t("主要文件")}：${d.top_files.join("、")}`);
  if (d.milestones.length > 0) lines.push(`- ${t("里程碑")}：${d.milestones.join(" / ")}`);
  if (d.recent_errors.length > 0) lines.push(`- ${t("错误")}：${d.recent_errors.join(" / ")}`);
  if (d.closing_reply) lines.push(`- ${t("结尾回复")}：${d.closing_reply}`);
  return lines.join("\n");
}

/** 时长人类可读（分钟级起步）。 */
function fmtDuration(startMs: number | null, endMs: number | null): string | null {
  if (startMs == null || endMs == null || endMs <= startMs) return null;
  const min = Math.round((endMs - startMs) / 60_000);
  if (min < 1) return `<1${t("分钟")}`;
  if (min < 60) return `${min} ${t("分钟")}`;
  return `${Math.floor(min / 60)} ${t("小时")} ${min % 60} ${t("分钟")}`;
}

export default function ConversationDigestPanel({ convId }: { convId: string }) {
  const [digest, setDigest] = useState<ConversationDigest | null>(null);
  useEffect(() => {
    let cancelled = false;
    setDigest(null);
    invoke<ConversationDigest | null>("conversation_digest", { conversationId: convId })
      .then((d) => {
        if (!cancelled) setDigest(d ? normalize(d) : null);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [convId]);

  if (!digest) return null;
  const duration = fmtDuration(digest.started_at_ms, digest.updated_at_ms);
  const meta = [
    sourceLabel(digest.provider),
    digest.model,
    duration,
    `${t("用户")} ${digest.user_messages} ${t("轮")}`,
    `${t("助手")} ${digest.assistant_messages}`,
  ]
    .filter(Boolean)
    .join(" · ");

  const copy = () => {
    void copyToClipboard(digestToMarkdown(digest)).then((r) => {
      showToast(r.ok ? t("摘要已复制") : `${t("复制失败")}：${r.error ?? ""}`, r.ok ? "info" : "error");
    });
  };

  return (
    <div className="digest-panel">
      <div className="digest-head">
        <span className="digest-title">{t("会话摘要")}</span>
        <button className="digest-copy-btn" onClick={copy}>
          {t("复制摘要")}
        </button>
      </div>
      <div className="digest-meta" title={meta}>
        {meta}
      </div>
      {(digest.topic_opening || digest.topic_latest) && (
        <div className="digest-row">
          <span className="digest-label">{t("诉求")}</span>
          <span className="digest-value">
            {digest.topic_opening && <span title={digest.topic_opening}>{t("开头")}：{digest.topic_opening}</span>}
            {digest.topic_opening && digest.topic_latest && <span className="digest-arrow"> → </span>}
            {digest.topic_latest && <span title={digest.topic_latest}>{digest.topic_latest}</span>}
          </span>
        </div>
      )}
      <div className="digest-row">
        <span className="digest-label">{t("动作")}</span>
        <span className="digest-value mono">
          {t("命令")} {digest.commands_run} · {t("读")} {digest.files_read} · {t("新建")} {digest.files_created} ·{" "}
          {t("修改")} {digest.files_updated} · {t("错误")} {digest.errors} · 💭 {digest.thinking_events} · 🧩{" "}
          {digest.system_block_events}
        </span>
      </div>
      {digest.top_files.length > 0 && (
        <div className="digest-row">
          <span className="digest-label">{t("主要文件")}</span>
          <span className="digest-value mono">{digest.top_files.join("、")}</span>
        </div>
      )}
      {digest.milestones.length > 0 && (
        <div className="digest-row">
          <span className="digest-label">{t("里程碑")}</span>
          <span className="digest-value">{digest.milestones.slice(0, 4).join(" / ")}</span>
        </div>
      )}
      {digest.recent_errors.length > 0 && (
        <div className="digest-row digest-row-err">
          <span className="digest-label">{t("错误")}</span>
          <span className="digest-value">{digest.recent_errors.join(" / ")}</span>
        </div>
      )}
      {digest.closing_reply && (
        <div className="digest-row">
          <span className="digest-label">{t("结尾")}</span>
          <span className="digest-value" title={digest.closing_reply}>
            {digest.closing_reply}
          </span>
        </div>
      )}
    </div>
  );
}
