// 第 8 轮测试：私人笔记 UI + 状态栏 + splitCodeBlocks
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: any) => {
    if (cmd === "get_conversation_note") return args?.id === "c-with-note" ? { note: "已存笔记", updated_at: Date.now() } : null;
    if (cmd === "set_conversation_note") return Date.now();
    return null;
  }),
}));

import ConversationDetail from "../ConversationDetail";
import { splitCodeBlocks } from "../messageRender";

describe("splitCodeBlocks 消息代码块切分", () => {
  it("无代码块 → 整段 text", () => {
    const r = splitCodeBlocks("hello world");
    expect(r.length).toBe(1);
    expect(r[0]).toEqual({ kind: "text", lang: "", content: "hello world" });
  });

  it("单个代码块带语言标签", () => {
    const r = splitCodeBlocks("before\n```python\nprint(1)\n```\nafter");
    expect(r.length).toBe(3);
    expect(r[0].kind).toBe("text");
    expect(r[0].content).toBe("before\n");
    expect(r[1].kind).toBe("code");
    expect(r[1].lang).toBe("python");
    expect(r[1].content).toBe("print(1)");
    expect(r[2].kind).toBe("text");
    expect(r[2].content).toBe("\nafter");
  });

  it("无语言标签默认空字符串", () => {
    const r = splitCodeBlocks("```\nplain\n```");
    expect(r.length).toBe(1);
    expect(r[0].lang).toBe("");
  });

  it("多块代码", () => {
    const r = splitCodeBlocks("```ts\nA\n```\nmiddle\n```rs\nB\n```");
    const codeSegs = r.filter((s) => s.kind === "code");
    expect(codeSegs.length).toBe(2);
    expect(codeSegs[0].content).toBe("A");
    expect(codeSegs[1].content).toBe("B");
  });
});

describe("ConversationDetail 私人笔记", () => {
  const baseConv = {
    id: "c-with-note", provider: "zcode" as any, source_conversation_id: "sc", title: "T", user_title: null,
    status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null,
    started_at_ms: null, updated_at_ms: null, child_count: 0, favorite: false, archived: false,
  };

  it("无 note 时折叠但点击展开后显示空 textarea", async () => {
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
        note="" onNoteChange={() => {}}
      />,
    );
    expect(container.querySelector(".private-note")).toBeInTheDocument();
  });

  it("有 note 时 details 默认展开且显示内容", async () => {
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
        note="我的笔记内容"
        onNoteChange={() => {}}
      />,
    );
    const ta = container.querySelector(".private-note-text") as HTMLTextAreaElement;
    expect(ta).toBeTruthy();
    expect(ta.value).toBe("我的笔记内容");
    // details 默认 open
    const det = container.querySelector(".private-note") as HTMLDetailsElement;
    expect(det.open).toBe(true);
  });

  it("失焦触发 onNoteChange（trim 后）", () => {
    const onNote = vi.fn();
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
        note="" onNoteChange={onNote}
      />,
    );
    const ta = container.querySelector(".private-note-text") as HTMLTextAreaElement;
    fireEvent.change(ta, { target: { value: "  新笔记  " } });
    fireEvent.blur(ta);
    expect(onNote).toHaveBeenCalledWith("新笔记");
  });

  it("清空内容失焦 → onNoteChange(null)（删除）", () => {
    const onNote = vi.fn();
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
        note="旧笔记" onNoteChange={onNote}
      />,
    );
    const ta = container.querySelector(".private-note-text") as HTMLTextAreaElement;
    fireEvent.change(ta, { target: { value: "   " } }); // 空白
    fireEvent.blur(ta);
    expect(onNote).toHaveBeenCalledWith(null);
  });
});

describe("消息内代码块渲染", () => {
  const baseConv = {
    id: "c1", provider: "zcode" as any, source_conversation_id: "sc", title: "T", user_title: null,
    status: null, model: null, completeness_score: null, workspace_id: null, source_parent_id: null,
    started_at_ms: null, updated_at_ms: null, child_count: 0, favorite: false, archived: false,
  };
  const sampleMsg = {
    id: "m1", role: "assistant" as const, content_text: 'Here is code:\n```python\ndef hello():\n    print("hi")\n```\nDone.', sequence_number: 0, created_at_ms: 0,
  };

  it("渲染 ``` 块为 .msg-code-block + 语言标签", () => {
    const { container } = render(
      <ConversationDetail
        conv={baseConv} messages={[sampleMsg]} events={[]} completenessLabel="" loading={false} exporting={false}
        timelineMode={false} highlightMsgId={null} collapsedMsgs={new Set()} tags={[]}
        onToggleTimeline={() => {}} onExport={() => {}} onExtractKnowledge={() => {}}
        onToggleCollapse={() => {}}
        onAddTag={() => {}} onRemoveTag={() => {}} onRescanAudit={() => {}}
      />,
    );
    expect(container.querySelector(".msg-code-block")).toBeInTheDocument();
    expect(container.querySelector(".msg-code-lang")?.textContent).toBe("python");
    expect(container.querySelector(".msg-code-pre code")?.textContent).toContain('def hello()');
  });
});
