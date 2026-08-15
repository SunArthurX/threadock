// 列表分页 hook：超过 pageSize 自动分页（安全/资产等长列表用）
import { useState } from "react";

export function usePager<T>(items: T[], pageSize = 20) {
  const [page, setPage] = useState(0);
  const totalPages = Math.max(1, Math.ceil(items.length / pageSize));
  const safePage = Math.min(page, totalPages - 1);
  const slice = items.slice(safePage * pageSize, (safePage + 1) * pageSize);
  return {
    slice,
    page: safePage,
    totalPages,
    /** 是否需要分页（不足一页时 UI 不渲染翻页器）。 */
    needed: items.length > pageSize,
    total: items.length,
    prev: () => setPage((p) => Math.max(0, p - 1)),
    next: () => setPage((p) => Math.min(totalPages - 1, p + 1)),
  };
}
