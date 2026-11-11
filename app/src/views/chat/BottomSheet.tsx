/// 手机上的底部面板（对照 Kimi 的 BottomSheet：斜杠命令、@文件、模型/模式切换在手机上都用它）。
///
/// 为什么手机上不用贴着输入框向上弹的菜单：键盘弹起后输入框上方只剩一两百像素，菜单要么
/// 被压成两三行，要么顶进标题栏底下。底部面板从下往上铺、最高到可视区的 86%，键盘问题
/// 直接不存在；手指够得着、行高按触控来。桌面不用它——那里锚定菜单更快、不挡内容。

import { useEffect, useRef, type ReactNode } from "react";
import { pushEscLayer } from "../../escLayers";
import { useT } from "../../i18n";

/** 下滑关闭的判定：拖过这么远，或松手时往下甩得够快（px/ms），就关。 */
const DISMISS_DISTANCE = 90;
const DISMISS_VELOCITY = 0.5;

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function BottomSheet({ title, onClose, children, ownsEscape = true }: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** 自己注册 Esc 层并处理 Esc。挂在已有菜单（useMenuPopup）里时由菜单负责，传 false。 */
  ownsEscape?: boolean;
}) {
  const t = useT();
  const panelRef = useRef<HTMLDivElement>(null);
  const scrimRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!ownsEscape) return;
    const popLayer = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey);
    };
  }, [ownsEscape, onClose]);

  // 下滑关闭（用户反馈：抽屉形态却不能下滑关，只能点小横条或遮罩）。手感对齐 iOS 原生
  // 抽屉：从横条/标题拖，面板跟手下移；从列表里拖，只有列表已在顶部时才接管——否则
  // 是在滚列表。松手过阈值（距离或速度）即关，不过就弹回。用原生 touch 监听而不是
  // React 的：接管拖动时要 preventDefault 掉列表的橡皮筋回弹，React 的 touchmove 是被动监听。
  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    let startY = 0;
    let lastY = 0;
    let lastT = 0;
    let velocity = 0;
    let tracking = false;
    let dragging = false;
    // 松手后的回弹/滑出动画交给 CSS 的 .is-settling（时长、曲线走 --dur-slow / --ease-out，减少动效时
    // 那里直接关掉过渡）。原先这里内联写死 ".22s cubic-bezier(…)"，减少动效的用户照样看到面板滑走。
    const setOffset = (dy: number, animate: boolean) => {
      panel.classList.toggle("is-settling", animate);
      panel.style.transform = dy > 0 ? `translateY(${dy}px)` : "";
      const scrim = scrimRef.current;
      if (scrim) {
        scrim.classList.toggle("is-settling", animate);
        scrim.style.opacity = dy > 0 ? String(Math.max(0, 1 - dy / Math.max(1, panel.offsetHeight))) : "";
      }
    };
    const onStart = (event: TouchEvent) => {
      if (event.touches.length !== 1) return;
      const body = bodyRef.current;
      const inBody = !!body && body.contains(event.target as Node);
      // 列表没在顶部：这一下是滚列表，不接管。
      tracking = !inBody || (body?.scrollTop ?? 0) <= 0;
      dragging = false;
      startY = lastY = event.touches[0].clientY;
      lastT = event.timeStamp;
      velocity = 0;
    };
    const onMove = (event: TouchEvent) => {
      if (!tracking) return;
      const y = event.touches[0].clientY;
      const dy = y - startY;
      if (!dragging) {
        // 先往上推的是在滚列表/拉长内容：放弃接管，整个手势都不再管。
        if (dy < -4) { tracking = false; return; }
        if (dy < 6) return;
        dragging = true;
      }
      event.preventDefault();
      // 间隔按至少一帧算：同一帧里连到的两次 move 会把速度算成无穷大，短拖也被当成一甩。
      velocity = (y - lastY) / Math.max(16, event.timeStamp - lastT);
      lastY = y;
      lastT = event.timeStamp;
      setOffset(Math.max(0, dy), false);
    };
    const onEnd = () => {
      if (!dragging) { tracking = false; return; }
      const dy = lastY - startY;
      tracking = dragging = false;
      const reduced = prefersReducedMotion();
      if (dy > DISMISS_DISTANCE || velocity > DISMISS_VELOCITY) {
        // 减少动效：不做滑出位移，直接关。
        if (reduced) { onCloseRef.current(); return; }
        setOffset(panel.offsetHeight, true);
        window.setTimeout(() => onCloseRef.current(), 180);
      } else {
        setOffset(0, !reduced);
      }
    };
    panel.addEventListener("touchstart", onStart, { passive: true });
    panel.addEventListener("touchmove", onMove, { passive: false });
    panel.addEventListener("touchend", onEnd);
    panel.addEventListener("touchcancel", onEnd);
    return () => {
      panel.removeEventListener("touchstart", onStart);
      panel.removeEventListener("touchmove", onMove);
      panel.removeEventListener("touchend", onEnd);
      panel.removeEventListener("touchcancel", onEnd);
    };
  }, []);

  return (
    <div className="chat-sheet">
      <div className="chat-sheet-scrim" ref={scrimRef} onClick={onClose} />
      <div className="chat-sheet-panel" role="dialog" aria-label={title} ref={panelRef}>
        <button type="button" className="chat-sheet-grab" aria-label={t.chat.close} onClick={onClose} />
        <div className="chat-sheet-head"><span className="chat-sheet-title">{title}</span></div>
        <div className="chat-sheet-body" ref={bodyRef}>{children}</div>
      </div>
    </div>
  );
}

/// 手机上包成底部面板、桌面原样返回内容——同一份菜单内容，两种呈现。
export function SheetOnPhone({ phone, title, onClose, ownsEscape, children }: {
  phone: boolean;
  title: string;
  onClose: () => void;
  ownsEscape?: boolean;
  children: ReactNode;
}) {
  if (!phone) return <>{children}</>;
  return <BottomSheet title={title} onClose={onClose} ownsEscape={ownsEscape}>{children}</BottomSheet>;
}
