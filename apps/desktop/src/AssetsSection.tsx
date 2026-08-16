// 资产 Section：资产清单（按 agent 分组+类型颜色）+ 自动化任务（完成折叠）
// 增强：点击资产弹详情（路径/版本/说明）+ 风险资产标红 + 复制资产 ID
import { useState } from "react";
import type { AssetRow, AutomationRow } from "./ops-types";
import { usePager } from "./usePager";
import { meta } from "./ops-types";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";

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
  const [detail, setDetail] = useState<AssetRow | null>(null);
  const isDone = (s: string | null) => s?.includes("completed") || s?.includes("finished") || s?.includes("idle");
  const activeAll = [...automations.filter((a) => !isDone(a.status))].sort(
    (a, b) => Number(watch.has(`${b.provider}:${b.name}`)) - Number(watch.has(`${a.provider}:${a.name}`)),
  );
  const activePager = usePager(activeAll, 20);
  const activeSorted = activePager.slice;
  const done = automations.filter((a) => isDone(a.status));

  const copyAssetField = async (label: string, text: string) => {
    try { await navigator.clipboard.writeText(text); showToast(`✓ 已复制 ${label}`, "info"); }
    catch { showToast("剪贴板不可用", "error"); }
  };

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
                  <div
                    key={i}
                    className={`asset-item kind-${a.kind} ${a.risky_hits > 0 ? "risky" : ""}`}
                    onClick={() => setDetail(a)}
                    title="点击查看详情"
                  >
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
            {activePager.needed && (
              <div className="pager">
                <button className="pager-btn" onClick={activePager.prev} disabled={activePager.page === 0}>‹ 上一页</button>
                <span className="pager-info">{activePager.page + 1} / {activePager.totalPages} 页 · 共 {activePager.total} 条</span>
                <button className="pager-btn" onClick={activePager.next} disabled={activePager.page >= activePager.totalPages - 1}>下一页 ›</button>
              </div>
            )}
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

      {detail && (
        <div className="settings-backdrop" onClick={() => setDetail(null)}>
          <div className="settings-modal asset-detail-modal" onClick={(e) => e.stopPropagation()}>
            <div className="settings-header">
              <h2>
                🧩 资产详情
                <span className={`asset-kind-chip kind-${detail.kind}`} style={{ marginLeft: 8 }}>
                  {detail.kind === "builtin_skill" ? "内置" : detail.kind === "plugin" ? "插件" : detail.kind === "mcp" ? "MCP" : "技能"}
                </span>
              </h2>
              <button className="settings-close" onClick={() => setDetail(null)}>✕</button>
            </div>
            <ScrollArea className="settings-body">
              <div className="asset-detail-row">
                <span className="asset-detail-label">名称</span>
                <span className="mono">{detail.name}</span>
                <button className="kb-copy" onClick={() => copyAssetField("资产名", detail.name)}>📋</button>
              </div>
              {detail.version && (
                <div className="asset-detail-row">
                  <span className="asset-detail-label">版本</span>
                  <span className="mono">v{detail.version}</span>
                </div>
              )}
              {detail.path && (
                <div className="asset-detail-row">
                  <span className="asset-detail-label">路径</span>
                  <span className="mono" style={{ fontSize: 11, wordBreak: "break-all" }}>{detail.path}</span>
                  <button className="kb-copy" onClick={() => copyAssetField("路径", detail.path ?? "")}>📋</button>
                </div>
              )}
              <div className="asset-detail-row">
                <span className="asset-detail-label">Provider</span>
                <span className={`badge source ${detail.provider}`}>{meta(detail.provider).label}</span>
              </div>
              {detail.risky_hits != null && detail.risky_hits > 0 && (
                <div className="asset-detail-row">
                  <span className="asset-detail-label">风险点</span>
                  <span className="risk-flag high">⚠ {detail.risky_hits} 处风险（按审计规则扫描）</span>
                </div>
              )}
              <div className="asset-detail-row">
                <span className="asset-detail-label">详情 JSON</span>
                <button className="kb-copy" onClick={() => copyAssetField("完整 JSON", JSON.stringify(detail, null, 2))}>📋 复制</button>
              </div>
            </ScrollArea>
          </div>
        </div>
      )}
    </>
  );
}
