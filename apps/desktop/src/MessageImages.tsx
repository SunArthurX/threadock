// 消息内联本机图片：消息引用的截图等若仍在本机原位置则直接展示；
// 已移动/删除显示灰色占位（告知用户为什么没图），加载错误显示原因。
// 带模块级缓存：同一路径只读一次（切换会话/滚动不重复 IO 与 base64）。
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { extractLocalImagePaths } from "./localImages";

interface ImageData {
  mime: string;
  data_url: string;
}

/** path → Promise<数据 | null(不存在) | { error }；模块级，跨组件共享。 */
const cache = new Map<string, Promise<ImageData | null | { error: string }>>();

function load(path: string): Promise<ImageData | null | { error: string }> {
  let entry = cache.get(path);
  if (!entry) {
    if (cache.size > 80) cache.clear(); // 粗粒度上限：超 80 项整体重置（防长会话内存增长）
    entry = invoke<ImageData | null>("read_image_file", { path }).catch((e) => ({
      error: typeof e === "string" ? e : String(e),
    }));
    cache.set(path, entry);
  }
  return entry;
}

/** 清空图片缓存（测试隔离用；生产路径无需调用）。 */
export function clearImageCache() {
  cache.clear();
}

function InlineImage({ path }: { path: string }) {
  const [state, setState] = useState<ImageData | null | { error: string } | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    load(path).then((r) => {
      if (alive) setState(r);
    });
    return () => {
      alive = false;
    };
  }, [path]);

  const short = path.length > 56 ? `…${path.slice(-54)}` : path;
  if (state === undefined) {
    return <div className="msg-image msg-image-loading" title={path} aria-label="图片加载中" />;
  }
  if (state === null) {
    return (
      <div className="msg-image-missing" title={path}>
        🖼 图片已不在原位置 · <span className="mono">{short}</span>
      </div>
    );
  }
  if ("error" in state) {
    return (
      <div className="msg-image-missing" title={path}>
        ⚠ {state.error} · <span className="mono">{short}</span>
      </div>
    );
  }
  return <img className="msg-image" src={state.data_url} alt={short} title={path} loading="lazy" />;
}

/** 消息文本 → 内联图片区（无候选时返回 null，零开销）。 */
export default function MessageImages({ text }: { text: string }) {
  const refs = useMemo(() => extractLocalImagePaths(text), [text]);
  if (refs.length === 0) return null;
  return (
    <div className="msg-images">
      {refs.map((r) => (
        <InlineImage key={r.path} path={r.path} />
      ))}
    </div>
  );
}
