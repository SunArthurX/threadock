// 安全 Section：异常检测 + 安全审计 + 策略规则 + 风险调用
// 增强：bulk 处置（全部忽略/全部误报）+ 策略规则 export/import JSON
import { formatDuration } from "./charts";
import { usePager } from "./usePager";
import type { AnomalyRow, AuditReport, AuditFinding, PolicyRule, RiskyCall } from "./ops-types";
import { meta, SEV_LABEL } from "./ops-types";
import { showToast } from "./toast";
import ScrollArea from "./ScrollArea";

interface Props {
  anomalies: AnomalyRow[];
  audit: AuditReport | null;
  auditing: boolean;
  auditKindFilter: "all" | "sensitive" | "dangerous_command";
  policies: PolicyRule[];
  newPolicy: { name: string; pattern: string; kind: string; severity: string };
  risky: RiskyCall[];
  expandedRisk: Set<string>;
  loading: boolean;
  onScan: () => void;
  onExportHtml: () => void;
  onFilter: (f: "all" | "sensitive" | "dangerous_command") => void;
  onAddPolicy: () => void;
  onRemovePolicy: (name: string) => void;
  onPolicyInput: (field: string, value: string) => void;
  onTogglePolicyEnabled: (rule: PolicyRule) => void;
  onDisposeFinding: (fingerprint: string, status: "ignored" | "false_positive") => void;
  onBulkDisposeFindings: (fingerprints: string[], status: "ignored" | "false_positive") => void;
  onRefreshAfterDispose: () => void;
  onToggleRisk: (id: string) => void;
  onImportPolicies: (json: string) => void;
  onJump: (provider: string, sessionId: string, messageId: string | null) => void;
}

export default function SecuritySection(p: Props) {
  // P1-B2: 相对时间格式化（与 App.tsx FreshnessBadge 风格保持一致）
  const relativeTime = (input: string | number | null | undefined): string => {
    if (input == null) return "—";
    const ts = typeof input === "number" ? input : Date.parse(input);
    if (!Number.isFinite(ts)) return "—";
    // 相对时间显示：每次 render 都取最新时间，父级会驱动重渲染。
    // eslint-disable-next-line react-hooks/purity
    const ageMs = Date.now() - ts;
    if (ageMs < 0) return "刚刚";
    const min = Math.floor(ageMs / 60_000);
    if (min < 1) return "刚刚";
    if (min < 60) return `${min} 分钟前`;
    if (min < 1440) return `${Math.floor(min / 60)} 小时前`;
    return `${Math.floor(min / 1440)} 天前`;
  };
  const findings = (p.audit?.findings ?? []).filter((f) => p.auditKindFilter === "all" || f.kind === p.auditKindFilter);
  const anomalyPager = usePager(p.anomalies, 20);
  const riskyPager = usePager(p.risky, 20);
  const pagerBar = (pg: { page: number; totalPages: number; total: number; needed: boolean; prev: () => void; next: () => void }) =>
    pg.needed ? (
      <div className="pager">
        <button className="pager-btn" onClick={pg.prev} disabled={pg.page === 0}>‹ 上一页</button>
        <span className="pager-info">{pg.page + 1} / {pg.totalPages} 页 · 共 {pg.total} 条</span>
        <button className="pager-btn" onClick={pg.next} disabled={pg.page >= pg.totalPages - 1}>下一页 ›</button>
      </div>
    ) : null;

  const visibleFindings = findings.slice(0, 50);
  const handleBulk = async (status: "ignored" | "false_positive") => {
    if (visibleFindings.length === 0) return;
    const fps = visibleFindings.map((f) => f.fingerprint);
    try {
      await p.onBulkDisposeFindings(fps, status);
      const label = status === "ignored" ? "忽略" : "标记为误报";
      showToast(`✓ 已${label} ${fps.length} 条发现`, "info");
      p.onRefreshAfterDispose();
    } catch (e) { showToast(`失败：${String(e)}`, "error"); }
  };

  /** 策略规则 export 为 JSON 复制到剪贴板（用户可贴到 issue / 备份）。 */
  const exportPolicies = async () => {
    if (p.policies.length === 0) { showToast("无策略规则可导出", "warn"); return; }
    const json = JSON.stringify(p.policies, null, 2);
    try {
      await navigator.clipboard.writeText(json);
      showToast(`✓ 已复制 ${p.policies.length} 条策略规则（JSON）到剪贴板`, "info");
    } catch { showToast("剪贴板不可用", "error"); }
  };
  const importPolicies = async () => {
    const text = window.prompt("粘贴策略规则 JSON（覆盖现有同名规则）：");
    if (!text) return;
    p.onImportPolicies(text);
  };

  return (
    <>
      <div className="ops-card">
        <div className="ops-card-title">🚨 异常检测（{p.anomalies.length}）</div>
        {p.anomalies.length === 0 ? (
          p.loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">未检测到异常 🎉</div>
        ) : (
          <>
          <div className="ops-risky">
            {anomalyPager.slice.map((a, i) => (
              <div key={i} className="ops-risky-row">
                <span className={`risk-flag ${a.severity}`}>
                  {a.kind === "error_spike" ? "错误尖峰" : a.kind === "retry_storm" ? "重试风暴" : "context超限"}
                </span>
                <span className="mono" style={{ fontSize: 11.5 }}>{a.detail}</span>
                {a.source_session_id && (
                  <button className="finding-btn" title="跳转到对应会话"
                    onClick={() => p.onJump(a.provider ?? "*", a.source_session_id!, null)}>→ 会话</button>
                )}
              </div>
            ))}
          </div>
          {pagerBar(anomalyPager)}
          </>
        )}
      </div>

      <div className="ops-card">
        <div className="ops-card-title">
          🛡 安全审计
          {p.audit ? (
            <>
              <span className="audit-stats">
                扫描 {p.audit.scanned_messages.toLocaleString()} 消息 / {p.audit.scanned_tool_calls.toLocaleString()} 命令 ·
                <b className="text-danger"> 高危 {p.audit.high}</b> · <b>中危 {p.audit.medium}</b>
              </span>
              {/* P1-B2: 审计新鲜度（与 OpsView 同步按钮风格一致） */}
              <span className="ops-freshness" style={{ marginLeft: "auto" }} title={`扫描时间：${p.audit.generated_at}`}>· 扫描于 {relativeTime(p.audit.generated_at)}</span>
            </>
          ) : null}
        </div>
        {/* P2-4: 首次访问引导 — 突出主操作 + 背景说明（无 audit 时显示） */}
        {p.audit == null && !p.auditing && (
          <div className="audit-hero">
            <div className="audit-hero-icon" aria-hidden>🔍</div>
            <div className="audit-hero-body">
              <div className="audit-hero-title">首次访问？点此开始全库扫描</div>
              <div className="audit-hero-sub">
                扫描器会遍历本地所有已归档会话，识别敏感信息（密钥、token、邮箱）
                与危险命令（rm -rf、sudo、chmod 777 等），并按正则规则匹配。
                通常 1–2 分钟内完成；扫描结果保存到本地，不会上传到任何服务器。
              </div>
              <div className="audit-hero-cta">
                <button className="action-btn primary" onClick={p.onScan}>▶ 开始全库扫描</button>
              </div>
            </div>
          </div>
        )}
        <div className="audit-toolbar">
          {/* P2-4: audit==null 时此按钮降级为次要（hero 才是主操作） */}
          <button className={`action-btn ${p.audit == null && !p.auditing ? "" : "primary"}`} onClick={p.onScan}>
            {p.auditing ? "扫描中…" : p.audit ? "↻ 重新扫描" : "▶ 开始全库扫描"}
          </button>
          {p.audit && p.audit.findings.length > 0 && (<>
            <button className="action-btn" onClick={p.onExportHtml}>⤓ 导出报告</button>
            <button className="action-btn" onClick={() => handleBulk("ignored")} title="全部标记为「忽略」（本会话不再报）">⊘ 全部忽略</button>
            <button className="action-btn" onClick={() => handleBulk("false_positive")} title="全部标记为「误报」（同类规则不再报）">⊗ 全部误报</button>
            <div className="ops-range">
              {([["all","全部"],["sensitive","敏感信息"],["dangerous_command","危险命令"]] as const).map(([v,l]) => (
                <button key={v} className={`filter-chip ${p.auditKindFilter === v ? "active" : ""}`} onClick={() => p.onFilter(v)}>{l}</button>
              ))}
            </div>
          </>)}
        </div>

        {p.audit && findings.length > 0 && (
          <div className="audit-findings">
            {findings.slice(0, 50).map((f: AuditFinding, i) => (
              <div key={i} className="audit-finding-row" onClick={() => p.onJump(f.provider, f.source_conversation_id, f.message_id)}>
                <span className={`risk-flag ${f.severity}`}>{SEV_LABEL[f.severity]}</span>
                <span className={`badge source ${f.provider}`}>{meta(f.provider).label}</span>
                <span className="audit-finding-rule mono">{f.rule}</span>
                <span className="audit-finding-snippet mono">{f.snippet}</span>
                <span className="finding-actions" onClick={(e) => e.stopPropagation()}>
                  <button className="finding-btn" title="不再提示此发现" onClick={() => { p.onDisposeFinding(f.fingerprint, "ignored"); p.onRefreshAfterDispose(); }}>忽略</button>
                  <button className="finding-btn" title="标记为误报（同类不再报）" onClick={() => { p.onDisposeFinding(f.fingerprint, "false_positive"); p.onRefreshAfterDispose(); }}>误报</button>
                </span>
              </div>
            ))}
          </div>
        )}
        {p.audit && p.audit.findings.length === 0 && (
          <div className="ops-table-empty">
            扫描完成，未发现未处置风险 🎉（已忽略/误报的不再显示，可在下方处置列表管理）
          </div>
        )}

        <div className="policy-section">
          <div className="budget-label">
            自定义策略规则（正则）
            <button className="kb-copy" style={{ marginLeft: "auto" }} onClick={exportPolicies} title="把当前规则复制为 JSON 粘贴到剪贴板">⤓ 导出</button>
            <button className="kb-copy" onClick={importPolicies} title="粘贴 JSON 批量导入（同名规则覆盖）">⤒ 导入</button>
          </div>
          <div className="policy-add">
            <input placeholder="规则名" value={p.newPolicy.name} onChange={(e) => p.onPolicyInput("name", e.target.value)} />
            <input placeholder="正则" value={p.newPolicy.pattern} onChange={(e) => p.onPolicyInput("pattern", e.target.value)} />
            <select value={p.newPolicy.kind} onChange={(e) => p.onPolicyInput("kind", e.target.value)}>
              <option value="dangerous_command">危险命令</option>
              <option value="sensitive">敏感信息</option>
            </select>
            <select value={p.newPolicy.severity} onChange={(e) => p.onPolicyInput("severity", e.target.value)}>
              <option value="high">高危</option><option value="medium">中危</option><option value="low">低危</option>
            </select>
            <button className="action-btn" onClick={p.onAddPolicy}>＋ 添加</button>
          </div>
          {p.policies.length > 0 && (
            <div className="policy-list">
              {p.policies.map((rule) => (
                <div key={rule.id} className={`policy-row ${rule.enabled ? "" : "disabled"}`}>
                  <span className={`risk-flag ${rule.severity}`}>{SEV_LABEL[rule.severity]}</span>
                  <span className="mono">{rule.name}</span>
                  <span className="policy-kind">{rule.kind === "sensitive" ? "敏感" : "命令"}</span>
                  <span className="mono policy-pattern">{rule.pattern}</span>
                  <label className="policy-toggle" title={rule.enabled ? "点击停用（保留规则不扫描）" : "点击启用"}>
                    <input type="checkbox" checked={rule.enabled} onChange={() => p.onTogglePolicyEnabled(rule)} />
                    {rule.enabled ? "启用" : "停用"}
                  </label>
                  <button className="policy-del" onClick={() => p.onRemovePolicy(rule.name)}>✕</button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="ops-card">
        <div className="ops-card-title">风险调用（{p.risky.length}）</div>
        <div className="ops-risky">
          {riskyPager.slice.map((r) => {
            const open = p.expandedRisk.has(r.id);
            return (
              <div key={r.id} className={`ops-risky-item ${open ? "open" : ""}`}>
                <div className="ops-risky-row" onClick={() => p.onToggleRisk(r.id)}>
                  <span className="risk-caret">{open ? "▾" : "▸"}</span>
                  <span className={`badge source ${r.provider}`}>{meta(r.provider).label}</span>
                  <span className="mono ops-risky-tool">{r.tool_name}</span>
                  {r.destructive && <span className="risk-flag high">危险</span>}
                  {r.exit_code != null && r.exit_code !== 0 && <span className="risk-flag medium">exit {r.exit_code}</span>}
                  <span className="ops-risky-cmd mono">{r.command_text ?? r.source_session_id.slice(0, 18)}</span>
                  <span className="ops-risky-time">{new Date(r.ts_ms).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}</span>
                </div>
                {open && (
                  <div className="ops-risky-detail">
                    <div className="risk-detail-grid">
                      <div><b>时间：</b>{new Date(r.ts_ms).toLocaleString("zh-CN")}</div>
                      <div><b>状态：</b>{r.status}</div>
                      <div><b>退出码：</b>{r.exit_code ?? "—"}</div>
                      <div><b>耗时：</b>{r.duration_ms != null ? formatDuration(r.duration_ms) : "—"}</div>
                    </div>
                    {r.command_text && <ScrollArea className="risk-detail-cmd mono"><pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{r.command_text}</pre></ScrollArea>}
                    <button className="action-btn" onClick={() => p.onJump(r.provider, r.source_session_id, null)}>→ 跳转到对应会话</button>
                  </div>
                )}
              </div>
            );
          })}
          {p.risky.length === 0 && <div className="ops-table-empty">无风险调用 🎉</div>}
        </div>
        {pagerBar(riskyPager)}
      </div>
    </>
  );
}
