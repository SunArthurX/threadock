// 项目中心页（持续优化）：可点击跳转/排序升降序/空状态/卡片 hover/批量导出
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTokens, formatCost } from "./charts";
import { formatTime } from "./types";
import { usePager } from "./usePager";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";
import type { Conversation } from "./types";

export interface ProjectRow {
  dir: string;
  sessions: number;
  tokens: number;
  cost_usd: number;
  requests: number;
  last_active_ms: number | null;
  main_agent: string | null;
}

type SortKey = "cost" | "tokens" | "active" | "sessions";
type SortDir = "desc" | "asc";

export const SORT_LABELS: Record<SortKey, string> = {
  cost: "成本",
  tokens: "Tokens",
  active: "最近活跃",
  sessions: "会话数",
};

export function sortProjects(projects: ProjectRow[], key: SortKey, dir: SortDir = "desc"): ProjectRow[] {
  const arr = [...projects];
  const sign = dir === "asc" ? 1 : -1;
  switch (key) {
    case "cost":
      return arr.sort((a, b) => (a.cost_usd - b.cost_usd) * sign);
    case "tokens":
      return arr.sort((a, b) => (a.tokens - b.tokens) * sign);
    case "active":
      return arr.sort((a, b) => ((a.last_active_ms ?? 0) - (b.last_active_ms ?? 0)) * sign);
    case "sessions":
      return arr.sort((a, b) => (a.sessions - b.sessions) * sign);
  }
}

/** 项目列表导出为 CSV（Excel 友好 UTF-8 BOM）。 */
export function projectsToCsv(projects: ProjectRow[]): string {
  const head = "目录,会话数,请求数,Tokens,成本USD,主力Agent,最近活跃(ms)";
  const escape = (v: unknown) => {
    const s = String(v ?? "");
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  const rows = projects.map((p) => [
    p.dir, p.sessions, p.requests, p.tokens, p.cost_usd.toFixed(4),
    p.main_agent ?? "", p.last_active_ms ?? "",
  ].map(escape).join(","));
  return "\uFEFF" + [head, ...rows].join("\n");
}

/**
 * @param onJumpToConversation 跳转到指定会话（由 App.tsx 注入）
 * @param onJumpToChat 跳转到 chat 视图并按 source_dir 过滤（由 App.tsx 注入）。
 *                     用于项目卡会话列表的"查看全部"链接。
 *                     P1-A5: 旧版静默截断 10 条 → 现在可点击跳到 chat 全量。
 *                     Cluster 1 (App.tsx) 需在 ProjectsView 调用处补充此 prop 的实现。
 */
export default function ProjectsView({
  onJumpToConversation,
  onJumpToChat,
}: {
  onJumpToConversation?: (cid: string) => void;
  onJumpToChat?: (dir: string) => void;
} = {}) {
  const [projects, setProjects] = useState<ProjectRow[] | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("cost");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [search, setSearch] = useState("");
  // 项目卡 → 会话列表展开
  const [openDir, setOpenDir] = useState<string | null>(null);
  const [dirConvs, setDirConvs] = useState<Conversation[] | null>(null);
  const [dirLoading, setDirLoading] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const r = await invoke<{ projects: ProjectRow[] }>("projects_overview", {});
        setProjects(r.projects);
      } catch { /* 空库静默 */ }
    })();
  }, []);

  const shortDir = (d: string) => d.split("/").filter(Boolean).slice(-2).join("/") || d;

  // 搜索过滤 + 排序（usePager 在其后切片）
  const processed = useMemo(() => {
    if (!projects) return [];
    const q = search.trim().toLowerCase();
    const filtered = q
      ? projects.filter((p) => p.dir.toLowerCase().includes(q) || (p.main_agent ?? "").toLowerCase().includes(q))
      : projects;
    return sortProjects(filtered, sortKey, sortDir);
  }, [projects, search, sortKey, sortDir]);

  const pager = usePager(processed, 20);
  // 搜索/排序变化时回到首页：在事件处理器中重置（替代 effect 内 setState）

  const totals = useMemo(() => {
    const all = projects ?? [];
    return {
      count: all.length,
      tokens: all.reduce((a, b) => a + b.tokens, 0),
      cost: all.reduce((a, b) => a + b.cost_usd, 0),
      sessions: all.reduce((a, b) => a + b.sessions, 0),
    };
  }, [projects]);
  const maxCost = Math.max(...(projects ?? []).map((p) => p.cost_usd), 0.0001);
  const isEmpty = projects !== null && projects.length === 0;

  /** 排序键：点击同一键切换升降序（同时回到第 1 页）。 */
  const clickSort = (k: SortKey) => {
    if (sortKey === k) setSortDir(sortDir === "desc" ? "asc" : "desc");
    else { setSortKey(k); setSortDir("desc"); }
    pager.reset();
  };

  /** 导出当前过滤后的列表为 CSV。 */
  const exportCsv = async () => {
    try {
      await navigator.clipboard.writeText(projectsToCsv(processed));
      showToast(`✓ 已复制 ${processed.length} 个项目 CSV 到剪贴板`, "info");
    } catch {
      showToast("剪贴板不可用", "error");
    }
  };

  /** 展开/收起某个项目卡的会话列表。 */
  const toggleDir = async (dir: string) => {
    if (openDir === dir) { setOpenDir(null); setDirConvs(null); return; }
    setOpenDir(dir);
    setDirLoading(true);
    try {
      const list = await invoke<Conversation[]>("list_conversations_by_dir", { dir });
      setDirConvs(list);
    } catch (e) {
      showToast(`查询失败：${String(e)}`, "error");
      setDirConvs(null);
    } finally {
      setDirLoading(false);
    }
  };

  return (
    <ScrollArea className="projects-page">
      <div className="ops-card">
        <div className="ops-card-title">
          📁 项目中心
          <span className="ops-card-sub">
            {projects ? `${totals.count} 个项目 · ${totals.sessions} 会话 · ${formatTokens(totals.tokens)} · ${formatCost(totals.cost)}` : "加载中…"}
          </span>
          {projects && projects.length > 0 && (
            <button className="action-btn" style={{ marginLeft: "auto", fontSize: 11 }} onClick={exportCsv}
              title="把当前过滤+排序后的项目列表复制为 CSV（Excel 友好 UTF-8 BOM）">
              ⧉ 导出 CSV
            </button>
          )}
        </div>
        <div className="scope-bar" style={{ alignItems: "center" }}>
          <span style={{ fontSize: 11.5, opacity: 0.6 }}>排序</span>
          {(Object.keys(SORT_LABELS) as SortKey[]).map((k) => {
            const active = sortKey === k;
            return (
              <button
                key={k}
                className={`scope-chip ${active ? "active" : ""}`}
                onClick={() => clickSort(k)}
                title={active ? `再次点击切换为${sortDir === "desc" ? "升序" : "降序"}` : "点击按此字段排序"}
              >
                {SORT_LABELS[k]}{active ? (sortDir === "desc" ? " ↓" : " ↑") : ""}
              </button>
            );
          })}
          <input
            className="settings-confirm-input"
            style={{ marginLeft: "auto", width: 180, fontSize: 12 }}
            placeholder="🔍 搜索项目 / Agent…"
            value={search}
            onChange={(e) => { setSearch(e.target.value); pager.reset(); }}
          />
        </div>
        {isEmpty && (
          <div className="ops-table-empty">
            📂 暂无项目用量数据 — 导入会话并同步指标后，按 source_dir 自动归并为项目卡片
          </div>
        )}
        {projects && projects.length > 0 && processed.length === 0 && (
          <div className="ops-table-empty">🔍 无匹配项目（试试清空搜索或换个关键词）</div>
        )}
      </div>
      <div className="project-grid">
        {pager.slice.map((p) => (
          <div
            key={p.dir}
            className={`project-card ${openDir === p.dir ? "expanded" : ""}`}
            title={`点击展开/收起会话列表 · ${p.dir}`}
            onClick={() => toggleDir(p.dir)}
          >
            <div className="project-name mono" title={p.dir}>
              {openDir === p.dir ? "▼" : "▶"} {shortDir(p.dir)}
            </div>
            <div className="cost-ratio" title={`成本占比 ${((p.cost_usd / maxCost) * 100).toFixed(1)}%（相对最大项目）`}>
              <div className="cost-ratio-fill" style={{ width: `${Math.max(3, (p.cost_usd / maxCost) * 100)}%` }} />
            </div>
            <div className="project-rows">
              <div className="project-row"><span>会话</span><b>{p.sessions}</b></div>
              <div className="project-row"><span>请求</span><b>{p.requests.toLocaleString()}</b></div>
              <div className="project-row"><span>Tokens</span><b>{formatTokens(p.tokens)}</b></div>
              <div className="project-row"><span>成本</span><b>{formatCost(p.cost_usd)}</b></div>
              <div className="project-row"><span>主力 Agent</span><b>{p.main_agent ?? "—"}</b></div>
              <div className="project-row"><span>最近活跃</span><b>{formatTime(p.last_active_ms) || "—"}</b></div>
            </div>
            {openDir === p.dir && (
              <div className="project-conv-list" onClick={(e) => e.stopPropagation()}>
                <div className="project-conv-title">
                  {dirLoading ? "加载中…" : dirConvs && `${dirConvs.length} 条会话`}
                </div>
                {dirConvs && dirConvs.length === 0 && (
                  <div className="project-conv-empty">该项目下没有主任务会话</div>
                )}
                {dirConvs && dirConvs.slice(0, 10).map((c) => (
                  <div
                    key={c.id}
                    className="project-conv-row"
                    onClick={() => onJumpToConversation?.(c.id)}
                  >
                    <span className={`badge source ${c.provider}`}>{c.provider}</span>
                    <span className="project-conv-name">{c.user_title ?? c.title ?? "(无标题)"}</span>
                    <span className="project-conv-time">{formatTime(c.started_at_ms ?? null)}</span>
                  </div>
                ))}
                {dirConvs && dirConvs.length > 10 && (
                  <div
                    className="project-conv-more-link"
                    role="button"
                    tabIndex={0}
                    onClick={(e) => {
                      e.stopPropagation();
                      // P1-A5: 把截断提示变成可点击跳转：调用 onJumpToChat（App.tsx 注入）
                      // 跳到 chat 视图并按 source_dir 过滤；无回调时退化为不可点的提示文本。
                      if (onJumpToChat) onJumpToChat(p.dir);
                      else showToast(`请在 App.tsx 注入 onJumpToChat 回调以启用「查看全部 ${dirConvs.length} 条」`, "info", 5000);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        e.stopPropagation();
                        if (onJumpToChat) onJumpToChat(p.dir);
                      }
                    }}
                    title="跳转到 chat 视图并按此项目目录过滤"
                  >
                    在 Chat 中查看全部 {dirConvs.length} 条 →
                  </div>
                )}
              </div>
            )}
          </div>
        ))}
      </div>
      {pager.needed && (
        <div className="pager" style={{ justifyContent: "center" }}>
          <button className="pager-btn" onClick={pager.prev} disabled={pager.page === 0}>‹ 上一页</button>
          <span className="pager-info">{pager.page + 1} / {pager.totalPages} 页 · 共 {pager.total} 个项目</span>
          <button className="pager-btn" onClick={pager.next} disabled={pager.page >= pager.totalPages - 1}>下一页 ›</button>
        </div>
      )}
    </ScrollArea>
  );
}
