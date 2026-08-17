// 第 13 轮测试：热力图 GitHub 风格改造（12×12 圆角 + 5 档绿色 + 英文月份/星期 + Less/More legend）
import { describe, expect, it, beforeAll } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { buildHeatGrid, heatColor, heatLevel, HEAT_LEVELS } from "../ActivityView";

describe("heatColor GitHub 5 档绿色梯度", () => {
  it("0 档：透明（CSS 边框兜底）", () => {
    expect(heatColor(0, 100)).toBe("transparent");
  });

  it("1 档：最深绿 #0e4429（0 < r ≤ 0.25）", () => {
    expect(heatColor(10, 100)).toBe("#0e4429");
  });

  it("2 档：#006d32（0.25 < r ≤ 0.5）", () => {
    expect(heatColor(40, 100)).toBe("#006d32");
  });

  it("3 档：#26a641（0.5 < r ≤ 0.75）", () => {
    expect(heatColor(60, 100)).toBe("#26a641");
  });

  it("4 档：亮绿 #39d353（r > 0.75）", () => {
    expect(heatColor(100, 100)).toBe("#39d353");
  });

  it("max=0 时 0 档走 transparent（不抛错）", () => {
    expect(heatColor(0, 0)).toBe("transparent");
    expect(heatColor(5, 0)).toBe("#0e4429"); // r=0 → 走 1 档
  });
});

describe("heatLevel 0-4 档分档", () => {
  it("0 档", () => expect(heatLevel(0, 100)).toBe(0));
  it("1 档（>0 且 ≤0.25）", () => expect(heatLevel(10, 100)).toBe(1));
  it("2 档（>0.25 且 ≤0.5）", () => expect(heatLevel(30, 100)).toBe(2));
  it("3 档（>0.5 且 ≤0.75）", () => expect(heatLevel(60, 100)).toBe(3));
  it("4 档（>0.75）", () => expect(heatLevel(90, 100)).toBe(4));
});

describe("HEAT_LEVELS 5 档常量", () => {
  it("长度 = 5", () => expect(HEAT_LEVELS.length).toBe(5));
  it("索引 0 = transparent", () => expect(HEAT_LEVELS[0]).toBe("transparent"));
  it("索引 4 = 亮绿 #39d353", () => expect(HEAT_LEVELS[4]).toBe("#39d353"));
});

describe("buildHeatGrid 月份 label 改英文（GitHub 风格）", () => {
  it("2026-08 月份 label = 'Aug'", () => {
    const r = buildHeatGrid([{ day: "2026-08-15", calls: 5 }]);
    expect(r.labels.some((l) => l.label === "Aug")).toBe(true);
  });

  it("跨年数据：12 月 → 1 月 label 正确切换", () => {
    const r = buildHeatGrid([
      { day: "2025-12-30", calls: 1 },
      { day: "2025-12-31", calls: 1 },
      { day: "2026-01-01", calls: 1 },
    ]);
    const labels = r.labels.map((l) => l.label);
    expect(labels).toContain("Dec");
    expect(labels).toContain("Jan");
  });

  it("12 个月名都正确", () => {
    const en = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    for (let m = 1; m <= 12; m++) {
      const day = `2026-${String(m).padStart(2, "0")}-15`;
      const r = buildHeatGrid([{ day, calls: 1 }]);
      expect(r.labels.some((l) => l.label === en[m - 1])).toBe(true);
    }
  });
});

describe("CSS：GitHub 风格 cell 12×12 + 圆角 2px", () => {
  let css = "";
  beforeAll(() => {
    const HERE = dirname(fileURLToPath(import.meta.url));
    css = readFileSync(resolve(HERE, "../styles.css"), "utf-8");
  });

  it(".heat-cell width: 12px", () => {
    expect(/\.heat-cell\s*\{[^}]*width:\s*12px/m.test(css)).toBe(true);
  });

  it(".heat-cell height: 12px", () => {
    expect(/\.heat-cell\s*\{[^}]*height:\s*12px/m.test(css)).toBe(true);
  });

  it(".heat-cell border-radius: 2px", () => {
    expect(/\.heat-cell\s*\{[^}]*border-radius:\s*2px/m.test(css)).toBe(true);
  });

  it(".heat-legend-cell 12×12（与 cell 一致）", () => {
    expect(/\.heat-legend-cell\s*\{[^}]*width:\s*12px[^}]*height:\s*12px/m.test(css)).toBe(true);
  });

  it(".heatmap-scroll 不再带 mask-image（cell 小了不需要渐隐）", () => {
    // 取最后一个 .heatmap-scroll 规则（最近的覆盖前面的）
    const all = /\.heatmap-scroll\s*\{[^}]*\}/g;
    const blocks = css.match(all) ?? [];
    const last = blocks[blocks.length - 1] ?? "";
    expect(last.includes("mask-image")).toBe(false);
  });
});
