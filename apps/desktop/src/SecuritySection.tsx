// 安全 Section：异常检测 + 安全审计 + 策略规则 + 风险调用
import { formatDuration } from "./charts";
import type { AnomalyRow, AuditReport, AuditFinding, PolicyRule, RiskyCall } from "./ops-types";
import { meta, SEV_LABEL } from "./ops-types";

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
  onToggleRisk: (id: string) => void;
  onJump: (provider: string, sessionId: string, messageId: string | null) => void;
}

export default function SecuritySection(p: Props) {
  const findings = (p.audit?.findings ?? []).filter((f) => p.auditKindFilter === "all" || f.kind === p.auditKindFilter);

  return (
    <>
      <div className="ops-card">
        <div className="ops-card-title">🚨 异常检测（{p.anomalies.length}）</div>
        {p.anomalies.length === 0 ? (
          p.loading ? <div className="sk-line" style={{ margin: 12 }} /> : <div className="ops-table-empty">未检测到异常 🎉</div>
        ) : (
          <div className="ops-risky">
            {p.anomalies.map((a, i) => (
              <div key={i} className="ops-risky-row">
                <span className={`risk-flag ${a.severity}`}>
                  {a.kind === "error_spike" ? "错误尖峰" : a.kind === "retry_storm" ? "重试风暴" : "context超限"}
                </span>
                <span className="mono" style={{ fontSize: 11.5 }}>{a.detail}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="ops-card">
        <div className="ops-card-title">
          🛡 安全审计
          {p.audit && (
            <span className="audit-stats">
              扫描 {p.audit.scanned_messages.toLocaleString()} 消息 / {p.audit.scanned_tool_calls.toLocaleString()} 命令 ·
              <b className="text-danger"> 高危 {p.audit.high}</b> · <b>中危 {p.audit.medium}</b>
            </span>
          )}
        </div>
        <div className="audit-toolbar">
          <button className="action-btn" onClick={p.onScan}>
            {p.auditing ? "扫描中…" : p.audit ? "↻ 重新扫描" : "▶ 开始全库扫描"}
          </button>
          {p.audit && p.audit.findings.length > 0 && (<>
            <button className="action-btn" onClick={p.onExportHtml}>⤓ 导出报告</button>
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
              </div>
            ))}
          </div>
        )}
        {p.audit && p.audit.findings.length === 0 && <div className="ops-table-empty">扫描完成，未发现风险 🎉</div>}

        <div className="policy-section">
          <div className="budget-label">自定义策略规则（正则）</div>
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
                <div key={rule.id} className="policy-row">
                  <span className={`risk-flag ${rule.severity}`}>{SEV_LABEL[rule.severity]}</span>
                  <span className="mono">{rule.name}</span>
                  <span className="policy-kind">{rule.kind === "sensitive" ? "敏感" : "命令"}</span>
                  <span className="mono policy-pattern">{rule.pattern}</span>
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
          {p.risky.slice(0, 20).map((r) => {
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
                    {r.command_text && <pre className="risk-detail-cmd mono">{r.command_text}</pre>}
                    <button className="action-btn" onClick={() => p.onJump(r.provider, r.source_session_id, null)}>→ 跳转到对应会话</button>
                  </div>
                )}
              </div>
            );
          })}
          {p.risky.length === 0 && <div className="ops-table-empty">无风险调用 🎉</div>}
        </div>
      </div>
    </>
  );
}
