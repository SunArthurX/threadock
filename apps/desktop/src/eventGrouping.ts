// 事件 → 消息 的归属分组（独立可测纯函数）。
//
// 规则：消息与事件共享同一会话的 sequence_number 顺序流；每个事件归属
// 「序号 ≤ 事件序号的最大消息」（事件通常发生在某条消息处理期间/之后）。
// 早于首条消息的事件（导入边界、会话前置上下文）进 orphan 组，由调用方
// 在消息列表顶部单独渲染。
import type { Message, EventDto } from "./types";

export interface EventGroups {
  /** message_id → 该消息名下的事件（按序号升序）。 */
  byMessageId: Map<string, EventDto[]>;
  /** 早于首条消息的事件（seq 小于所有消息）。 */
  orphan: EventDto[];
}

/** 紧凑行默认最多显示条数（其余折叠到「还有 N 条」）。 */
export const EVENT_ROWS_COLLAPSED = 4;

export function groupEventsByMessage(messages: Message[], events: EventDto[]): EventGroups {
  const groups: EventGroups = { byMessageId: new Map(), orphan: [] };
  if (events.length === 0) return groups;

  // 消息按 sequence_number 升序（与后端 ORDER BY 一致；这里防御性再排一次）
  const sortedMsgs = [...messages].sort((a, b) => a.sequence_number - b.sequence_number);
  if (sortedMsgs.length === 0) {
    return { byMessageId: new Map(), orphan: [...events].sort((a, b) => a.sequence_number - b.sequence_number) };
  }

  let ownerIdx = -1; // 当前归属消息下标（-1 = 尚未有消息，orphan）
  let ei = 0;
  const sortedEvents = [...events].sort((a, b) => a.sequence_number - b.sequence_number);
  for (const msg of sortedMsgs) {
    // 把 seq < 下一条消息 seq 的事件都归给当前消息（严格小于：与消息同序号的事件归属该消息）
    while (ei < sortedEvents.length && sortedEvents[ei].sequence_number < msg.sequence_number) {
      const ev = sortedEvents[ei];
      if (ownerIdx < 0) groups.orphan.push(ev);
      else {
        const owner = sortedMsgs[ownerIdx];
        const list = groups.byMessageId.get(owner.id) ?? [];
        list.push(ev);
        groups.byMessageId.set(owner.id, list);
      }
      ei++;
    }
    ownerIdx += 1;
  }
  // 尾部：剩余事件全部归最后一条消息
  const last = sortedMsgs[sortedMsgs.length - 1];
  while (ei < sortedEvents.length) {
    const list = groups.byMessageId.get(last.id) ?? [];
    list.push(sortedEvents[ei]);
    groups.byMessageId.set(last.id, list);
    ei++;
  }
  return groups;
}
