import { useCallback, useId, useRef } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";

/// 页签组的键盘与读屏约定（WAI-ARIA tabs 模式，照 SidePanel 现成写法抽出）：
/// - ←/→（纵向时 ↑/↓）切到相邻页签、Home/End 到首尾，到头不回绕（与 SidePanel 一致）；
/// - 选中跟随焦点（自动激活）：这几处切页签都是纯前端换列表，没有加载代价；
/// - roving tabindex：只有当前页签在 Tab 序列里，整组只占一个 Tab 停靠点；
/// - 所有页签 aria-controls 指向同一个面板 id，面板 aria-labelledby 当前页签。
///   这几处都只渲染当前页签的内容（QuestionPanels 同法），所以只有一个面板。
/// 起因：侧栏三页签、后台任务筛选、Lead 面板页签写了 role="tab" 却没有方向键，
/// 读屏念「标签页 1/3」、按方向键没反应（2026-11-11 调研 i9，a11y-out.txt 第 7～9 步）。
///
/// 用法：
///   const tabs = useTablist({ keys: ["a", "b"], active, onSelect: setActive });
///   <div {...tabs.listProps} aria-label="…">
///     {keys.map((k) => <button key={k} type="button" {...tabs.tabProps(k)}>…</button>)}
///   </div>
///   <div {...tabs.panelProps}>…当前页签内容…</div>
/// 面板元素本身不该改 role 时（例如 <nav> 地标），只取 id：`id={tabs.panelId}`。
/// 页签和面板不在同一个组件里时，父组件 useId 出一个前缀传给页签侧的 `idBase`，
/// 面板侧用 `tablistIds(前缀)` 拼 id（TasksDockPanel 即此用法）。
export type TablistOptions<K extends string | number> = {
  keys: readonly K[];
  active: K | null;
  onSelect: (key: K) => void;
  orientation?: "horizontal" | "vertical";
  /// 不传就用 useId。页签与面板分在两个组件时，两边传同一个值。
  idBase?: string;
};

/// 页签与面板分在两个组件时，面板那侧用它拼出同一套 id（与 useTablist 内部同一格式）。
export function tablistIds(base: string) {
  return { panelId: `${base}-panel`, tabId: (key: string | number) => `${base}-tab-${String(key)}` };
}

export function useTablist<K extends string | number>({ keys, active, onSelect, orientation = "horizontal", idBase }: TablistOptions<K>) {
  const auto = useId();
  const base = idBase ?? auto;
  const refs = useRef(new Map<K, HTMLElement>());
  const { panelId, tabId } = tablistIds(base);
  // 没有选中项（例如侧栏三页签之外的视图）时让第一个页签留在 Tab 序列里，整组仍可达。
  const focusable = active !== null && keys.includes(active) ? active : keys[0];

  const onKeyDown = useCallback((key: K) => (event: ReactKeyboardEvent) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const prev = orientation === "vertical" ? "ArrowUp" : "ArrowLeft";
    const next = orientation === "vertical" ? "ArrowDown" : "ArrowRight";
    const index = keys.indexOf(key);
    const last = keys.length - 1;
    const target = event.key === prev ? index - 1 : event.key === next ? index + 1 : event.key === "Home" ? 0 : event.key === "End" ? last : null;
    if (target === null || index < 0) return;
    event.preventDefault();
    const to = keys[Math.max(0, Math.min(target, last))];
    if (to !== key) onSelect(to);
    refs.current.get(to)?.focus();
  }, [keys, onSelect, orientation]);

  const tabProps = (key: K) => ({
    id: tabId(key),
    role: "tab" as const,
    "aria-selected": active === key,
    "aria-controls": panelId,
    tabIndex: key === focusable ? 0 : -1,
    onKeyDown: onKeyDown(key),
    ref: (el: HTMLElement | null) => { if (el) refs.current.set(key, el); else refs.current.delete(key); },
  });

  return {
    listProps: { role: "tablist" as const, ...(orientation === "vertical" ? { "aria-orientation": "vertical" as const } : {}) },
    tabProps,
    panelId,
    panelProps: { id: panelId, role: "tabpanel" as const, ...(focusable !== undefined ? { "aria-labelledby": tabId(focusable) } : {}) },
  };
}
