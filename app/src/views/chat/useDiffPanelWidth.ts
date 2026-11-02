/// 改动 / 文件停靠面板的宽度：拖拽分栏柄与键盘 ← → 调宽，宽度持久化到 localStorage。
///
/// 从 ChatWindow 拆出：这是一段只和分栏柄、面板宽度打交道的指针事件处理，和对话本身无关。

import { useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";

/** 改动/文件停靠面板的宽度持久化键（localStorage 既有 meowo-* 命名惯例）。 */
const DIFF_WIDTH_KEY = "meowo-diff-panel-width";
/** 面板宽度下限保面板可用；上限按 .chat-body 实测宽给对话区留 CHAT_MIN_WIDTH。 */
export const DIFF_PANEL_MIN_WIDTH = 280;
const DIFF_PANEL_DEFAULT_WIDTH = 400;
const CHAT_MIN_WIDTH = 360;
/** 键盘调宽的步进。 */
const KEY_STEP = 24;

const maxWidthIn = (body: Element | null) =>
  body ? Math.max(DIFF_PANEL_MIN_WIDTH, body.getBoundingClientRect().width - CHAT_MIN_WIDTH) : Number.MAX_SAFE_INTEGER;
const persist = (width: number) => {
  try { localStorage.setItem(DIFF_WIDTH_KEY, String(Math.round(width))); } catch { /* 存不下就只在内存里 */ }
};

export function useDiffPanelWidth() {
  // 面板宽度随会话持久化；折叠只藏不改宽，再展开原宽回来。
  const [width, setWidth] = useState(() => {
    const stored = Number(localStorage.getItem(DIFF_WIDTH_KEY));
    return Number.isFinite(stored) && stored >= DIFF_PANEL_MIN_WIDTH ? stored : DIFF_PANEL_DEFAULT_WIDTH;
  });

  // 拖拽分栏柄：pointer capture 挂在柄上，移出窗口也不丢事件；宽度上限按
  // .chat-body 实测宽给对话区留 ≥360px（CSS 的 max-width:40vw 仍是窗口缩小的兜底）。
  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const handle = event.currentTarget;
    const body = handle.parentElement;
    if (!body) return;
    event.preventDefault();
    handle.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const panel = body.querySelector<HTMLElement>(".chat-diff-panel");
    // 以实测宽为起点：state 宽可能被 40vw 上限压小，从 state 起算会跳变。
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

  return { width, onPointerDown, onKeyDown };
}
