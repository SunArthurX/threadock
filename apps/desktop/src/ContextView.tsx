// 全保真上下文查看插件（thinking / 系统注入块 / 工具完整输出）。
//
// 设计参照主流 LLM 客户端的 reasoning 展示惯例（Claude/ChatGPT/v0 的
// collapsible thinking 模式，见 northbase.design 对 233 个 AI 输出设计的分析）：
// 1. 默认折叠——思考/注入是脚注不是正文，摘要行 + 字数，点击展开；
// 2. 视觉区分——独立配色与等宽字体，与对话正文明确分隔；
// 3. 懒加载全文——详情 DTO 的 payload 超 8KB 截断，展开时按事件 id 拉
//    event_payload_full 全文，避免长输出撑爆首屏 DOM；
// 4. 复制一键化——长输出/思考的主要消费方式是复制出来用。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { copyToClipboard } from "./clipboard";
import type { EventDto } from "./types";

/** 后端 DTO 截断标记（dto.rs PAYLOAD_MAX_CHARS 截断后追加）。 */
const TRUNCATED_SUFFIX = "…（已截断）";

/** thinking / 系统注入块事件 → 走上下文插件渲染（其余走通用事件行）。 */
export function isContextEvent(eventType: string): boolean {
  return eventType === "thinking" || eventType === "system_block";
}

function parsePayload(raw: string | null | undefined): Record<string, unknown> | null {
  if (!raw) return null;
  try {
    const v: unknown = JSON.parse(raw);
    return typeof v === "object" && v !== null ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** 从 payload 里按序取第一个非空字符串字段（thinking/block/output/command/cmd…）。 */
function payloadText(p: Record<string, unknown> | null, keys: string[]): string | null {
  if (!p) return null;
  for (const k of keys) {
    const v = p[k];
    if (typeof v === "string" && v.trim()) return v;
  }
  return null;
}

/** 截断 payload → 展开时懒加载全文（event_payload_full 不截断）。 */
function useContextFullText(
  initial: string,
  eventId: string,
  keys: string[],
): { text: string; truncated: boolean; loading: boolean } {
  const [full, setFull] = useState<string | null>(null);
  const truncated = initial.endsWith(TRUNCATED_SUFFIX);
  useEffect(() => {
    if (!truncated || full !== null) return;
    let cancelled = false;
    invoke<string | null>("event_payload_full", { eventId })
      .then((payload) => {
        if (cancelled) return;
        const txt = payloadText(parsePayload(payload), keys);
        setFull(txt ?? payload ?? initial);
      })
      .catch(() => {
        if (!cancelled) setFull(initial);
      });
    return () => {
      cancelled = true;
    };
  }, [truncated, full, eventId, initial, keys]);
  return { text: full ?? initial, truncated, loading: truncated && full === null };
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="ctx-copy-btn"
      onClick={(ev) => {
        ev.stopPropagation();
        void copyToClipboard(text).then(() => {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        });
      }}
      title={t("复制全部")}
    >
      {copied ? t("已复制") : t("复制")}
    </button>
  );
}

/** 等宽滚动正文区（max-height 与事件 payload 面板一致 280px，可滚）。 */
function ContextBody({ text }: { text: string }) {
  return <pre className="ctx-body mono">{text}</pre>;
}

const THINKING_KEYS = ["thinking", "text"];
const BLOCK_KEYS = ["block", "thinking", "text"];
const OUTPUT_KEYS = ["output", "stdout", "result"];

/** thinking 事件的行内容：💭 + 字数（+ 思考耗时，若有）。 */
export function ThinkingRow({ e }: { e: EventDto }) {
  const [open, setOpen] = useState(false);
  const keys = THINKING_KEYS;
  const initial = payloadText(parsePayload(e.payload_json), keys) ?? e.summary ?? "";
  const { text, truncated, loading } = useContextFullText(initial, e.id, keys);
  const dur =
    e.payload_json != null
      ? (parsePayload(e.payload_json)?.["duration_ms"] as number | undefined)
      : undefined;
  const chars = text.length;
  return (
    <div className={`ctx-row-wrap ${open ? "open" : ""}`}>
      <button className="ctx-row ctx-thinking" onClick={() => setOpen((o) => !o)}>
        <span className="ctx-icon">💭</span>
        <span className="ctx-label">{t("思考过程")}</span>
        <span className="ctx-meta">
          {loading ? t("加载全文…") : `${chars.toLocaleString()} ${t("字")}`}
          {dur != null && dur > 0 && ` · ${(dur / 1000).toFixed(1)}s`}
          {open ? " ▾" : " ▸"}
        </span>
      </button>
      {open && (
        <div className="ctx-detail">
          {!loading && truncated && <div className="ctx-truncated-hint">{t("原文超 8KB，已加载全文")}</div>}
          {!loading && <ContextBody text={text} />}
          {!loading && <CopyButton text={text} />}
        </div>
      )}
    </div>
  );
}

/** system_block 事件的行内容：🧩 + 摘要（hook 名/注入类型）。 */
export function SystemBlockRow({ e }: { e: EventDto }) {
  const [open, setOpen] = useState(false);
  const keys = BLOCK_KEYS;
  const p = parsePayload(e.payload_json);
  const initial = payloadText(p, keys) ?? "";
  const { text, loading } = useContextFullText(initial, e.id, keys);
  const isHook = typeof p?.["hook_event"] === "string" || typeof p?.["hook_name"] === "string";
  return (
    <div className={`ctx-row-wrap ${open ? "open" : ""}`}>
      <button className="ctx-row ctx-block" onClick={() => setOpen((o) => !o)}>
        <span className="ctx-icon">🧩</span>
        <span className="ctx-label">{isHook ? t("钩子注入") : t("系统注入")}</span>
        {e.summary && (
          <span className="ctx-summary mono" title={e.summary}>
            {e.summary.length > 80 ? `${e.summary.slice(0, 80)}…` : e.summary}
          </span>
        )}
        <span className="ctx-meta">
          {loading ? t("加载全文…") : initial ? `${text.length.toLocaleString()} ${t("字")}` : ""}
          {open ? " ▾" : " ▸"}
        </span>
      </button>
      {open && (
        <div className="ctx-detail">
          {p != null && !loading && (
            <div className="ctx-meta-line mono">
              {typeof p["attachment_type"] === "string" && `${t("类型")} ${p["attachment_type"]}`}
              {typeof p["bytes"] === "number" && ` · ${t("源附件")} ${(p["bytes"] as number).toLocaleString()} B`}
            </div>
          )}
          {!loading && text && <ContextBody text={text} />}
          {!loading && text && <CopyButton text={text} />}
        </div>
      )}
    </div>
  );
}

/** 通用事件详情里的结构化输出块：命令 + 完整输出（替代裸 JSON payload 展示）。 */
export function StructuredOutput({ e }: { e: EventDto }) {
  const p = parsePayload(e.payload_json);
  const output = payloadText(p, OUTPUT_KEYS);
  const cmd = payloadText(p, ["cmd", "command"]);
  const inputJson = p && p["input"] != null ? JSON.stringify(p["input"], null, 2) : null;
  if (!output && !cmd && !inputJson) return null;
  return (
    <div className="ctx-structured">
      {cmd && (
        <div className="ctx-cmd-wrap">
          <div className="ctx-cmd-label">{t("命令")}</div>
          <pre className="ctx-body ctx-cmd mono">{cmd}</pre>
          <CopyButton text={cmd} />
        </div>
      )}
      {inputJson && !cmd && (
        <details className="ctx-input-details">
          <summary className="ctx-cmd-label">{t("工具入参")}</summary>
          <pre className="ctx-body mono">{inputJson}</pre>
        </details>
      )}
      {output && <StructuredOutputBody e={e} output={output} />}
    </div>
  );
}

function StructuredOutputBody({ e, output }: { e: EventDto; output: string }) {
  const keys = OUTPUT_KEYS;
  const { text, truncated, loading } = useContextFullText(output, e.id, keys);
  return (
    <div className="ctx-cmd-wrap">
      <div className="ctx-cmd-label">
        {t("完整输出")} {truncated && !loading ? t("（详情已截断，展开加载全文）") : ""}
      </div>
      {loading ? (
        <div className="ctx-meta">{t("加载全文…")}</div>
      ) : (
        <>
          <pre className="ctx-body mono">{text}</pre>
          <CopyButton text={text} />
        </>
      )}
    </div>
  );
}
