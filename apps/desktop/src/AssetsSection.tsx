// 资产 Section：资产清单（按 agent 分组+类型颜色）+ 自动化任务（完成折叠）
import { useState } from "react";
import type { AssetRow, AutomationRow } from "./ops-types";
import { meta } from "./ops-types";

interface Props {
  assets: AssetRow[];
  automations: AutomationRow[];
  loading: boolean;
}


/** 自动化关注列表（localStorage；受只读原则限制不做禁用，仅本地标记）。 */
export function loadAutomationWatch(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem("ch-automation-watch") ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}

export function toggleAutomationWatch(key: string): Set<string> {
  const cur = loadAutomationWatch();
  if (cur.has(key)) {
    cur.delete(key);
  } else {
    cur.add(key);
  }
  localStorage.setItem("ch-automation-watch", JSON.stringify([...cur]));
  return cur;
}
export default function AssetsSection({ assets, automations, loading }: Props) {
  const [watch, setWatch] = useState<Set<string>>(loadAutomationWatch);
  const isDone = (s: string | null) => s?.includes("completed") || s?.includes("finished") || s?.includes("idle");
  const activeSorted = [...automations.filter((a) => !isDone(a.status))].sort(
    (a, b) => Number(watch.has(`${b.provider}:${b.name}`)) - Number(watch.has(`${a.provider}:${a.name}`)),
  );
  const done = automations.filter((a) => isDone(a.status));

  const autoRow = (a: AutomationRow, i: number) => {
    const wk = `${a.provider}:${a.name}`;
    const watched = watch.has(wk);
    return (
      <div key={i} className={`ops-risky-row ${watched ? "watched" : ""}`}>
        <span
          className={`watch-toggle ${watched ? "on" : ""}`}
          title={watched ? "取消关注" : "关注此任务（置顶标记；受只读原则限制不修改来源配置）"}
          onClick={() => setWatch(toggleAutomationWatch(wk))}
        >
          {watched ? "★" : "☆"}
        </span>
        <span className={`badge source ${a.provider}`}>{meta(a.provider).label}</span>
        <span className="asset-name mono">{a.name}</span>
        <span className="policy-kind">{a.kind}</span>
        {a.schedule && <span className="mono" style={{ fontSize: 10.5, color: "var(--text-muted)" }}>{a.schedule}</span>}
        {a.status && <span className={`risk-flag ${isDone(a.status) ? "low" : "medium"}`}>{a.status}</span>}
        <span className="ops-risky-cmd mono">{a.detail ?? ""}</span>
      </div>
    );
  };

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
            {activeSorted.length > 0 && <div className="automation-sub">进行中（{activeSorted.length}）</div>}
            {activeSorted.map(autoRow)}
            {activeSorted.length === 0 && done.length === 0 && <div className="ops-table-empty">无任务</div>}
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
