// 切分消息文本为「代码块 + 普通文本」段（独立可测）。
//  - 匹配 ```lang\n...\n``` 三反引号块
//  - 匹配行内 `code` 单反引号块（暂未启用，避免破坏搜索高亮）
export interface ContentSeg { kind: "code" | "text"; lang: string; content: string }

export function splitCodeBlocks(text: string): ContentSeg[] {
  const segs: ContentSeg[] = [];
  const re = /```([a-zA-Z0-9_+-]*)\n([\s\S]*?)\n```/g;
  let last = 0; let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) segs.push({ kind: "text", lang: "", content: text.slice(last, m.index) });
    segs.push({ kind: "code", lang: m[1] || "", content: m[2] });
    last = m.index + m[0].length;
  }
  if (last < text.length) segs.push({ kind: "text", lang: "", content: text.slice(last) });
  return segs;
}
