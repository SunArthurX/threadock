// 交接弹窗：把归档会话（含上下文）写回目标 Agent 的原生会话存储，
// 在那边 resume 继续开发。写入为纯增量（全新会话 ID，不动目标已有数据）。
//
// 入口：详情工具栏「⇄ 交接」与列表右键「交接到其他 Agent…」（单条/多选）。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { showToast } from "./toast";
import { Icon } from "./Icon";
import { copyToClipboard } from "./clipboard";
import ScrollArea from "./ScrollArea";
import { sourceLabel } from "./types";
import type { HandoffSummary, HandoffTargetInfo } from "./types";

export interface HandoffModalProps {
  /** 点名交接的会话内部 ID（all=true 时忽略）。 */
  conversationIds: string[];
  onClose: () => void;
}

/** 目标 → resume 提示（claude/codex 有 CLI resume 命令；其余在各自会话列表里打开）。 */
function resumeHint(target: string, title: string, sessionId: string): { cmd?: string; tip: string } {
  switch (target) {
    case "claude-code":
      return { cmd: `claude --resume ${sessionId}`, tip: t("在项目目录的终端里执行即可继续。") };
    case "codex":
      return { cmd: `codex resume ${sessionId}`, tip: t("在项目目录的终端里执行即可继续。") };
    default:
      return { tip: t("在 {__0__} 的会话列表中打开「{__1__}」即可继续。", { __0__: sourceLabel(target), __1__: title }) };
  }
}

export default function HandoffModal({ conversationIds, onClose }: HandoffModalProps) {
  const [infos, setInfos] = useState<HandoffTargetInfo[]>([]);
  const [target, setTarget] = useState<string>("all");
  const [includeContext, setIncludeContext] = useState(true);
  const [all, setAll] = useState(false);
  const [cwd, setCwd] = useState("");
  const [running, setRunning] = useState(false);
  const [summary, setSummary] = useState<HandoffSummary | null>(null);

  useEffect(() => {
    invoke<HandoffTargetInfo[]>("handoff_targets").then(setInfos).catch(() => setInfos([]));
  }, []);
  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape" && !running) onClose(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onClose, running]);

  const availableCount = infos.filter((i) => i.available).length;
  const canRun = availableCount > 0 && !running && (all || conversationIds.length > 0);
  // 选中目标不可用（含默认 all 且零可用）时，派生出首个可用目标——
  // 不用 effect + setState，避免级联渲染。
  const targetOk = target === "all" ? availableCount > 0 : infos.some((i) => i.target === target && i.available);
  const effectiveTarget = targetOk ? target : (infos.find((i) => i.available)?.target ?? "all");

  const run = async (dryRun: boolean) => {
    setRunning(true);
    try {
      const s = await invoke<HandoffSummary>("handoff_conversations", {
        conversationIds: all ? [] : conversationIds,
        target: effectiveTarget,
        all,
        dryRun,
        includeContext,
        cwd: cwd.trim() || null,
      });
      setSummary(s);
      if (!dryRun) {
        const okCount = s.results.reduce((n, r) => n + r.ok.length, 0);
        const failCount = s.results.reduce((n, r) => n + r.failed.length, 0);
        const labels = s.results.map((r) => sourceLabel(r.target)).join(" / ");
        if (failCount === 0) showToast(t("✓ 已交接 {__0__} 条会话到 {__1__}", { __0__: okCount, __1__: labels }), "info");
        else showToast(t("交接完成：成功 {__0__} · 失败 {__1__}", { __0__: okCount, __1__: failCount }), "warn");
      }
    } catch (e) {
      showToast(t("交接失败：{__0__}", { __0__: String(e) }), "error");
    }
    setRunning(false);
  };

  return (
    <div className="settings-backdrop" onClick={() => { if (!running) onClose(); }}>
      <div className="settings-modal handoff-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2><Icon name="export" size={16} /> {t("⇄ 交接到其他 Agent")}</h2>
          <button className="settings-close" onClick={onClose}><Icon name="close" size={14} /></button>
        </div>
        <ScrollArea className="settings-body">
          <p className="settings-hint">
            {all
              ? t("将把全部会话（排除已删除与子任务，含上下文）写入目标 Agent 的本地会话存储，之后可在该工具里继续开发。")
              : t("将 {__0__} 条会话（含上下文）写入目标 Agent 的本地会话存储，之后可在该工具里继续开发。", { __0__: conversationIds.length })}
            {t("写入是纯增量：生成全新会话 ID，不改动目标里已有会话。")}
            {t("若你在治理中配置了自定义脱敏规则，内容会先脱敏再写入目标库（与导出一致）。")}
          </p>

          <label className="handoff-row">
            <input
              type="checkbox"
              checked={all}
              onChange={(e) => setAll(e.target.checked)}
            />
            {t("改为交接全部会话（排除已删除与子任务）")}
          </label>

          <div className="handoff-section-title">{t("目标 Agent")}</div>
          <div className="handoff-targets">
            <label className={`handoff-target ${availableCount === 0 ? "disabled" : ""}`}>
              <input
                type="radio"
                name="handoff-target"
                checked={effectiveTarget === "all"}
                disabled={availableCount === 0}
                onChange={() => setTarget("all")}
              />
              <span className="handoff-target-name">{t("全部可用目标")}</span>
              <span className="handoff-target-dest">
                {availableCount > 0 ? t("{__0__} 个目标可用", { __0__: availableCount }) : t("未检测到任何目标")}
              </span>
            </label>
            {infos.map((info) => (
              <label key={info.target} className={`handoff-target ${info.available ? "" : "disabled"}`}>
                <input
                  type="radio"
                  name="handoff-target"
                  checked={effectiveTarget === info.target}
                  disabled={!info.available}
                  onChange={() => setTarget(info.target)}
                />
                <span className="handoff-target-name">{sourceLabel(info.target)}</span>
                <span className="handoff-target-dest" title={info.destination}>
                  {info.available ? info.destination : t("未检测到")}
                </span>
              </label>
            ))}
          </div>

          <label className="handoff-row">
            <input
              type="checkbox"
              checked={includeContext}
              onChange={(e) => setIncludeContext(e.target.checked)}
            />
            {t("附带上下文前言（来源、工作目录、执行时间线）")}
          </label>

          <div className="handoff-section-title">{t("工作目录兜底（会话无工作区时使用，留空用主目录）")}</div>
          <input
            className="handoff-cwd-input"
            placeholder={t("如 /Users/me/projects/demo")}
            value={cwd}
            onChange={(e) => setCwd(e.target.value)}
          />

          {summary && (
            <div className="handoff-result">
              <div className="handoff-result-title">
                {summary.dry_run ? t("预演结果（未写入）") : t("交接结果")}
                <span> · {t("匹配 {__0__} 条会话", { __0__: summary.matched })}</span>
                {summary.missing.length > 0 && (
                  <span> · {t("未找到 {__0__} 条", { __0__: summary.missing.length })}</span>
                )}
              </div>
              {summary.results.flatMap((r) =>
                r.failed.map((f, i) => (
                  <div key={`f-${r.target}-${i}`} className="handoff-line fail">
                    ✗ {f.title}: {f.error}
                  </div>
                )),
              )}
              {summary.results.flatMap((r) =>
                r.ok.map((o, i) => {
                  const hint = resumeHint(r.target, o.title, o.session_id);
                  return (
                    <div key={`o-${r.target}-${i}`} className="handoff-line">
                      <span className="ok">✓</span> {sourceLabel(r.target)} · {o.title}
                      {summary.dry_run ? null : (
                        <>
                          {" · "}
                          {hint.cmd ? (
                            <button
                              className="link-btn"
                              onClick={() => {
                                void copyToClipboard(hint.cmd ?? "")
                                  .then((r2) => {
                                    if (r2.ok) showToast(t("✓ 恢复命令已复制"), "info", 1800);
                                    else showToast(t("剪贴板不可用"), "error");
                                  });
                              }}
                              title={t("点击复制恢复命令")}
                            >
                              <code>{hint.cmd}</code>
                            </button>
                          ) : (
                            <span className="handoff-hint">{hint.tip}</span>
                          )}
                        </>
                      )}
                    </div>
                  );
                }),
              )}
            </div>
          )}
        </ScrollArea>
        <div className="settings-footer">
          <button className="action-btn" disabled={!canRun} onClick={() => void run(true)}>
            {t("预演（不写入）")}
          </button>
          <button className="action-btn primary" disabled={!canRun} onClick={() => void run(false)}>
            {running ? t("交接中…") : t("开始交接")}
          </button>
        </div>
      </div>
    </div>
  );
}
