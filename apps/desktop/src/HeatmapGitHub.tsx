// GitHub 标准热力图：7 行（Mon→Sun）× N 列（周数），整体横躺
// 完全独立实现：inline style 强制 12×12 + aspect-ratio 兜底，避开 styles.css 中所有
// `.heat-cell` 相关的历史冲突（round 9/10/13/14/15 多次迭代导致 CSS 重复定义）。
import { memo } from "react";

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

/** GitHub 风格热力图：7 行（Mon-Sun）× N 列。完全独立样式，inline + aspect-ratio 兜底。 */
function HeatmapGitHub({ cols, max, monthLabels, selectedDay, todayKey, onClickCell }: Props) {
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
                    title={`${cell.day} · ${cell.calls} 次调用 · ${cell.sessions} 活跃会话${isToday ? "（今天）" : ""}`}
                    onClick={() => onClickCell?.(cell.day)}
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
                    onMouseEnter={(e) => {
                      (e.currentTarget as HTMLElement).style.transform = "scale(1.6)";
                      (e.currentTarget as HTMLElement).style.boxShadow = "0 0 0 1.5px #4f8cff, 0 2px 6px rgba(0,0,0,0.4)";
                      (e.currentTarget as HTMLElement).style.zIndex = "2";
                    }}
                    onMouseLeave={(e) => {
                      (e.currentTarget as HTMLElement).style.transform = "";
                      (e.currentTarget as HTMLElement).style.boxShadow = "";
                      (e.currentTarget as HTMLElement).style.zIndex = "";
                    }}
                  />
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default memo(HeatmapGitHub);
