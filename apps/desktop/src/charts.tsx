// 轻量 SVG 图表组件：零第三方依赖，配色取自设计系统 provider 专属色。

export interface DonutSlice {
  label: string;
  value: number;
  color: string;
}

/** 环形图（Agent 分布） */
export function DonutChart({ slices, size = 160 }: { slices: DonutSlice[]; size?: number }) {
  const total = slices.reduce((s, x) => s + x.value, 0);
  if (total <= 0) return <div className="chart-empty">无数据</div>;
  const r = size / 2 - 14;
  const c = 2 * Math.PI * r;
  let acc = 0;
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
      <g transform={`rotate(-90 ${size / 2} ${size / 2})`}>
        {slices.map((s, i) => {
          const frac = s.value / total;
          const dash = frac * c;
          const offset = -acc * c;
          acc += frac;
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
            >
              <title>{`${s.label}: ${formatTokens(s.value)} (${(frac * 100).toFixed(1)}%)`}</title>
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
}

/** 柱状图（每日 tokens 趋势），取最后 n 条 */
export function BarChart({ data, height = 140, color = "var(--accent)" }: { data: BarDatum[]; height?: number; color?: string }) {
  const items = data.slice(-30);
  if (items.length === 0) return <div className="chart-empty">无数据</div>;
  const max = Math.max(...items.map((d) => d.value), 1);
  const gap = 3;
  const bw = Math.max(100 / items.length - gap, 2);
  return (
    <div className="barchart" style={{ height }}>
      <div className="barchart-bars">
        {items.map((d, i) => (
          <div
            key={i}
            className="barchart-bar"
            style={{
              width: `${bw}%`,
              height: `${Math.max((d.value / max) * 100, 1.5)}%`,
              background: color,
            }}
            title={d.title ?? `${d.label}: ${formatTokens(d.value)}`}
          />
        ))}
      </div>
      <div className="barchart-axis">
        <span>{items[0]?.label.slice(5)}</span>
        <span>{items[items.length - 1]?.label.slice(5)}</span>
      </div>
    </div>
  );
}

/** 数字格式化：43 亿 → 4.3B / 1.2M / 350K */
export function formatTokens(n: number): string {
  if (n >= 1e9) return (n / 1e9).toFixed(2) + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(2) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "K";
  return String(n);
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
