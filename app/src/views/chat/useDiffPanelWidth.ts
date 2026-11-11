/// 改动 / 文件停靠面板的宽度：拖拽分栏柄与键盘 ← → 调宽，宽度持久化到 localStorage。
///
/// 从 ChatWindow 拆出：这是一段只和分栏柄、面板宽度打交道的指针事件处理，和对话本身无关。

import { CHAT_MIN_WIDTH, panelDefaultWidth, PANEL_MIN_WIDTH, siblingWidth } from "./paneLayout";
import { useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";

/** 改动/文件停靠面板的宽度持久化键（localStorage 既有 meowo-* 命名惯例）。 */
const DIFF_WIDTH_KEY = "meowo-diff-panel-width";
// 照 Kimi Code：面板最窄 320，对话区至少留 320（常驻侧栏的宽也要扣掉，见 paneLayout）。
export const DIFF_PANEL_MIN_WIDTH = PANEL_MIN_WIDTH;
/** 没调过宽度时的默认值，挂载时 .chat-body 还没量，按窗口估（见 paneLayout.panelDefaultWidth）。 */
const defaultWidth = () => panelDefaultWidth(window.innerWidth);
/** 键盘调宽的步进。 */
const KEY_STEP = 24;

/** 拖拽 / 键盘调宽的上限：.chat-body 宽减常驻侧栏、再给对话区留 CHAT_MIN_WIDTH。
 *  此前没扣侧栏，1440 宽侧栏常驻时能把对话区压到 80px（调研 c4）。 */
const maxWidthIn = (body: Element | null) =>
  body
    ? Math.max(DIFF_PANEL_MIN_WIDTH, body.getBoundingClientRect().width - siblingWidth(body, ".chat-sidebar") - CHAT_MIN_WIDTH)
    : Number.MAX_SAFE_INTEGER;
const persist = (width: number) => {
  try { localStorage.setItem(DIFF_WIDTH_KEY, String(Math.round(width))); } catch { /* 存不下就只在内存里 */ }
};

export function useDiffPanelWidth() {
  // 面板宽度随会话持久化；折叠只藏不改宽，再展开原宽回来。
  const [width, setWidth] = useState(() => {
    const stored = Number(localStorage.getItem(DIFF_WIDTH_KEY));
    return Number.isFinite(stored) && stored >= DIFF_PANEL_MIN_WIDTH ? stored : defaultWidth();
  });

  // 拖拽分栏柄：pointer capture 挂在柄上，移出窗口也不丢事件；上限见 maxWidthIn。
  // 窗口缩小时的回收不在这里：ChatWindow 渲染时按实测宽再钳一次（paneLayout.fitPanes）。
  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const handle = event.currentTarget;
    const body = handle.parentElement;
    if (!body) return;
    event.preventDefault();
    handle.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const panel = body.querySelector<HTMLElement>(".chat-diff-panel");
    // 以实测宽为起点：画出来的宽可能已被 fitPanes 压小，从 state 起算会跳变。
    const startWidth = panel ? panel.getBoundingClientRect().width : width;
    const maxWidth = maxWidthIn(body);
    let lastWidth = startWidth;
    body.classList.add("is-diff-resizing");
    // pointermove 每次都 setState 会以指针事件频率重渲整个面板子树，开着大文件时拖不动；
    // 按帧合流，一帧至多提交一次宽度。
    let raf = 0;
    const onMove = (move: PointerEvent) => {
      lastWidth = Math.min(Math.max(startWidth + (startX - move.clientX), DIFF_PANEL_MIN_WIDTH), maxWidth);
      if (!raf) {
        raf = requestAnimationFrame(() => {
          raf = 0;
          setWidth(lastWidth);
        });
      }
    };
    const onUp = () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      setWidth(lastWidth);
      body.classList.remove("is-diff-resizing");
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
      handle.removeEventListener("pointercancel", onUp);
      persist(lastWidth);
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
    handle.addEventListener("pointercancel", onUp);
  };

  // separator 是 WAI-ARIA 的可聚焦部件：只给鼠标拖拽等于键盘用户调不了宽度。
  // ← 变宽、→ 变窄（柄在面板左侧，与拖拽方向一致）。
  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = Math.min(Math.max(width + (event.key === "ArrowLeft" ? KEY_STEP : -KEY_STEP), DIFF_PANEL_MIN_WIDTH), maxWidthIn(event.currentTarget.parentElement));
    setWidth(next);
    persist(next);
  };

  // 双击分栏柄恢复默认宽（VS Code / Kimi 同款）：拖坏了不必再一点点拖回来。
  const reset = () => {
    try { localStorage.removeItem(DIFF_WIDTH_KEY); } catch { /* 同上 */ }
    setWidth(defaultWidth());
  };

  return { width, onPointerDown, onKeyDown, reset };
}
