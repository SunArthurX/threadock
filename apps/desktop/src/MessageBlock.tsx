// 单条消息渲染（高亮 + 复制 + 折叠 + 代码块）。
// 提取为 React.memo 子组件：避免搜索/高亮 props 变化时整列重渲。
import { memo, useMemo, type ReactNode } from "react";
import type { Message } from "./types";
import { COLLAPSE_THRESHOLD } from "./types";
import { splitCodeBlocks } from "./messageRender";
import { highlightCode } from "./codeHighlight.tsx";
import MessageImages from "./MessageImages";

export interface MessageBlockProps {
  message: Message;
  isMatch: boolean;
  searchQuery: string;
  isCollapsed: boolean;
  onToggleCollapse: (id: string) => void;
  onCopyMessage: (text: string) => void;
  onCopyMsgId: (id: string) => void;
}

function highlightSegments(text: string, lower: string, keyPrefix: string): ReactNode {
  if (!lower) return text;
  const out: ReactNode[] = [];
  const lowerS = text.toLowerCase();
  let i = 0;
  while (i < text.length) {
    const hit = lowerS.indexOf(lower, i);
    if (hit < 0) { out.push(text.slice(i)); break; }
    if (hit > i) out.push(text.slice(i, hit));
    out.push(<mark key={`${keyPrefix}-${hit}`} className="msg-search-hit">{text.slice(hit, hit + lower.length)}</mark>);
    i = hit + lower.length;
  }
  return out.length === 1 && typeof out[0] === "string" ? out[0] : <>{out}</>;
}

interface HighlightedSeg {
  key: string;
  kind: "code" | "text";
  node: ReactNode;
  lang: string;
  content: string;
}

function MessageBlockImpl({
  message, isMatch, searchQuery, isCollapsed, onToggleCollapse, onCopyMessage, onCopyMsgId,
}: MessageBlockProps) {
  const text = message.content_text ?? "(空)";
  const isLong = text.length > COLLAPSE_THRESHOLD;
  // 拆分代码块：缓存到组件内（同一 m.id + 文本不重算）
  const segs = useMemo(() => splitCodeBlocks(text), [text]);
  // 预高亮：每个段的 React 节点按 m.id 缓存（含 lang/content 用于复制按钮）
  const highlighted = useMemo<HighlightedSeg[]>(() => {
    const lower = searchQuery.trim().toLowerCase();
    return segs.map((seg, si) => {
      if (seg.kind === "code") {
        return {
          kind: "code" as const,
          key: `cb-${si}`,
          node: highlightCode(seg.content, seg.lang),
          lang: seg.lang,
          content: seg.content,
        };
      }
      return {
        kind: "text" as const,
        key: `tx-${si}`,
        node: highlightSegments(seg.content, lower, `tx-${si}`),
        lang: "",
        content: seg.content,
      };
    });
  }, [segs, searchQuery]);
  return (
    <>
      <div className="content">
        {highlighted.map((h) =>
          h.kind === "code" ? (
            <div key={h.key} className="msg-code-block">
              <div className="msg-code-head">
                <span className="msg-code-lang">{h.lang || "text"}</span>
                <button
                  className="msg-action-btn"
                  onClick={() => onCopyMessage(h.content)}
                  title="复制代码块"
                >📋</button>
              </div>
              <pre className="msg-code-pre"><code>{h.node}</code></pre>
            </div>
          ) : (
            <span key={h.key}>{h.node}</span>
          ),
        )}
      </div>
      {/* 本机图片内联：消息引用的截图等仍在本机原位置时直接展示 */}
      <MessageImages text={text} />
      <div className="msg-actions">
        {isLong && (
          <button className="msg-action-btn" onClick={() => onToggleCollapse(message.id)}>
            {isCollapsed ? `展开剩余 ${text.length - COLLAPSE_THRESHOLD} 字 ▾` : "收起 ▴"}
          </button>
        )}
        <button className="msg-action-btn" onClick={() => onCopyMessage(text)} title="复制本条消息">📋</button>
        <button className="msg-action-btn" onClick={() => onCopyMsgId(message.id)} title="复制 message_id（排错）">🆔</button>
      </div>
      {isMatch && <div className="msg-match-marker" aria-hidden>🎯</div>}
    </>
  );
}

export default memo(MessageBlockImpl);
