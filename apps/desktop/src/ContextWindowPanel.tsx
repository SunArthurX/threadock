// 会话上下文窗口用量面板（对标 ZCode 的 Context windows 选择器，加强数据密度）：
// 总量/窗口占比 + 分类分段堆叠条 + 每类「数值·占比」+ 每 turn 上下文增长
// sparkline + 请求/输出/推理/缓存明细。总量优先后末条 usage 记录（实测），
// 无记录退化为文本估算（黄色徽章标注口径）。
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { formatTime } from "./types";
import type { ContextUsageSummary } from "./types";

/** token 数格式化：1_234 → 1.2K，4_117_000 → 4.1M。 */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** 分类档位配色（借鉴 ZCode：首类亮蓝，其余渐浅；余量灰）。 */
const CATEGORY_COLORS: Record<string, string> = {
  messages: "#3b82f6",
  tool_outputs: "#60a5fa",
  thinking: "#93c5fd",
  system_blocks: "#bfdbfe",
  system_other: "#94a3b8",
};

const CATEGORY_LABELS: Record<string, string> = {
  messages: t("用户与助手消息"),
  tool_outputs: t("工具调用与输出"),
  thinking: t("思考过程"),
  system_blocks: t("系统注入"),
  system_other: t("系统提示与工具定义（估）"),
};

/** 每 turn 上下文规模 sparkline（面积折线）。 */
function TrendSpark({ trend }: { trend: number[] }) {
  const path = useMemo(() => {
    if (trend.length < 2) return null;
    const w = 100;
    const h = 26;
    const max = Math.max(...trend, 1);
    const step = w / (trend.length - 1);
    const pts = trend.map((v, i) => `${(i * step).toFixed(2)},${(h - (v / max) * (h - 3) - 1.5).toFixed(2)}`);
    return {
      line: `M${pts.join("L")}`,
      area: `M0,${h}L${pts.join("L")}L${w},${h}Z`,
    };
  }, [trend]);
  if (!path) return null;
  return (
    <svg
      className="ctx-trend"
      viewBox="0 0 100 26"
      preserveAspectRatio="none"
      role="img"
      aria-label={t("上下文增长趋势")}
    >
      <path d={path.area} fill="rgba(59,130,246,0.12)" stroke="none" />
      <path d={path.line} fill="none" stroke="#3b82f6" strokeWidth={1.2} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

/**
 * 后端 DTO 兜底：数组/数字字段缺失时补默认值——面板不应因单个字段缺失
 * 把整个详情页打崩（IPC 异常或后端结构变更时降级为不渲染）。
 */
function normalize(u: ContextUsageSummary): ContextUsageSummary {
  return {
    ...u,
    total_tokens: u.total_tokens ?? 0,
    window_tokens: u.window_tokens ?? 0,
    turn_count: u.turn_count ?? 0,
    output_tokens: u.output_tokens ?? 0,
    reasoning_tokens: u.reasoning_tokens ?? 0,
    cache_read_tokens: u.cache_read_tokens ?? 0,
    cache_write_tokens: u.cache_write_tokens ?? 0,
    fresh_tokens: u.fresh_tokens ?? 0,
    categories: u.categories ?? [],
    trend: u.trend ?? [],
  };
}

export default function ContextWindowPanel({ convId }: { convId: string }) {
  const [usage, setUsage] = useState<ContextUsageSummary | null>(null);
  useEffect(() => {
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切换会话时清旧用量再异步加载
    setUsage(null);
    invoke<ContextUsageSummary | null>("conversation_context_usage", {
      conversationId: convId,
    })
      .then((u) => {
        if (!cancelled) setUsage(u ? normalize(u) : null);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [convId]);

  // 降级：总量非正（缺字段/异常）→ 不渲染（不崩详情页）
  if (!usage || !(usage.total_tokens > 0)) return null;
  const categories = usage.categories;
  const trend = usage.trend;
  const pct = usage.window_tokens > 0 ? (usage.total_tokens / usage.window_tokens) * 100 : 0;
  const pctLabel = pct >= 100 ? "100%" : `${pct.toFixed(1)}%`;
  const share = (tokens: number) =>
    usage.total_tokens > 0 ? (tokens / usage.total_tokens) * 100 : 0;
  const remaining = Math.max(0, usage.window_tokens - usage.total_tokens);
  const trendDelta =
    trend.length >= 2
      ? trend[trend.length - 1] - trend[0]
      : null;

  return (
    <div className="ctx-window-panel">
      <div className="ctx-window-head">
        <span className="ctx-window-title">{t("上下文窗口")}</span>
        {!usage.measured && <span className="ctx-window-est">{t("按文本估算")}</span>}
        <span className="ctx-window-total mono">
          {fmtTokens(usage.total_tokens)}/{fmtTokens(usage.window_tokens)}（{pctLabel}）
        </span>
      </div>
      {/* 分段堆叠条：各分类占窗口比例，右侧灰段 = 剩余 */}
      <div className="ctx-window-bar" title={t("上下文窗口")}>
        {categories.map(
          (c) =>
            c.tokens > 0 && usage.window_tokens > 0 && (
              <div
                key={c.key}
                className="ctx-window-seg"
                style={{
                  width: `${Math.min(100, (c.tokens / usage.window_tokens) * 100)}%`,
                  background: CATEGORY_COLORS[c.key] ?? "#94a3b8",
                }}
                title={`${CATEGORY_LABELS[c.key] ?? c.key} ${fmtTokens(c.tokens)}`}
              />
            ),
        )}
        {remaining > 0 && usage.window_tokens > 0 && (
          <div
            className="ctx-window-seg ctx-window-seg-remain"
            style={{ width: `${(remaining / usage.window_tokens) * 100}%` }}
            title={`${t("剩余")} ${fmtTokens(remaining)}`}
          />
        )}
      </div>
      {/* 分类明细：数值 + 占比 */}
      <div className="ctx-window-cats">
        {categories.map((c) => (
          <div className="ctx-window-row" key={c.key}>
            <span
              className="ctx-dot"
              style={{ background: CATEGORY_COLORS[c.key] ?? "#94a3b8" }}
            />
            <span className="ctx-window-cat">{CATEGORY_LABELS[c.key] ?? c.key}</span>
            <span className="ctx-window-nums mono">
              {fmtTokens(c.tokens)}
              <span className="ctx-window-sep">·</span>
              {share(c.tokens).toFixed(1)}%
            </span>
          </div>
        ))}
        <div className="ctx-window-row">
          <span className="ctx-dot ctx-dot-remain" />
          <span className="ctx-window-cat">{t("剩余可用")}</span>
          <span className="ctx-window-nums mono">
            {fmtTokens(remaining)}
            <span className="ctx-window-sep">·</span>
            {usage.window_tokens > 0 ? ((remaining / usage.window_tokens) * 100).toFixed(1) : "0"}%
          </span>
        </div>
      </div>
      {/* 每 turn 上下文增长轨迹 */}
      {trend.length >= 2 && (
        <div className="ctx-window-trend">
          <div className="ctx-window-trend-head">
            <span>{t("上下文增长（最近 {{n}} 轮）").replace("{n}", String(trend.length))}</span>
            {trendDelta != null && (
              <span className="mono">
                {trendDelta >= 0 ? "+" : ""}
                {fmtTokens(trendDelta)}
              </span>
            )}
          </div>
          <TrendSpark trend={trend} />
        </div>
      )}
      {/* 明细栅格：请求 / 输出 / 推理 / 末次 */}
      {usage.measured && (
        <div className="ctx-window-grid mono">
          <span className="ctx-grid-item">
            {t("请求")} {usage.turn_count.toLocaleString()}
          </span>
          <span className="ctx-grid-item">
            {t("输出")} {fmtTokens(usage.output_tokens)}
          </span>
          {usage.reasoning_tokens > 0 && (
            <span className="ctx-grid-item">
              {t("推理")} {fmtTokens(usage.reasoning_tokens)}
            </span>
          )}
          {usage.last_turn_at_ms != null && (
            <span className="ctx-grid-item">
              {t("末次")} {formatTime(usage.last_turn_at_ms).slice(5, 16)}
            </span>
          )}
        </div>
      )}
      {/* 缓存明细：命中率 + 绝对量 */}
      {usage.cache_hit_rate != null && (
        <div className="ctx-window-cache">
          <span>
            {t("平均缓存命中率")}{" "}
            <span className="mono">{(usage.cache_hit_rate * 100).toFixed(1)}%</span>
          </span>
          <span className="mono ctx-cache-nums">
            {t("命中")} {fmtTokens(usage.cache_read_tokens)}
            <span className="ctx-window-sep">·</span>
            {t("写入")} {fmtTokens(usage.cache_write_tokens)}
            <span className="ctx-window-sep">·</span>
            {t("非缓存")} {fmtTokens(usage.fresh_tokens)}
          </span>
        </div>
      )}
    </div>
  );
}
