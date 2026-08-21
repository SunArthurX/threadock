// 事件 → 消息归属分组纯函数矩阵。
import { describe, expect, it } from "vitest";
import { groupEventsByMessage } from "../eventGrouping";
import type { Message, EventDto } from "../types";

const msg = (id: string, seq: number): Message => ({
  id, role: "user", content_text: null, sequence_number: seq, created_at_ms: seq,
});
const ev = (id: string, seq: number): EventDto => ({
  id, event_type: "tool_call_started", summary: id, sequence_number: seq, created_at_ms: seq,
  status: null, completed_at_ms: null, payload_json: null,
});

describe("groupEventsByMessage 归属矩阵", () => {
  it("事件归属「序号 ≤ 事件序号的最大消息」", () => {
    const g = groupEventsByMessage(
      [msg("m1", 10), msg("m2", 20), msg("m3", 30)],
      [ev("e15", 15), ev("e25", 25), ev("e35", 35)],
    );
    expect(g.byMessageId.get("m1")?.map((e) => e.id)).toEqual(["e15"]);
    expect(g.byMessageId.get("m2")?.map((e) => e.id)).toEqual(["e25"]);
    expect(g.byMessageId.get("m3")?.map((e) => e.id)).toEqual(["e35"]);
    expect(g.orphan).toEqual([]);
  });

  it("与消息同序号的事件归属该消息", () => {
    const g = groupEventsByMessage([msg("m1", 10), msg("m2", 20)], [ev("e10", 10)]);
    expect(g.byMessageId.get("m1")?.map((e) => e.id)).toEqual(["e10"]);
  });

  it("早于首条消息的事件进孤儿组", () => {
    const g = groupEventsByMessage([msg("m1", 10)], [ev("e1", 1), ev("e2", 5), ev("e9", 9)]);
    expect(g.orphan.map((e) => e.id)).toEqual(["e1", "e2", "e9"]);
    expect(g.byMessageId.get("m1")).toBeUndefined();
  });

  it("无消息时全部进孤儿组（按序号排序）", () => {
    const g = groupEventsByMessage([], [ev("e3", 3), ev("e1", 1)]);
    expect(g.orphan.map((e) => e.id)).toEqual(["e1", "e3"]);
  });

  it("多条事件按序号升序归同一消息", () => {
    const g = groupEventsByMessage(
      [msg("m1", 10), msg("m2", 20)],
      [ev("e17", 17), ev("e12", 12), ev("e15", 15)],
    );
    expect(g.byMessageId.get("m1")?.map((e) => e.id)).toEqual(["e12", "e15", "e17"]);
  });

  it("空事件 / 空消息安全", () => {
    expect(groupEventsByMessage([msg("m1", 1)], [])).toEqual({ byMessageId: new Map(), orphan: [] });
  });
});
