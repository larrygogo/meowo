/// 侧栏「手动排序」的拖拽（对照 Kimi Code 侧栏的手动排序）。
///
/// 用指针事件自己做，不用 HTML5 拖放：对话窗开着 Tauri 的原生文件拖放（拖文件进来当附件），
/// Windows 上 WebView2 会把网页内的 HTML5 拖拽一并截走，dragover/drop 根本到不了页面。
/// 指针事件不受它影响，鼠标和触控笔都能用。
///
/// 只能在同一个「桶」里拖：同一分组、同为置顶或同为非置顶。跨组拖没有意义（组由工作区 /
/// 日期 / 状态决定，不由位置决定），跨置顶边界拖了也会被置顶浮顶规则弹回去。

import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

export type DropAt = { id: number; after: boolean };

/// 落点 → 新位置的上下邻居。list 是拖动前这个桶里的显示顺序（含被拖的那条）。
/// above = 落下后紧挨在它上面的那条，below = 紧挨在下面的那条；落回原位返回 null（不用动）。
export function dropNeighbors(list: readonly number[], dragged: number, drop: DropAt): { above: number | null; below: number | null } | null {
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
export function moveNextTo<T>(items: readonly T[], idOf: (item: T) => number, id: number, above: number | null, below: number | null): T[] {
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

export function useSessionDrag({ enabled, bucketOf, listOf, onMove }: {
  enabled: boolean;
  /// 会话所在的桶（同一分组 + 同为置顶 / 非置顶）。
  bucketOf: (id: number) => string | null;
  /// 桶里当前的显示顺序。
  listOf: (bucket: string) => readonly number[];
  onMove: (id: number, above: number | null, below: number | null) => void;
}) {
  const [dragId, setDragId] = useState<number | null>(null);
  const [dropAt, setDropAt] = useState<DropAt | null>(null);
  // 刚拖完：吞掉松手后紧跟着的那次 click，不然拖完会顺手打开会话。
  const suppressClickRef = useRef(false);
  const cleanupRef = useRef<(() => void) | null>(null);
  useEffect(() => () => cleanupRef.current?.(), []);

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>, id: number) => {
    if (!enabled || event.button !== 0 || event.pointerType === "touch") return;
    const bucket = bucketOf(id);
    if (bucket == null) return;
    const startX = event.clientX;
    const startY = event.clientY;
    let started = false;
    let target: DropAt | null = null;
    const pick = (x: number, y: number): DropAt | null => {
      const row = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-session-id]");
      const over = row ? Number(row.dataset.sessionId) : NaN;
      if (!row || !Number.isFinite(over) || bucketOf(over) !== bucket) return null;
      const rect = row.getBoundingClientRect();
      return { id: over, after: y > rect.top + rect.height / 2 };
    };
    const move = (e: PointerEvent) => {
      if (!started) {
        if (Math.abs(e.clientY - startY) < DRAG_THRESHOLD && Math.abs(e.clientX - startX) < DRAG_THRESHOLD) return;
        started = true;
        setDragId(id);
        document.body.classList.add("is-sidebar-dragging");
      }
      e.preventDefault();
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
      if (!commit || !target) return;
      const neighbors = dropNeighbors(listOf(bucket), id, target);
      if (neighbors) onMove(id, neighbors.above, neighbors.below);
    };
    const up = () => finish(true);
    const cancel = () => finish(false);
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") finish(false); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
    cleanupRef.current = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      document.body.classList.remove("is-sidebar-dragging");
      cleanupRef.current = null;
    };
  };

  return {
    dragId,
    dropAt,
    onPointerDown,
    /// 点击处理前先问一句：刚拖完的那次 click 不算点击。
    swallowClick: () => suppressClickRef.current,
  };
}
