/// 侧栏「手动排序」的拖拽（对照 Kimi Code 侧栏的手动排序）。
///
/// 用指针事件自己做，不用 HTML5 拖放：对话窗开着 Tauri 的原生文件拖放（拖文件进来当附件），
/// Windows 上 WebView2 会把网页内的 HTML5 拖拽一并截走，dragover/drop 根本到不了页面。
/// 指针事件不受它影响，鼠标和触控笔都能用。
///
/// 触屏（手机页）改成长按起拖：手指一按下就拖会和列表滚动抢手势，用户上下划列表全成了挪会话。
/// 长按到点之前手指动了就是在滚动，放手不管；到点之后才接管手势、拦住页面滚动。长按到点但没挪
/// 就松手，交给 onHold（会话行用它开菜单，iOS 没有长按出的 contextmenu）。
///
/// 只能在同一个「桶」里拖：同一分组、同为置顶或同为非置顶。跨组拖没有意义（组由工作区 /
/// 日期 / 状态决定，不由位置决定），跨置顶边界拖了也会被置顶浮顶规则弹回去。

import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { folderName, groupDirOf, pathKey } from "../paths";

export type DropAt<K = number> = { id: K; after: boolean };

/// 落点 → 新位置的上下邻居。list 是拖动前这个桶里的显示顺序（含被拖的那条）。
/// above = 落下后紧挨在它上面的那条，below = 紧挨在下面的那条；落回原位返回 null（不用动）。
export function dropNeighbors<K>(list: readonly K[], dragged: K, drop: DropAt<K>): { above: K | null; below: K | null } | null {
  if (drop.id === dragged || !list.includes(drop.id)) return null;
  const rest = list.filter((id) => id !== dragged);
  const index = rest.indexOf(drop.id) + (drop.after ? 1 : 0);
  const above = rest[index - 1] ?? null;
  const below = rest[index] ?? null;
  const original = list.indexOf(dragged);
  if ((list[original - 1] ?? null) === above && (list[original + 1] ?? null) === below) return null;
  return { above, below };
}

/// 本地先挪（乐观更新）：把 id 那条挪到 above 正下方，没有 above 时挪到 below 正上方。
export function moveNextTo<T, K = number>(items: readonly T[], idOf: (item: T) => K, id: K, above: K | null, below: K | null): T[] {
  const moving = items.find((item) => idOf(item) === id);
  if (!moving) return [...items];
  const rest = items.filter((item) => idOf(item) !== id);
  const anchor = above ?? below;
  const at = rest.findIndex((item) => idOf(item) === anchor);
  if (at < 0) return [...items];
  rest.splice(above != null ? at + 1 : at, 0, moving);
  return rest;
}

/// 按下后移动多少像素才算开始拖：小于它仍是点击。
const DRAG_THRESHOLD = 4;
/// 触屏长按多久起拖。比系统长按菜单（Android 约 500ms）短，拖起来之后再到的 contextmenu 一律拦掉。
const LONG_PRESS_MS = 350;
/// 长按到点前手指挪了多远算「在滚动」，放弃这次长按。浏览器开始滚动的门槛比它大，滚动不受影响。
const LONG_PRESS_SLOP = 8;

/// 指针拖拽重排（会话行、工作区分组头共用）。`attr` 是行上 data-* 属性的 dataset 名，
/// `parse` 把属性值还原成 id；落点只认同一个桶里的行。
export function usePointerReorder<K extends string | number>({ enabled, attr, parse, bucketOf, listOf, onMove, onHold }: {
  enabled: boolean;
  attr: string;
  parse: (raw: string) => K | null;
  bucketOf: (id: K) => string | null;
  listOf: (bucket: string) => readonly K[];
  onMove: (id: K, above: K | null, below: K | null) => void;
  /// 触屏长按到点但没挪就松手（坐标是按下的位置）。
  onHold?: (id: K, x: number, y: number) => void;
}) {
  const [dragId, setDragId] = useState<K | null>(null);
  const [dropAt, setDropAt] = useState<DropAt<K> | null>(null);
  // 刚拖完：吞掉松手后紧跟着的那次 click，不然拖完会顺手打开会话。
  const suppressClickRef = useRef(false);
  const cleanupRef = useRef<(() => void) | null>(null);
  // 触屏长按进行中（按下到松手）。期间 Android 长按弹的 contextmenu 归这里管，行上别再开菜单。
  const touchHoldRef = useRef(false);
  useEffect(() => () => cleanupRef.current?.(), []);

  const selector = `[data-${attr.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}]`;
  /// ghostSource：跟手「影子」照着谁复制，默认按下的那个元素（分组只复制组头，不复制整组）。
  const onPointerDown = (event: ReactPointerEvent<HTMLElement>, id: K, ghostSource?: HTMLElement | null) => {
    if (!enabled || event.button !== 0) return;
    const bucket = bucketOf(id);
    if (bucket == null) return;
    cleanupRef.current?.();
    const touch = event.pointerType === "touch";
    const startX = event.clientX;
    const startY = event.clientY;
    // 指针拖拽没有浏览器自带的拖拽影像（用户实拍反馈「拖的时候没有影子」），自己复制一份跟着鼠标走。
    // currentTarget 只在事件分发期间有效，按下时就记住。
    const source = ghostSource ?? event.currentTarget;
    let ghost: HTMLElement | null = null;
    let grabX = 0;
    let grabY = 0;
    let started = false;
    let moved = false;
    let holdTimer = 0;
    let target: DropAt<K> | null = null;
    const pick = (x: number, y: number): DropAt<K> | null => {
      const row = document.elementFromPoint(x, y)?.closest<HTMLElement>(selector);
      const raw = row?.dataset[attr];
      const over = raw == null ? null : parse(raw);
      if (!row || over == null || bucketOf(over) !== bucket) return null;
      const rect = row.getBoundingClientRect();
      return { id: over, after: y > rect.top + rect.height / 2 };
    };
    const begin = () => {
      started = true;
      setDragId(id);
      document.body.classList.add("is-sidebar-dragging");
      const rect = source.getBoundingClientRect();
      grabX = startX - rect.left;
      grabY = startY - rect.top;
      ghost = source.cloneNode(true) as HTMLElement;
      ghost.removeAttribute("id");
      // 选中 / 勾选态的半透明底同优先级会盖掉影子的实底，影子里不带这些状态。
      ghost.classList.remove("is-active", "is-checked", "is-dragging", "is-drop-before", "is-drop-after");
      ghost.classList.add("sidebar-drag-ghost");
      ghost.style.width = `${rect.width}px`;
      // 挂在侧栏里而不是 body 下：行样式多是 .chat-sidebar 下的后代选择器，挂出去就走样。
      (source.closest(".chat-sidebar") ?? document.body).appendChild(ghost);
      ghost.style.transform = `translate(${startX - grabX}px, ${startY - grabY}px)`;
    };
    const move = (e: PointerEvent) => {
      const far = (limit: number) => Math.abs(e.clientY - startY) >= limit || Math.abs(e.clientX - startX) >= limit;
      if (touch && !started) {
        // 长按还没到点手指就走了：用户在滚列表，这次不拖。
        if (far(LONG_PRESS_SLOP)) finish(false);
        return;
      }
      if (!started) {
        if (!far(DRAG_THRESHOLD)) return;
        begin();
      }
      if (far(DRAG_THRESHOLD)) moved = true;
      e.preventDefault();
      if (ghost) ghost.style.transform = `translate(${e.clientX - grabX}px, ${e.clientY - grabY}px)`;
      target = pick(e.clientX, e.clientY);
      setDropAt(target);
    };
    const finish = (commit: boolean) => {
      cleanupRef.current?.();
      if (!started) return;
      suppressClickRef.current = true;
      window.setTimeout(() => { suppressClickRef.current = false; }, 0);
      setDragId(null);
      setDropAt(null);
      if (touch && !moved) {
        if (commit) onHold?.(id, startX, startY);
        return;
      }
      if (!commit || !target) return;
      const neighbors = dropNeighbors(listOf(bucket), id, target);
      if (neighbors) onMove(id, neighbors.above, neighbors.below);
    };
    const up = () => finish(true);
    const cancel = () => finish(false);
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") finish(false); };
    // 拖起来之后拦住页面滚动：指针事件的 preventDefault 管不了触屏滚动，得拦 touchmove，
    // 且必须是非被动监听（window 上默认被动，preventDefault 会被忽略）。到点前不拦，滚动照常。
    const touchMove = (e: TouchEvent) => { if (started && e.cancelable) e.preventDefault(); };
    if (touch) {
      touchHoldRef.current = true;
      holdTimer = window.setTimeout(() => {
        begin();
        setDropAt(null);
        // 拖起来了给一下震动反馈（Android 支持，iOS Safari 没有这个 API，静默跳过）。
        navigator.vibrate?.(10);
      }, LONG_PRESS_MS);
      window.addEventListener("touchmove", touchMove, { passive: false });
    }
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
    cleanupRef.current = () => {
      window.clearTimeout(holdTimer);
      window.removeEventListener("touchmove", touchMove);
      // contextmenu 在松手后才派发的情况也要拦住，留到下一拍再放开。
      if (touch) window.setTimeout(() => { touchHoldRef.current = false; }, 0);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      document.body.classList.remove("is-sidebar-dragging");
      ghost?.remove();
      ghost = null;
      cleanupRef.current = null;
    };
  };

  return {
    dragId,
    dropAt,
    onPointerDown,
    /// 点击处理前先问一句：刚拖完的那次 click 不算点击。
    swallowClick: () => suppressClickRef.current,
    /// contextmenu 处理前先问一句：触屏长按的菜单由 onHold 开，别再开一次。
    swallowContextMenu: () => touchHoldRef.current,
  };
}

/// 会话行的拖拽（data-session-id）。
export function useSessionDrag(options: {
  enabled: boolean;
  /// 会话所在的桶（同一分组 + 同为置顶 / 非置顶）。
  bucketOf: (id: number) => string | null;
  /// 桶里当前的显示顺序。
  listOf: (bucket: string) => readonly number[];
  onMove: (id: number, above: number | null, below: number | null) => void;
  onHold?: (id: number, x: number, y: number) => void;
}) {
  return usePointerReorder<number>({
    ...options,
    attr: "sessionId",
    parse: (raw) => { const id = Number(raw); return Number.isFinite(id) ? id : null; },
  });
}

/// 工作区组按手动顺序排（对照 Kimi 可拖的分组头）：排过的按 order 里的位置；没排过的（新工作区）
/// 放在最上面，保持原来的相对顺序——与新会话出现在最上面同一个规矩。keyOf 取组的归一路径键。
export function applyGroupOrder<T>(groups: readonly T[], order: readonly string[], keyOf: (group: T) => string): T[] {
  if (order.length === 0) return [...groups];
  const rank = new Map(order.map((key, index) => [key, index]));
  const fresh = groups.filter((group) => !rank.has(keyOf(group)));
  const known = groups.filter((group) => rank.has(keyOf(group)))
    .sort((a, b) => rank.get(keyOf(a))! - rank.get(keyOf(b))!);
  return [...fresh, ...known];
}

/** 无 cwd 的会话归到同一组（后端 cwd 可空:ping 型/早期数据）。以 NUL 开头，不可能与路径撞车。 */
export const NO_DIR = String.fromCharCode(0) + "no-dir";

export type DirGroup<T> = { key: string; cwd: string | null; label: string; items: T[] };

/**
 * 按 cwd 分组,**组序由已有排序派生**:组按「组内第一条会话的位置」排,组内保持原顺序。
 * 于是后端的 connected-first + 时间倒序原样透过来——正在跑的会话所在目录自然浮到最上面,
 * 用户原来靠什么找会话,分组后还靠什么找。另起一套组排序(按目录名/会话数)会打乱这一点。
 * 包 5:worktree 会话按 origin_root(主检出)归组(groupDirOf),和原项目的会话排在一起。
 */
export function groupByDir<T extends { cwd: string | null; origin_root?: string | null }>(items: readonly T[]): DirGroup<T>[] {
  const groups: DirGroup<T>[] = [];
  const index = new Map<string, DirGroup<T>>();
  for (const item of items) {
    const dir = groupDirOf(item);
    const key = dir ? pathKey(dir) : NO_DIR;
    let group = index.get(key);
    if (!group) {
      group = { key, cwd: dir, label: folderName(dir), items: [] };
      index.set(key, group);
      groups.push(group);
    }
    group.items.push(item);
  }
  return groups;
}

/** 工作区顺序存进设置用的键：pathKey 会按本机平台改斜杠方向，桌面（Windows）与手机算出来的
 *  不一样，存下来的顺序在另一端就对不上。统一成正斜杠再存、再比。 */
export function orderKey(groupKey: string): string {
  return groupKey.replace(/\\/g, "/");
}

/** 侧栏「按工作区分组」时的组序：手动排序时组序也归用户（对照 Kimi 可拖的分组头）；按最近活动时
 *  组序随组内会话的位置。侧栏和灵动岛共用这一份，岛上头像才和侧栏扁平下来的顺序一致。 */
export function sidebarDirGroups<T extends { cwd: string | null; origin_root?: string | null }>(
  ordered: readonly T[],
  sortMode: "manual" | "recent",
  workspaceOrder: readonly string[],
): DirGroup<T>[] {
  const groups = groupByDir(ordered);
  return sortMode === "manual" ? applyGroupOrder(groups, workspaceOrder, (group) => orderKey(group.key)) : groups;
}
