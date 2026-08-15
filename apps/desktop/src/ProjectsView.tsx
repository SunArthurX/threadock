// 项目中心页（5 轮优化版）：分页/排序/搜索/汇总/成本占比条
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTokens, formatCost } from "./charts";
import { formatTime } from "./types";
import { usePager } from "./usePager";

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

export const SORT_LABELS: Record<SortKey, string> = {
  cost: "成本",
  tokens: "Tokens",
  active: "最近活跃",
  sessions: "会话数",
};

export function sortProjects(projects: ProjectRow[], key: SortKey): ProjectRow[] {
  const arr = [...projects];
  switch (key) {
    case "cost":
      return arr.sort((a, b) => b.cost_usd - a.cost_usd);
    case "tokens":
      return arr.sort((a, b) => b.tokens - a.tokens);
    case "active":
      return arr.sort((a, b) => (b.last_active_ms ?? 0) - (a.last_active_ms ?? 0));
    case "sessions":
      return arr.sort((a, b) => b.sessions - a.sessions);
  }
}

export default function ProjectsView() {
  const [projects, setProjects] = useState<ProjectRow[] | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("cost");
  const [search, setSearch] = useState("");

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
    return sortProjects(filtered, sortKey);
  }, [projects, search, sortKey]);

  const pager = usePager(processed, 20);

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

  return (
    <div className="projects-page">
      <div className="ops-card">
        <div className="ops-card-title">
          📁 项目中心
          <span className="ops-card-sub">
            {projects ? `${totals.count} 个项目 · ${totals.sessions} 会话 · ${formatTokens(totals.tokens)} · ${formatCost(totals.cost)}` : "加载中…"}
          </span>
        </div>
        <div className="scope-bar" style={{ alignItems: "center" }}>
          <span style={{ fontSize: 11.5, opacity: 0.6 }}>排序</span>
          {(Object.keys(SORT_LABELS) as SortKey[]).map((k) => (
            <button key={k} className={`scope-chip ${sortKey === k ? "active" : ""}`} onClick={() => setSortKey(k)}>
              {SORT_LABELS[k]}
            </button>
          ))}
          <input
            className="settings-confirm-input"
            style={{ marginLeft: "auto", width: 180, fontSize: 12 }}
            placeholder="🔍 搜索项目 / Agent…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        {projects?.length === 0 && <div className="ops-table-empty">暂无项目用量数据（导入会话并同步指标后生成）</div>}
        {projects && projects.length > 0 && processed.length === 0 && (
          <div className="ops-table-empty">无匹配项目</div>
        )}
      </div>
      <div className="project-grid">
        {pager.slice.map((p) => (
          <div key={p.dir} className="project-card">
            <div className="project-name mono" title={p.dir}>{shortDir(p.dir)}</div>
            <div className="cost-ratio" title={`成本占比（相对最大项目）`}>
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
    </div>
  );
}
