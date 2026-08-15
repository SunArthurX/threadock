// 项目中心页：按工作目录聚合的卡片墙（用量/成本/活跃/主力 agent）
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatTokens, formatCost } from "./charts";
import { formatTime } from "./types";

export interface ProjectRow {
  dir: string;
  sessions: number;
  tokens: number;
  cost_usd: number;
  requests: number;
  last_active_ms: number | null;
  main_agent: string | null;
}

export default function ProjectsView() {
  const [projects, setProjects] = useState<ProjectRow[] | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const r = await invoke<{ projects: ProjectRow[] }>("projects_overview", {});
        setProjects(r.projects);
      } catch { /* 空库静默 */ }
    })();
  }, []);

  const shortDir = (d: string) => d.split("/").filter(Boolean).slice(-2).join("/") || d;

  return (
    <div className="projects-page">
      <div className="ops-card">
        <div className="ops-card-title">
          📁 项目中心
          <span className="ops-card-sub">{projects ? `${projects.length} 个项目 · 按实际工作目录聚合` : "加载中…"}</span>
        </div>
        {projects?.length === 0 && <div className="ops-table-empty">暂无项目用量数据（导入会话并同步指标后生成）</div>}
      </div>
      <div className="project-grid">
        {(projects ?? []).map((p) => (
          <div key={p.dir} className="project-card">
            <div className="project-name mono" title={p.dir}>{shortDir(p.dir)}</div>
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
    </div>
  );
}
