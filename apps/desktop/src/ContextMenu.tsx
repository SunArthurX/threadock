// 通用右键菜单：屏幕定位 + Esc 关闭 + 点击外关闭 + 上下方向键移动 + Enter 触发。
// 替代「每个列表项都放一堆图标按钮」的臃肿布局 —— 把「收藏/归档/置顶/标签/导出/删除」等动作
// 收纳到单个 ⋯ 或右键触发的小菜单里。
import { useEffect, useRef, useState } from "react";

export interface MenuItem {
  label: string;
  icon?: string;
  onClick: () => void;
  /** 设为 true 时菜单项置灰，不响应点击。 */
  disabled?: boolean;
  /** 危险操作（红色字），如删除。 */
  danger?: boolean;
  /** 分组（与前一项不同组时插入一条分割线）。 */
  group?: number;
}

interface Props {
  /** 触发位置（相对视口）。 */
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

export default function ContextMenu({ x, y, items, onClose }: Props) {
  const [idx, setIdx] = useState(0);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { onClose(); return; }
      // 只在「未 disabled」项之间移动
      const navigable = items.map((it, i) => ({ it, i })).filter((x) => !x.it.disabled);
      if (navigable.length === 0) return;
      const curPos = navigable.findIndex((x) => x.i === idx);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        const next = navigable[(curPos + 1) % navigable.length];
        if (next) setIdx(next.i);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const prev = navigable[(curPos - 1 + navigable.length) % navigable.length];
        if (prev) setIdx(prev.i);
      } else if (e.key === "Enter") {
        e.preventDefault();
        const cur = items[idx];
        if (cur && !cur.disabled) { cur.onClick(); onClose(); }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [idx, items]);

  // 视口边界保护：菜单不能超出视口
  const MENU_W = 200;
  const MENU_H_EST = items.length * 28 + 8;
  const posX = Math.min(x, (typeof window !== "undefined" ? window.innerWidth : 1024) - MENU_W - 8);
  const posY = Math.min(y, (typeof window !== "undefined" ? window.innerHeight : 768) - MENU_H_EST - 8);

  return (
    <>
      {/* 透明 backdrop 拦截点击外部（按 context menu 习惯） */}
      <div className="contextmenu-backdrop" onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      <div
        ref={ref}
        className="contextmenu"
        style={{ left: posX, top: posY }}
        role="menu"
        data-testid="contextmenu"
        onClick={(e) => e.stopPropagation()}
      >
        {items.map((it, i) => (
          <div key={i}>
            {i > 0 && it.group !== undefined && items[i - 1]?.group !== it.group && (
              <div className="contextmenu-sep" />
            )}
            <div
              role="menuitem"
              className={`contextmenu-item ${idx === i ? "active" : ""} ${it.disabled ? "disabled" : ""} ${it.danger ? "danger" : ""}`}
              onMouseEnter={() => setIdx(i)}
              onClick={() => {
                if (it.disabled) return;
                it.onClick();
                onClose();
              }}
            >
              {it.icon && <span className="contextmenu-icon">{it.icon}</span>}
              <span className="contextmenu-label">{it.label}</span>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
