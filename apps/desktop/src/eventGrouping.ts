// 事件 → 消息 的归属分组（独立可测纯函数）。
//
// 归属依据（2026-08-21 修正）：标准化层给消息与事件**各自独立编号**
//（messages 1..N、events 1..M），序号不可跨流比较——按时间戳归属：
// 每个事件归属「时间 ≤ 事件时间的最后一条消息」（事件发生在某条消息
// 处理期间/之后；与消息同时刻的工具调用归属该消息本身）。
//
// 无时间戳的事件（旧数据/缺失）归给最后一条消息（工具活动通常紧随其后）；
// 早于首条消息的事件进 orphan 组，由调用方在消息列表顶部单独渲染。
import type { Message, EventDto } from "./types";

export interface EventGroups {
  /** message_id → 该消息名下的事件（按时间升序）。 */
  byMessageId: Map<string, EventDto[]>;
  /** 早于首条消息的事件。 */
  orphan: EventDto[];
}

/** 紧凑行默认最多显示条数（其余折叠到「还有 N 条」）。 */
export const EVENT_ROWS_COLLAPSED = 4;

export function groupEventsByMessage(messages: Message[], events: EventDto[]): EventGroups {
  const groups: EventGroups = { byMessageId: new Map(), orphan: [] };
  if (events.length === 0) return groups;
  if (messages.length === 0) {
    return { byMessageId: new Map(), orphan: [...events].sort(byTime) };
  }

  // 消息按时间升序（无时间的保持原相对顺序、视为最晚 → 只兜无时间事件）
  const timed = messages
    .map((m, i) => ({ m, at: m.created_at_ms ?? null, i }))
    .sort((a, b) => (a.at ?? Number.MAX_SAFE_INTEGER) - (b.at ?? Number.MAX_SAFE_INTEGER) || a.i - b.i);

  const sorted = [...events].sort(byTime);
  for (const ev of sorted) {
    const at = ev.created_at_ms ?? null;
    let owner: { m: Message } | null = null;
    if (at == null) {
      owner = timed[timed.length - 1]; // 无时间 → 最后一条消息（近似）
    } else {
      // 最后一条 时间 ≤ 事件时间 的消息（同时刻 → 归属该消息本身）
      for (const t of timed) {
        if (t.at != null && t.at <= at) owner = t;
      }
    }
    if (owner == null) groups.orphan.push(ev);
    else {
      const list = groups.byMessageId.get(owner.m.id) ?? [];
      list.push(ev);
      groups.byMessageId.set(owner.m.id, list);
    }
  }
  return groups;
}

function byTime(a: EventDto, b: EventDto): number {
  return (a.created_at_ms ?? Number.MAX_SAFE_INTEGER) - (b.created_at_ms ?? Number.MAX_SAFE_INTEGER);
}
