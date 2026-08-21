// 从消息文本提取「本机图片路径」候选（独立可测的纯函数）。
//
// 识别三种形态（AI 会话里常见的本机截图引用）：
//  1. Markdown 图片语法：![alt](/abs/path.png)，支持 <尖括号> 目标
//  2. 裸绝对路径：/Users/...、C:\Users\...（含盘符），扩展名为图片
//  3. file:// URL：file:///Users/.../a.png
//
// 不处理：http(s)://（远程图，不属于「本机原位置」语义）、相对路径
//（无法确定基准目录，误报率高）。
export interface LocalImageRef {
  path: string;
  /** 原始匹配形态（诊断/测试用）。 */
  source: "markdown" | "path" | "file-url";
}

/** 每条消息最多内联的图片数（防超长消息带几十个路径撑爆内存）。 */
export const MAX_IMAGES_PER_MESSAGE = 6;

const IMG_EXT = /\.(?:png|jpe?g|gif|webp|bmp|svg|ico)$/i;

/** 去掉 file:// 前缀与 markdown 尖括号，返回本机绝对路径；非本机返回 null。 */
function normalizeToAbsPath(raw: string): string | null {
  let p = raw.trim();
  if (p.startsWith("<") && p.endsWith(">")) p = p.slice(1, -1);
  if (/^https?:\/\//i.test(p)) return null; // 远程图不处理
  if (p.startsWith("file://")) {
    p = p.slice("file://".length);
    // file:///Users/x → /Users/x；file://localhost/x → /x
    if (/^localhost\//i.test(p)) p = p.slice("localhost".length);
    p = decodeURIComponent(p);
  }
  if (!IMG_EXT.test(p)) return null;
  // 本机绝对路径：unix 以 / 开头，windows 以盘符开头
  if (p.startsWith("/")) return p;
  if (/^[A-Za-z]:[\\/]/.test(p)) return p.replace(/\//g, "\\");
  return null; // 相对路径无法定位
}

/** 提取消息文本中的本机图片路径（按出现顺序去重，最多 limit 个）。 */
export function extractLocalImagePaths(
  text: string,
  limit: number = MAX_IMAGES_PER_MESSAGE,
): LocalImageRef[] {
  const out: LocalImageRef[] = [];
  const seen = new Set<string>();
  const push = (raw: string, source: LocalImageRef["source"]) => {
    const path = normalizeToAbsPath(raw);
    if (!path || seen.has(path)) return;
    seen.add(path);
    out.push({ path, source });
  };

  // 1. Markdown 图片语法 ![alt](target)
  const md = /!\[[^\]]*\]\(\s*(<[^>]+>|[^)\s]+)\s*(?:"[^"]*")?\)/g;
  // 2. 裸绝对路径（unix：以 / 开头，不含空白与常见分隔符）
  const unix = /(?:^|[\s'"`(（【[{<])(\/[^\s'"`）\]（）<>|;,\u3000]+\.(?:png|jpe?g|gif|webp|bmp|svg|ico))/gi;
  // 3. Windows 盘符路径（反斜杠或正斜杠写法）
  const win = /\b([A-Za-z]:[\\/][^\s"'`）\]<>|;,\u3000]+\.(?:png|jpe?g|gif|webp|bmp|svg|ico))/gi;
  // 4. file:// URL
  const fileUrl = /file:\/\/[^\s"'`）\]<>|]+/gi;

  let m: RegExpExecArray | null;
  while ((m = md.exec(text)) !== null) push(m[1], "markdown");
  while ((m = unix.exec(text)) !== null) push(m[1], "path");
  while ((m = win.exec(text)) !== null) push(m[1], "path");
  while ((m = fileUrl.exec(text)) !== null) push(m[0], "file-url");

  return out.slice(0, limit);
}
