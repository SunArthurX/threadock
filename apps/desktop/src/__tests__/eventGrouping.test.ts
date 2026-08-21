// 事件 → 消息归属分组纯函数矩阵（按时间戳归属；序号流独立不可比）。
import { describe, expect, it } from "vitest";
import { groupEventsByMessage } from "../eventGrouping";
import type { Message, EventDto } from "../types";

const msg = (id: string, at: number | null): Message => ({
  id, role: "user", content_text: null,
  // 注意：sequence_number 是独立流（messages 1..N / events 1..M），分组不看它
  sequence_number: 1, created_at_ms: at,
});
const ev = (id: string, at: number | null): EventDto => ({
  id, event_type: "tool_call_started", summary: id, sequence_number: 1,
  created_at_ms: at, status: null, completed_at_ms: null, payload_json: null,
});

describe("groupEventsByMessage 归属矩阵（时间戳）", () => {
  it("事件归属「时间 ≤ 事件时间的最后一条消息」", () => {
    const g = groupEventsByMessage(
      [msg("m1", 10), msg("m2", 20), msg("m3", 30)],
      [ev("e15", 15), ev("e25", 25), ev("e35", 35)],
    );
    expect(g.byMessageId.get("m1")?.map((e) => e.id)).toEqual(["e15"]);
    expect(g.byMessageId.get("m2")?.map((e) => e.id)).toEqual(["e25"]);
    expect(g.byMessageId.get("m3")?.map((e) => e.id)).toEqual(["e35"]);
    expect(g.orphan).toEqual([]);
  });

  it("与消息同时刻的事件归属该消息（工具调用来自该消息本身）", () => {
    const g = groupEventsByMessage(
      [msg("m1", 10), msg("m2", 20)],
      [ev("e20", 20)],
    );
    expect(g.byMessageId.get("m2")?.map((e) => e.id)).toEqual(["e20"]);
  });

  it("序号独立流不再影响归属（回归：曾按 seq 比较导致错挂）", () => {
    // 10 条消息、50 条事件：事件 seq 全是 1..50、消息 seq 1..10，
    // 真实归属只看时间——第 2 条消息后的 3 个事件（时间 21/22/23）应挂 m2
    const msgs = Array.from({ length: 10 }, (_, i) => msg(`m${i + 1}`, (i + 1) * 10));
    const evs = [ev("a", 21), ev("b", 22), ev("c", 23)];
    const g = groupEventsByMessage(msgs, evs);
    expect(g.byMessageId.get("m2")?.map((e) => e.id)).toEqual(["a", "b", "c"]);
    expect(g.byMessageId.size).toBe(1);
  });

  it("早于首条消息的事件进孤儿组", () => {
    const g = groupEventsByMessage([msg("m1", 10)], [ev("e1", 1), ev("e9", 9)]);
    expect(g.orphan.map((e) => e.id)).toEqual(["e1", "e9"]);
    expect(g.byMessageId.get("m1")).toBeUndefined();
  });

  it("无时间戳的事件归最后一条消息（旧数据近似，排在该消息名下事件末尾）", () => {
    const g = groupEventsByMessage(
      [msg("m1", 10), msg("m2", 20)],
      [ev("e-null", null), ev("e25", 25)],
    );
    expect(g.byMessageId.get("m2")?.map((e) => e.id)).toEqual(["e25", "e-null"]);
  });

  it("无消息时全部进孤儿组（按时间排序）", () => {
    const g = groupEventsByMessage([], [ev("e3", 3), ev("e1", 1)]);
    expect(g.orphan.map((e) => e.id)).toEqual(["e1", "e3"]);
  });

  it("同消息多事件按时间升序", () => {
    const g = groupEventsByMessage(
      [msg("m1", 10), msg("m2", 20)],
      [ev("e17", 17), ev("e12", 12), ev("e15", 15)],
    );
    expect(g.byMessageId.get("m1")?.map((e) => e.id)).toEqual(["e12", "e15", "e17"]);
  });

  it("空事件安全", () => {
    expect(groupEventsByMessage([msg("m1", 1)], [])).toEqual({ byMessageId: new Map(), orphan: [] });
  });
});
