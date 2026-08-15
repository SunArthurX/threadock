// 资产 Section：资产清单（按 agent 分组+类型颜色）+ 自动化任务（完成折叠）
import type { AssetRow, AutomationRow } from "./ops-types";
import { meta } from "./ops-types";

interface Props {
  assets: AssetRow[];
  automations: AutomationRow[];
  loading: boolean;
}

export default function AssetsSection({ assets, automations, loading }: Props) {
  const isDone = (s: string | null) => s?.includes("completed") || s?.includes("finished") || s?.includes("idle");
  const active = automations.filter((a) => !isDone(a.status));
  const done = automations.filter((a) => isDone(a.status));

  const autoRow = (a: AutomationRow, i: number) => (
    <div key={i} className="ops-risky-row">
      <span className={`badge source ${a.provider}`}>{meta(a.provider).label}</span>
      <span className="asset-name mono">{a.name}</span>
      <span className="policy-kind">{a.kind}</span>
      {a.schedule && <span className="mono" style={{ fontSize: 10.5, color: "var(--text-muted)" }}>{a.schedule}</span>}
      {a.status && <span className={`risk-flag ${isDone(a.status) ? "low" : "medium"}`}>{a.status}</span>}
      <span className="ops-risky-cmd mono">{a.detail ?? ""}</span>
    </div>
  );

  return (
    <>
      <div className="ops-card">
        <div className="ops-card-title">
          🧩 资产清单（{assets.length}）
          <span className="ops-card-sub">skills / plugins / 内置技能</span>
        </div>
        {assets.length === 0 ? (
          loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">后台同步中…</div>
        ) : (
          Object.entries(
            assets.reduce<Record<string, AssetRow[]>>((g, a) => {
              (g[a.provider] = g[a.provider] || []).push(a);
              return g;
            }, {})
          ).map(([prov, items]) => (
            <div key={prov} className="asset-group">
              <div className="asset-group-header">
                <span className={`badge source ${prov}`}>{meta(prov).label}</span>
                <span className="asset-group-count">{items.length} 项</span>
              </div>
              <div className="assets-grid">
                {items.map((a, i) => (
                  <div key={i} className={`asset-item kind-${a.kind} ${a.risky_hits > 0 ? "risky" : ""}`}>
                    <span className={`asset-kind-chip kind-${a.kind}`}>
                      {a.kind === "builtin_skill" ? "内置" : a.kind === "plugin" ? "插件" : a.kind === "mcp" ? "MCP" : "技能"}
                    </span>
                    <span className="asset-name mono" title={a.path ?? ""}>{a.name}</span>
                    {a.version && <span className="asset-ver mono">v{a.version}</span>}
                    {a.risky_hits > 0 && <span className="risk-flag high">⚠ {a.risky_hits}</span>}
                  </div>
                ))}
              </div>
            </div>
          ))
        )}
      </div>

      <div className="ops-card">
        <div className="ops-card-title">
          ⏱ 自动化任务（{automations.length}）
          <span className="ops-card-sub">cron / workflow / 后台任务</span>
        </div>
        {automations.length === 0 ? (
          loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">暂无数据</div>
        ) : (<>
          <div className="ops-risky">
            {active.length > 0 && <div className="automation-sub">进行中（{active.length}）</div>}
            {active.map(autoRow)}
            {active.length === 0 && done.length === 0 && <div className="ops-table-empty">无任务</div>}
          </div>
          {done.length > 0 && (
            <details className="automation-done">
              <summary>已完成的任务（{done.length}）</summary>
              <div className="ops-risky">{done.map(autoRow)}</div>
            </details>
          )}
        </>)}
      </div>
    </>
  );
}
