// 轻量 SVG 图表组件：零第三方依赖，配色取自设计系统 provider 专属色。
// 全部带入场动画：数值从 0 → 目标（0 到有值再到最大）。

import { useEffect, useRef, useState, type ReactNode } from "react";

/** mount 后一帧置 true，触发 CSS transition 从 0 → 目标 */
function useMounted(): boolean {
  const [m, setM] = useState(false);
  useEffect(() => {
    const id = requestAnimationFrame(() => setM(true));
    return () => cancelAnimationFrame(id);
  }, []);
  return m;
}

/** 数字滚动（easeOut，800ms）：从 0 计数到目标值 */
export function useCountUp(target: number, duration = 800): number {
  const [val, setVal] = useState(0);
  const fromRef = useRef(0);
  useEffect(() => {
    const from = fromRef.current;
    const start = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const t = Math.min((now - start) / duration, 1);
      const eased = 1 - Math.pow(1 - t, 3);
      setVal(from + (target - from) * eased);
      if (t < 1) raf = requestAnimationFrame(tick);
      else fromRef.current = target;
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [target, duration]);
  return val;
}

export interface DonutSlice {
  label: string;
  value: number;
  color: string;
}

/** 环形图（Agent 分布）：入场时各扇区从 0 度扫到目标 */
export function DonutChart({ slices, size = 160 }: { slices: DonutSlice[]; size?: number }) {
  const mounted = useMounted();
  const total = slices.reduce((s, x) => s + x.value, 0);
  if (total <= 0) return <div className="chart-empty">无数据</div>;
  const r = size / 2 - 14;
  const c = 2 * Math.PI * r;
  let acc = 0;
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
      <g transform={`rotate(-90 ${size / 2} ${size / 2})`}>
        {slices.map((s, i) => {
          const frac = (s.value / total) * (mounted ? 1 : 0);
          const dash = frac * c;
          const offset = -acc * c;
          acc += s.value / total;
          return (
            <circle
              key={i}
              cx={size / 2}
              cy={size / 2}
              r={r}
              fill="none"
              stroke={s.color}
              strokeWidth={18}
              strokeDasharray={`${dash} ${c - dash}`}
              strokeDashoffset={offset}
              style={{
                transition: `stroke-dasharray 700ms cubic-bezier(.2,.8,.3,1) ${i * 90}ms`,
              }}
            >
              <title>{`${s.label}: ${formatTokens(s.value)} (${((s.value / total) * 100).toFixed(1)}%)`}</title>
            </circle>
          );
        })}
      </g>
      <text x="50%" y="47%" textAnchor="middle" className="donut-total">
        {formatTokens(total)}
      </text>
      <text x="50%" y="58%" textAnchor="middle" className="donut-label">
        tokens
      </text>
    </svg>
  );
}

export interface BarDatum {
  label: string;
  value: number;
  title?: string;
  /** 附加到柱子 div 的 className（如 'bar-peak' 高亮峰值）。 */
  className?: string;
}

/** 柱状图（每日 tokens 趋势）：柱子从 0 生长到目标高度（交错延迟） */
export function BarChart({
  data,
  height = 140,
  color = "var(--accent)",
  barClassName,
  /** 自定义 hover 渲染（label/value 实时 tooltip），不传则走 title 降级。 */
  renderTooltip,
  /** 自定义轴 label 提取函数（默认取 label 后 5 位）。 */
  axisLabel,
}: {
  data: BarDatum[];
  height?: number;
  color?: string;
  /** 给所有柱子追加的 className（前缀）。 */
  barClassName?: string;
  renderTooltip?: (d: BarDatum, max: number) => ReactNode;
  axisLabel?: (d: BarDatum) => string;
}) {
  const mounted = useMounted();
  const [hover, setHover] = useState<{ i: number; x: number } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const items = data.slice(-30);
  if (items.length === 0) return <div className="chart-empty">无数据</div>;
  const max = Math.max(...items.map((d) => d.value), 1);

  const onMouseMove = (e: React.MouseEvent<HTMLDivElement>, i: number) => {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;
    setHover({ i, x: e.clientX - rect.left });
  };

  const hovered = hover ? items[hover.i] : null;
  const tooltip = hovered && renderTooltip ? renderTooltip(hovered, max) : null;

  return (
    <div
      ref={containerRef}
      className="barchart"
      style={{ height }}
      onMouseLeave={() => setHover(null)}
    >
      {/* 横向网格线：4 条（25% / 50% / 75% / 100%）便于读数 */}
      <div className="barchart-grid" style={{ pointerEvents: "none" }} aria-hidden>
        {[0.25, 0.5, 0.75].map((p) => (
          <div key={p} className="barchart-grid-line" style={{ bottom: `${p * 100}%` }} />
        ))}
      </div>
      <div className="barchart-bars">
        {items.map((d, i) => (
          <div
            key={i}
            className={`barchart-bar ${barClassName ?? ""} ${d.className ?? ""}`.trim()}
            style={{
              width: `${Math.max(100 / items.length - 3, 2)}%`,
              height: mounted ? `${Math.max((d.value / max) * 100, 1.5)}%` : "0%",
              background: color,
              transition: `height 600ms cubic-bezier(.2,.8,.3,1) ${Math.min(i * 25, 500)}ms`,
              filter: hover?.i === i ? "brightness(1.25)" : undefined,
            }}
            title={d.title ?? `${d.label}: ${formatTokens(d.value)}`}
            onMouseMove={(e) => onMouseMove(e, i)}
          />
        ))}
      </div>
      <div className="barchart-axis">
        <span>{axisLabel ? axisLabel(items[0]) : items[0]?.label.slice(5)}</span>
        <span>{axisLabel ? axisLabel(items[items.length - 1]) : items[items.length - 1]?.label.slice(5)}</span>
      </div>
      {hovered && tooltip && hover && (
        <div
          className="barchart-tooltip"
          style={{ left: hover.x, bottom: height + 4 }}
        >
          {tooltip}
        </div>
      )}
      {/* 悬停柱子上方的时间 pill：直接在柱顶上方浮一个时间标签，比 tooltip 更显眼 */}
      {hovered && hover && (
        <div
          className="barchart-hover-label"
          style={{ left: hover.x }}
        >
          {axisLabel ? axisLabel(hovered) : hovered.label}
        </div>
      )}
    </div>
  );
}

/** 数字格式化：43 亿 → 4.3B / 1.2M / 350K */
export function formatTokens(n: number): string {
  if (n >= 1e9) return (n / 1e9).toFixed(2) + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(2) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "K";
  return String(Math.round(n));
}

export function formatCost(usd: number): string {
  if (usd <= 0) return "—";
  return `$${usd < 100 ? usd.toFixed(2) : usd.toFixed(0)}`;
}

export function formatDuration(ms: number): string {
  if (ms <= 0) return "—";
  if (ms < 1000) return `${ms.toFixed(0)}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60_000).toFixed(1)}min`;
}
