// 简易代码语法高亮：基于 regex 的 token 切分，支持 5+ 种语言。
// 输出为 React 节点列表（高亮 token 用 <span class="tok-...">）。
// 故意不做精确解析（足够聊天场景代码片段），避免引 highlight.js/prism 等大依赖。
import { Fragment, type ReactNode } from "react";

export type Lang = "python" | "ts" | "tsx" | "js" | "rs" | "rust" | "go" | "sh" | "bash" | "sql" | "json" | "" | string;

const KEYWORDS: Record<string, string[]> = {
  python: ["def", "class", "import", "from", "as", "if", "elif", "else", "for", "while", "return", "yield", "with", "try", "except", "finally", "raise", "lambda", "pass", "break", "continue", "and", "or", "not", "in", "is", "None", "True", "False", "async", "await", "self"],
  ts: ["const", "let", "var", "function", "class", "interface", "type", "enum", "namespace", "import", "export", "from", "as", "if", "else", "for", "while", "do", "return", "break", "continue", "switch", "case", "default", "try", "catch", "finally", "throw", "new", "delete", "typeof", "instanceof", "in", "of", "void", "null", "undefined", "true", "false", "async", "await", "this", "super", "extends", "implements", "public", "private", "protected", "readonly", "static", "abstract"],
  // tsx / rust / sh / json 走 fallback（见 resolveKeywords），不显式列
  js: ["const", "let", "var", "function", "class", "import", "export", "from", "as", "if", "else", "for", "while", "do", "return", "break", "continue", "switch", "case", "default", "try", "catch", "finally", "throw", "new", "delete", "typeof", "instanceof", "in", "of", "void", "null", "undefined", "true", "false", "async", "await", "this", "super", "extends"],
  rs: ["fn", "let", "mut", "const", "static", "pub", "use", "mod", "struct", "enum", "trait", "impl", "for", "while", "loop", "if", "else", "match", "return", "break", "continue", "as", "in", "where", "self", "Self", "true", "false", "async", "await", "move", "ref", "Box", "Vec", "Option", "Result", "Ok", "Err", "Some", "None", "String", "i32", "u32", "i64", "u64", "usize", "bool", "f32", "f64"],
  go: ["func", "var", "const", "type", "struct", "interface", "map", "chan", "package", "import", "if", "else", "for", "range", "switch", "case", "default", "break", "continue", "return", "go", "defer", "select", "true", "false", "nil", "make", "new", "len", "cap", "append", "range", "iota"],
  bash: ["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function", "return", "exit", "echo", "export", "local", "set", "unset", "readonly", "declare", "in"],
  sql: ["select", "from", "where", "and", "or", "not", "join", "left", "right", "inner", "outer", "on", "as", "group", "by", "order", "limit", "offset", "insert", "into", "values", "update", "set", "delete", "create", "table", "index", "view", "drop", "alter", "add", "column", "primary", "key", "foreign", "references", "unique", "null", "is", "in", "exists", "between", "like", "case", "when", "then", "else", "end"],
};

/** 解析 lang 别名 → 真实关键字列表（tsx→ts, rust→rs, sh→bash）。 */
function resolveKeywords(lang: string): string[] {
  const lk = (lang || "").toLowerCase();
  if (KEYWORDS[lk]) return KEYWORDS[lk];
  if (lk === "tsx" || lk === "typescript") return KEYWORDS.ts;
  if (lk === "rust") return KEYWORDS.rs;
  if (lk === "sh" || lk === "shell" || lk === "zsh") return KEYWORDS.bash;
  if (lk === "" || lk === "text" || lk === "txt") return [];
  return KEYWORDS.ts; // 未知 lang 走 ts fallback（不抛错）
}

/** lang 是否大小写不敏感（SQL 关键字本身大写约定，但用户输入大小写都有）。 */
function caseInsensitive(lang: string): boolean {
  const lk = (lang || "").toLowerCase();
  return lk === "sql";
}

/** 高亮一行：返回 React 节点（可能嵌套）。 */
function highlightLine(line: string, lang: Lang): ReactNode[] {
  const keywords = resolveKeywords(lang);
  const ci = caseInsensitive(lang);
  const flags = ci ? "i" : "";
  const keywordRe = keywords.length > 0 ? new RegExp(`\\b(${keywords.join("|")})\\b`, flags) : null;

  // 用一个扫描器切分：先 match 字符串/注释/数字/keyword（按优先级），其他为普通文本
  const out: { kind: string; text: string }[] = [];
  let i = 0;
  while (i < line.length) {
    let matched = false;
    // 优先：注释（行尾 # 或 //）
    if (line[i] === "#" || (line[i] === "/" && line[i + 1] === "/")) {
      out.push({ kind: "comment", text: line.slice(i) });
      i = line.length;
      continue;
    }
    // 字符串
    if (line[i] === '"' || line[i] === "'") {
      const quote = line[i];
      let j = i + 1;
      while (j < line.length && line[j] !== quote) {
        if (line[j] === "\\") j++;
        j++;
      }
      const end = j < line.length ? j + 1 : j;
      out.push({ kind: "string", text: line.slice(i, end) });
      i = end;
      matched = true;
      continue;
    }
    // 数字
    const numMatch = /^\d+(\.\d+)?/.exec(line.slice(i));
    if (numMatch) {
      out.push({ kind: "number", text: numMatch[0] });
      i += numMatch[0].length;
      matched = true;
      continue;
    }
    // 关键字
    if (keywordRe) {
      const km = keywordRe.exec(line.slice(i));
      if (km && km.index === 0) {
        out.push({ kind: "keyword", text: km[0] });
        i += km[0].length;
        matched = true;
        continue;
      }
    }
    if (matched) continue;
    // 标识符
    const idMatch = /^[A-Za-z_][A-Za-z0-9_]*/.exec(line.slice(i));
    if (idMatch) {
      out.push({ kind: "ident", text: idMatch[0] });
      i += idMatch[0].length;
      continue;
    }
    // 操作符 / 标点（合并连续同种）
    if (/[+\-*/%=<>!&|^~?:]/.test(line[i])) {
      let j = i;
      while (j < line.length && /[+\-*/%=<>!&|^~?:]/.test(line[j])) j++;
      out.push({ kind: "operator", text: line.slice(i, j) });
      i = j;
      continue;
    }
    if (/[{}()[\];,.]/.test(line[i])) {
      out.push({ kind: "punct", text: line[i] });
      i++;
      continue;
    }
    // 空白 / 其他
    out.push({ kind: "text", text: line[i] });
    i++;
  }
  // 合并相邻 text 节点
  const merged: { kind: string; text: string }[] = [];
  for (const t of out) {
    const last = merged[merged.length - 1];
    if (last && last.kind === t.kind) last.text += t.text;
    else merged.push({ ...t });
  }
  return merged.map((t, i) => {
    if (t.kind === "text" || t.kind === "ident") return <Fragment key={i}>{t.text}</Fragment>;
    return <span key={i} className={`tok tok-${t.kind}`}>{t.text}</span>;
  });
}

/** 整段代码高亮。返回 React 节点。 */
export function highlightCode(code: string, lang: Lang): ReactNode {
  const lines = code.split("\n");
  return lines.map((line, i) => (
    <Fragment key={i}>
      {highlightLine(line, lang)}
      {i < lines.length - 1 && "\n"}
    </Fragment>
  ));
}

/** 简易 token 计数（用于测试 / 调试）。 */
export function countHighlightTokens(code: string, lang: Lang): { keyword: number; string: number; number: number; comment: number; total: number } {
  let keyword = 0; let string = 0; let number = 0; let comment = 0;
  const lines = code.split("\n");
  for (const line of lines) {
    const nodes = highlightLine(line, lang);
    for (const n of nodes) {
      if (typeof n === "object" && n && "props" in n) {
        const cls = (n.props as { className?: string }).className ?? "";
        if (cls.includes("tok-keyword")) keyword++;
        else if (cls.includes("tok-string")) string++;
        else if (cls.includes("tok-number")) number++;
        else if (cls.includes("tok-comment")) comment++;
      }
    }
  }
  return { keyword, string, number, comment, total: keyword + string + number + comment };
}
