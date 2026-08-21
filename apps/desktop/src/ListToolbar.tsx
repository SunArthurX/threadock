// 列表页 toolbar 统一组件：search + sort + 计数 + 操作
// 替代之前在 资产/项目/知识库 三个页面各自写的 toolbar 逻辑。
// 用法：
//   <ListToolbar
//     search={query} onSearch={setQuery} searchPlaceholder="搜索项目 / Agent ..."
//     count={filtered.length} total={all.length}
//     sort={sort} onSortChange={setSort} sortOptions={[{value:"cost",label:"成本"}]}
//     trailing={<button>重算</button>}
//   />
import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export interface ListToolbarSortOption<T extends string = string> {
  value: T;
  label: string;
  /** 可选，排序项前的图标 */
  icon?: IconName;
}

export interface ListToolbarProps {
  /** 搜索框受控值 */
  search?: string;
  onSearch?: (v: string) => void;
  searchPlaceholder?: string;
  /** 当前选中过滤维度（如"全部工具"/"全部时间"） */
  filterValue?: string;
  onFilterChange?: (v: string) => void;
  filterOptions?: { value: string; label: string }[];
  filterLabel?: string;
  /** 排序 */
  sort?: string;
  onSortChange?: (v: string) => void;
  sortOptions?: ListToolbarSortOption[];
  sortLabel?: string;
  /** 计数（如"12 / 50 个"），无值时不显示 */
  count?: number;
  countTotal?: number;
  countLabel?: string;
  /** 右侧操作区（如"重算"按钮） */
  trailing?: ReactNode;
  /** 左侧副操作（如"同步指标"按钮） */
  leading?: ReactNode;
  /** 紧凑模式（不显示标签） */
  dense?: boolean;
}

export function ListToolbar({
  search,
  onSearch,
  searchPlaceholder = "搜索…",
  filterValue,
  onFilterChange,
  filterOptions,
  filterLabel = "筛选",
  sort,
  onSortChange,
  sortOptions,
  sortLabel = "排序",
  count,
  countTotal,
  countLabel = "项",
  trailing,
  leading,
  dense = false,
}: ListToolbarProps) {
  return (
    <div className={`list-toolbar ${dense ? "list-toolbar-dense" : ""}`.trim()}>
      {leading && <div className="list-toolbar-leading">{leading}</div>}

      {(filterOptions || sortOptions) && (
        <div className="list-toolbar-segment">
          {filterOptions && onFilterChange && (
            <label className="list-toolbar-segment-item">
              {!dense && <span className="list-toolbar-segment-label">{filterLabel}</span>}
              <select
                className="list-toolbar-select"
                value={filterValue ?? ""}
                onChange={(e) => onFilterChange(e.target.value)}
              >
                {filterOptions.map((o) => (
                  <option key={o.value} value={o.value}>{o.label}</option>
                ))}
              </select>
            </label>
          )}
          {sortOptions && onSortChange && (
            <label className="list-toolbar-segment-item">
              {!dense && <span className="list-toolbar-segment-label">{sortLabel}</span>}
              <select
                className="list-toolbar-select"
                value={sort ?? sortOptions[0]?.value ?? ""}
                onChange={(e) => onSortChange(e.target.value)}
              >
                {sortOptions.map((o) => (
                  <option key={o.value} value={o.value}>{o.label}</option>
                ))}
              </select>
            </label>
          )}
        </div>
      )}

      {count !== undefined && (
        <div className="list-toolbar-count">
          <b>{count.toLocaleString()}</b>
          {countTotal !== undefined && <span> / {countTotal.toLocaleString()}</span>}
          <span className="list-toolbar-count-label">{countLabel}</span>
        </div>
      )}

      {onSearch && (
        <div className="list-toolbar-search">
          <Icon name="search" size={13} className="list-toolbar-search-icon" />
          <input
            type="search"
            placeholder={searchPlaceholder}
            value={search ?? ""}
            onChange={(e) => onSearch(e.target.value)}
          />
        </div>
      )}

      {trailing && <div className="list-toolbar-trailing">{trailing}</div>}
    </div>
  );
}

export default ListToolbar;
