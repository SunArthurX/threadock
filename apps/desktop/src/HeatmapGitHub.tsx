// GitHub 标准热力图：7 行（Mon→Sun）× N 列（周数），整体横躺
// 完全独立实现：inline style 强制 12×12 + aspect-ratio 兜底，避开 styles.css 中所有
// `.heat-cell` 相关的历史冲突（round 9/10/13/14/15 多次迭代导致 CSS 重复定义）。
// 第 17 轮：加 hover 自定义 tooltip（绕开 macOS 原生 title 1.5s 延迟）
import { memo, useState } from "react";

export interface HeatCell {
  day: string;
  calls: number;
  sessions: number;
}

export interface HeatGridCol {
  cells: (HeatCell | null)[];
}

const HEAT_COLORS = ["transparent", "#0e4429", "#006d32", "#26a641", "#39d353"];

/** 5 档分档（与 heatColor 一致）。 */
function level(calls: number, max: number): number {
  if (calls === 0 || max <= 0) return 0;
  const r = calls / max;
  if (r > 0.75) return 4;
  if (r > 0.5) return 3;
  if (r > 0.25) return 2;
  return 1;
}

const DOWS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] as const;

const CELL = 12;
const GAP = 3;
const COL_W = CELL;
const DOW_COL_W = 30;

interface Props {
  cols: HeatGridCol[];
  max: number;
  /** 月份 label 映射（col index → 英文月份名）。 */
  monthLabels?: Map<number, string>;
  /** 当前选中的 day（高亮 cell）。 */
  selectedDay?: string | null;
  /** 今天的 day（高亮边框）。 */
  todayKey?: string | null;
  /** 点击 cell。 */
  onClickCell?: (day: string) => void;
}

/** 把 YYYY-MM-DD 转中文星期（Mon=一、Sun=日），无依赖项。 */
function weekdayCN(day: string): string {
  // 1970-01-01 是周四（week 4），归一到周日 = 0
  const seq = (() => {
    const [y, m, d] = day.split("-").map(Number);
    const yy = m <= 2 ? y - 1 : y;
    const mm = m > 2 ? m - 3 : m + 9;
    const era = Math.floor(yy / 400);
    const yoe = yy - era * 400;
    const doy = Math.floor((153 * mm + 2) / 5) + d - 1;
    const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
    return era * 146097 + doe - 719468;
  })();
  const dow = ["日", "一", "二", "三", "四", "五", "六"][(seq + 4) % 7];
  return `周${dow}`;
}

interface HoverInfo {
  cell: HeatCell | null; // null = 空格（无数据）
  x: number;
  y: number;
}

/** GitHub 风格热力图：7 行（Mon-Sun）× N 列。完全独立样式，inline + aspect-ratio 兜底。 */
function HeatmapGitHub({ cols, max, monthLabels, selectedDay, todayKey, onClickCell }: Props) {
  const [hover, setHover] = useState<HoverInfo | null>(null);

  return (
    <div
      data-testid="heatmap-github"
      style={{
        display: "inline-block",
        position: "relative",
        minWidth: "max-content", // 不允许父容器压缩 cell
      }}
    >
      {/* 顶部月份 label 行 */}
      <div
        style={{
          display: "flex",
          gap: GAP,
          paddingLeft: DOW_COL_W + 6, // 留出 weekday 列 + 间距
          marginBottom: 4,
          minWidth: "max-content",
        }}
      >
        {cols.map((_, ci) => (
          <div
            key={ci}
            data-testid="heatmap-month"
            style={{
              width: COL_W,
              ['flexShrink' as any]: 0,
              fontSize: 10.5,
              opacity: 0.7,
              whiteSpace: "nowrap",
              color: "var(--text-secondary, #9aa3bd)",
            }}
          >
            {monthLabels?.get(ci) ?? ""}
          </div>
        ))}
      </div>

      {/* 7 行 × N 列：左侧 weekday + 右侧 7×N 网格 */}
      <div style={{ display: "flex", gap: 6, alignItems: "flex-start", minWidth: "max-content" }}>
        {/* 左侧 weekday 列（7 行） */}
        <div style={{ display: "flex", flexDirection: "column", gap: GAP, ['flexShrink' as any]: 0 }}>
          {DOWS.map((d) => (
            <div
              key={d}
              data-testid="heatmap-dow"
              style={{
                width: DOW_COL_W,
                height: CELL,
                fontSize: 9.5,
                opacity: 0.55,
                color: "var(--text-secondary, #9aa3bd)",
                display: "flex",
                alignItems: "center",
                justifyContent: "flex-start",
                ['flexShrink' as any]: 0,
              }}
            >
              {d}
            </div>
          ))}
        </div>
        {/* 7×N 网格：N 个 col，每个 col 内 7 cells 垂直堆叠 */}
        <div style={{ display: "flex", gap: GAP, minWidth: "max-content" }}>
          {cols.map((col, ci) => (
            <div
              key={ci}
              style={{ display: "flex", flexDirection: "column", gap: GAP, ['flexShrink' as any]: 0 }}
            >
              {col.cells.map((cell, ri) => {
                if (!cell) {
                  return (
                    <span
                      key={ri}
                      data-testid="heatmap-cell-empty"
                      onMouseEnter={(e) => setHover({ cell: null, x: e.clientX, y: e.clientY })}
                      onMouseMove={(e) => setHover((h) => (h ? { ...h, x: e.clientX, y: e.clientY } : null))}
                      onMouseLeave={() => setHover(null)}
                      style={{
                        display: "block",
                        width: CELL,
                        height: CELL,
                        aspectRatio: "1 / 1",
                        border: "1px solid rgba(255,255,255,0.06)",
                        borderRadius: 2,
                        background: "transparent",
                        ['flexShrink' as any]: 0,
                        boxSizing: "border-box",
                      }}
                    />
                  );
                }
                const isSelected = selectedDay === cell.day;
                const isToday = cell.day === todayKey;
                const bg = HEAT_COLORS[level(cell.calls, max)];
                return (
                  <div
                    key={ri}
                    data-testid="heatmap-cell"
                    onClick={() => onClickCell?.(cell.day)}
                    onMouseEnter={(e) => {
                      setHover({ cell, x: e.clientX, y: e.clientY });
                      // 同时给 cell 加缩放高亮
                      (e.currentTarget as HTMLElement).style.transform = "scale(1.6)";
                      (e.currentTarget as HTMLElement).style.boxShadow = "0 0 0 1.5px #4f8cff, 0 2px 6px rgba(0,0,0,0.4)";
                      (e.currentTarget as HTMLElement).style.zIndex = "2";
                    }}
                    onMouseMove={(e) => setHover((h) => (h ? { ...h, x: e.clientX, y: e.clientY } : null))}
                    onMouseLeave={(e) => {
                      setHover(null);
                      (e.currentTarget as HTMLElement).style.transform = "";
                      (e.currentTarget as HTMLElement).style.boxShadow = "";
                      (e.currentTarget as HTMLElement).style.zIndex = "";
                    }}
                    style={{
                      display: "block",
                      width: CELL,
                      height: CELL,
                      aspectRatio: "1 / 1", // 强制正方形
                      border: isSelected || isToday
                        ? "1.5px solid " + (isSelected ? "#fbbf24" : "rgba(255,255,255,0.5)")
                        : "1px solid rgba(255,255,255,0.06)",
                      borderRadius: 2,
                      background: bg,
                      cursor: "pointer",
                      transition: "transform 0.1s, box-shadow 0.1s",
                      ['flexShrink' as any]: 0,
                      boxSizing: "border-box",
                    }}
                  />
                );
              })}
            </div>
          ))}
        </div>
      </div>

      {/* Hover 自定义 tooltip 浮层：position fixed 跟随鼠标 + transform(12,12) 偏移 + 主题色 */}
      {hover && (
        <div
          data-testid="heatmap-tooltip"
          style={{
            position: "fixed",
            zIndex: 1000,
            left: hover.x,
            top: hover.y,
            transform: "translate(12px, 12px)",
            pointerEvents: "none",
            background: "var(--bg-elevated, #161a25)",
            border: "1px solid var(--border-default, rgba(148, 163, 199, 0.18))",
            borderRadius: 6,
            padding: "6px 10px",
            fontSize: 11.5,
            lineHeight: 1.5,
            minWidth: 180,
            boxShadow: "0 4px 12px rgba(0, 0, 0, 0.5)",
            color: "var(--text-primary, #e6e9f2)",
          }}
        >
          {hover.cell ? (
            <>
              <div
                data-testid="heatmap-tooltip-day"
                style={{ fontWeight: 600, marginBottom: 2 }}
              >
                {hover.cell.day} · {weekdayCN(hover.cell.day)}
                {hover.cell.day === todayKey && (
                  <span
                    data-testid="heatmap-tooltip-today"
                    style={{
                      display: "inline-block",
                      marginLeft: 6,
                      padding: "1px 6px",
                      background: "rgba(96, 165, 250, 0.18)",
                      color: "#60a5fa",
                      borderRadius: 3,
                      fontSize: 10,
                      fontWeight: 500,
                    }}
                  >今天</span>
                )}
              </div>
              <div style={{ color: "var(--text-secondary, #9aa3bd)", fontSize: 11 }}>
                <span style={{ color: "var(--accent, #4f8cff)", fontWeight: 600 }}>{hover.cell.calls.toLocaleString()}</span> 次调用
                <span style={{ margin: "0 6px", opacity: 0.4 }}>·</span>
                <span style={{ color: "var(--accent, #4f8cff)", fontWeight: 600 }}>{hover.cell.sessions}</span> 活跃会话
              </div>
              <div style={{ marginTop: 3, fontSize: 10, opacity: 0.5 }}>
                强度 {level(hover.cell.calls, max)} / 4
              </div>
            </>
          ) : (
            <div style={{ color: "var(--text-muted, #5d6880)", fontSize: 11 }}>
              {cols[0]?.cells.find((c) => c)?.day
                ? `${hover.cell === null ? "无数据" : ""}`
                : "无数据"}
              <span style={{ marginLeft: 6, opacity: 0.7 }}>(空格 / 未来日期)</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default memo(HeatmapGitHub);
