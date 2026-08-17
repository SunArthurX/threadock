// Workspace 管理分区（v1.0.0，plan §4.3）：列表 + 匹配置信度 + 重命名 + 手动合并
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";

interface WorkspaceRow {
  id: string;
  display_name: string;
}

interface SourceLink {
  workspace_id: string;
  workspace_name: string;
  provider_id: string;
  raw_name: string | null;
  match_method: string | null;
  match_confidence: number | null;
}

/** 置信度低于该值的映射建议人工确认（plan §4.3 低置信度交互）。 */
const LOW_CONFIDENCE = 0.8;

export default function WorkspaceSection() {
  const [workspaces, setWorkspaces] = useState<WorkspaceRow[] | null>(null);
  const [links, setLinks] = useState<SourceLink[]>([]);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    // Array.isArray 防御：IPC 异常/返回非数组时保持 null（空态），不让整树崩
    try {
      const r = await invoke<unknown>("list_workspaces");
      if (Array.isArray(r)) setWorkspaces(r as WorkspaceRow[]);
    } catch { /* 空库忽略 */ }
    try {
      const r = await invoke<unknown>("workspace_source_links");
      if (Array.isArray(r)) setLinks(r as SourceLink[]);
    } catch { /* 空表忽略 */ }
  };
  // eslint-disable-next-line react-hooks/set-state-in-effect -- 数据加载 effect：加载完成后才 setState
  useEffect(() => { void load(); }, []);

  // 每个 workspace 的最低匹配置信度（低 → 建议人工确认）
  const minConf = useMemo(() => {
    const m = new Map<string, number>();
    for (const l of links) {
      if (l.match_confidence == null) continue;
      const cur = m.get(l.workspace_id);
      if (cur == null || l.match_confidence < cur) m.set(l.workspace_id, l.match_confidence);
    }
    return m;
  }, [links]);
  const lowCount = useMemo(
    () => [...minConf.values()].filter((v) => v < LOW_CONFIDENCE).length,
    [minConf],
  );

  const rename = async (ws: WorkspaceRow) => {
    const name = window.prompt("重命名 Workspace：", ws.display_name);
    if (!name?.trim() || name.trim() === ws.display_name) return;
    try {
      await invoke("workspace_rename", { id: ws.id, newName: name.trim() });
      showToast("✓ 已重命名", "info");
      await load();
    } catch (e) { showToast(String(e), "error"); }
  };

  const merge = async (ws: WorkspaceRow) => {
    if (!workspaces || workspaces.length < 2) return;
    const targets = workspaces.filter((w) => w.id !== ws.id);
    const idx = window.prompt(
      `把「${ws.display_name}」合并到哪个 Workspace？输入序号：\n` +
        targets.map((t, i) => `${i + 1}. ${t.display_name}`).join("\n"),
    );
    if (!idx) return;
    const n = Number(idx.trim());
    if (!Number.isInteger(n) || n < 1 || n > targets.length) { showToast("无效序号", "error"); return; }
    const target = targets[n - 1];
    if (!window.confirm(`确定把「${ws.display_name}」的全部会话并入「${target.display_name}」？此操作会删除原 Workspace（已记入治理审计日志）。`)) return;
    setBusy(true);
    try {
      const moved = await invoke<number>("workspace_merge", { sourceId: ws.id, targetId: target.id });
      showToast(`✓ 已合并：迁移 ${moved} 条会话到「${target.display_name}」`, "info");
      await load();
    } catch (e) { showToast(String(e), "error"); } finally { setBusy(false); }
  };

  if (workspaces === null) return null;
  if (workspaces.length === 0) {
    return (
      <section className="settings-section">
        <h3>🗂 Workspace 管理</h3>
        <div className="settings-hint">暂无 Workspace（导入会话后自动生成）</div>
      </section>
    );
  }

  return (
    <section className="settings-section">
      <h3>
        🗂 Workspace 管理
        <span className="settings-card-sub" style={{ marginLeft: 8 }}>
          {workspaces.length} 个{lowCount > 0 ? ` · ${lowCount} 个低置信度待确认` : ""}
        </span>
      </h3>
      <div className="settings-hint">
        自动合并基于 7 级优先级（plan §4.3）；置信度 &lt; {LOW_CONFIDENCE} 的映射建议人工合并确认。
        「拆分」在会话列表多选后使用「拆分到新 Workspace」。
      </div>
      <ScrollArea className="ws-admin-list">
        {workspaces.map((ws) => {
          const conf = minConf.get(ws.id);
          return (
            <div key={ws.id} className="ws-admin-row">
              <span className="ws-admin-name">{ws.display_name}</span>
              {conf != null && (
                <span
                  className={`ws-conf-badge ${conf < LOW_CONFIDENCE ? "low" : "ok"}`}
                  title={`该 Workspace 来源映射的最低匹配置信度 ${conf.toFixed(2)}（${conf < LOW_CONFIDENCE ? "建议人工确认" : "可信"}）`}
                >
                  {conf < LOW_CONFIDENCE ? `⚠ ${conf.toFixed(2)}` : `✓ ${conf.toFixed(2)}`}
                </span>
              )}
              <button className="bulk-btn" disabled={busy} onClick={() => rename(ws)}>✏️ 重命名</button>
              <button className="bulk-btn" disabled={busy || workspaces.length < 2} onClick={() => merge(ws)}
                title={workspaces.length < 2 ? "至少需要两个 Workspace 才能合并" : "把这个 Workspace 并入另一个"}>
                ⇄ 合并到…
              </button>
            </div>
          );
        })}
      </ScrollArea>
    </section>
  );
}
