// 列表分页 hook：超过 pageSize 自动分页（安全/资产/知识库/项目等长列表用）
import { useEffect, useState } from "react";

export function usePager<T>(items: T[], pageSize = 20) {
  const [page, setPage] = useState(0);
  const totalPages = Math.max(1, Math.ceil(items.length / pageSize));
  const safePage = Math.min(page, totalPages - 1);
  const slice = items.slice(safePage * pageSize, (safePage + 1) * pageSize);
  // 数据长度缩短时（如搜索后清空），safePage 可能大于 totalPages-1，
  // 强制把 page 状态重置到合法范围，避免出现「翻到第 5 页但只有 2 页」的脏状态。
  // （render 期间回正 state 会触发 set-state-in-render，此处只能在 effect 中纠正）
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 越界页码回正（防御性状态修复）
    if (page > totalPages - 1) setPage(0);
  }, [page, totalPages]);
  return {
    slice,
    page: safePage,
    totalPages,
    /** 是否需要分页（不足一页时 UI 不渲染翻页器）。 */
    needed: items.length > pageSize,
    total: items.length,
    prev: () => setPage((p) => Math.max(0, p - 1)),
    next: () => setPage((p) => Math.min(totalPages - 1, p + 1)),
    /** 显式重置到首页（外部过滤/搜索时用）。 */
    reset: () => setPage(0),
  };
}
