import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { pushEscLayer } from "../../escLayers";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icClose } from "../../icons/glyphs";

/** 大图查看层：滚轮缩放、拖拽平移、双击复位；手机上双指缩放、单指平移、双击在 1× 与 2.5× 之间切换。
 *  工具栏有缩放按钮与明显的关闭键。
 *  点击空白处或 Esc 也能关（拖拽松手不算点击）。
 *  actions：工具栏左段的额外按钮（agent 发来的图有「保存」「在文件夹中显示」），用同一套按钮皮。
 *  caption：底部的一句说明（发图时 agent 附的话），看大图时也知道这张图是干什么的。 */
export function Lightbox({ src, name, onClose, actions, caption }: {
  src: string;
  name: string;
  onClose: () => void;
  actions?: ReactNode;
  caption?: string | null;
}) {
  const t = useT();
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const stageRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number; moved: boolean } | null>(null);
  const justDraggedRef = useRef(false);
  // 桌面 Windows/Linux 的顶栏（拖拽区 + 最小化/最大化/关闭）留在灯箱外面：三颗窗口按钮上盖着原生覆盖子窗
  // （Windows 上，见 WindowControls.tsx 的 snap_layout），那片区域的点击进不了网页——工具栏压在那里时，
  // 点灯箱的「关闭」实际按到的是窗口的最大化或关闭。
  // 灯箱从顶栏下沿开始，窗口按钮照常可见可用。macOS（红绿灯在左、没有这条顶栏）与手机页量出来是 0。
  const [chromeTop, setChromeTop] = useState(measureChromeTop);
  useEffect(() => {
    const onResize = () => setChromeTop(measureChromeTop());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  const clampScale = (value: number) => Math.min(8, Math.max(0.2, value));
  const reset = () => { setScale(1); setOffset({ x: 0, y: 0 }); };

  // Esc 关灯箱。挂捕获段并拦住传播：ChatWindow 有一个全局 Esc=拒绝审批的监听，
  // 用户按 Esc 只想关图，不能顺手把审批拒了。同时注册 Esc 层（escLayers.ts）——
  // 层栈是全局监听的统一让位判据，捕获段截停是本层自己的双保险。
  useEffect(() => {
    const popLayer = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  // 滚轮缩放。React 的 onWheel 在根容器上是 passive 监听，preventDefault 无效
  // （页面会跟着滚）——自己挂非 passive 的原生监听。
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      setScale((current) => clampScale(current * (event.deltaY < 0 ? 1.15 : 1 / 1.15)));
    };
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => stage.removeEventListener("wheel", onWheel);
  }, []);

  // 指针（鼠标、手指）统一走 Pointer Events。舞台 touch-action: none，手势全归灯箱：原先只有鼠标事件，
  // 手机上双指缩放放大的是整个网页（leader 实拍，2026-11-22）。
  const pointersRef = useRef(new Map<number, { x: number; y: number }>());
  const pinchRef = useRef<{ dist: number; midX: number; midY: number; baseScale: number; baseX: number; baseY: number } | null>(null);
  const lastTapRef = useRef(0);
  const viewRef = useRef({ scale, offset });
  viewRef.current = { scale, offset };
  const startPinch = () => {
    const [a, b] = [...pointersRef.current.values()];
    pinchRef.current = { dist: Math.hypot(a.x - b.x, a.y - b.y) || 1, midX: (a.x + b.x) / 2, midY: (a.y + b.y) / 2, baseScale: viewRef.current.scale, baseX: viewRef.current.offset.x, baseY: viewRef.current.offset.y };
  };
  const startDrag = (x: number, y: number) => {
    dragRef.current = { startX: x, startY: y, baseX: viewRef.current.offset.x, baseY: viewRef.current.offset.y, moved: false };
  };
  // iOS Safari 的双指缩放另有一套私有 gesture 事件，touch-action 之外再拦一道，防整页被放大。
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const block = (event: Event) => event.preventDefault();
    stage.addEventListener("gesturestart", block);
    stage.addEventListener("gesturechange", block);
    return () => {
      stage.removeEventListener("gesturestart", block);
      stage.removeEventListener("gesturechange", block);
    };
  }, []);
  const onPointerDown = (event: ReactPointerEvent) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    if ((event.target as HTMLElement).closest?.("button")) return;
    if (event.pointerType === "mouse") event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    pointersRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (pointersRef.current.size === 2) {
      dragRef.current = null;
      justDraggedRef.current = true; // 双指之后抬手不是「点空白关闭」
      startPinch();
    } else if (pointersRef.current.size === 1) {
      justDraggedRef.current = false; // 新的一次手势：上一次手势留下的「别当点击」标记作废
      startDrag(event.clientX, event.clientY);
    }
  };
  const onPointerMove = (event: ReactPointerEvent) => {
    if (!pointersRef.current.has(event.pointerId)) return;
    pointersRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const pinch = pinchRef.current;
    if (pinch && pointersRef.current.size >= 2) {
      const [a, b] = [...pointersRef.current.values()];
      const dist = Math.hypot(a.x - b.x, a.y - b.y) || 1;
      setScale(clampScale(pinch.baseScale * (dist / pinch.dist)));
      setOffset({ x: pinch.baseX + (a.x + b.x) / 2 - pinch.midX, y: pinch.baseY + (a.y + b.y) / 2 - pinch.midY });
      return;
    }
    const drag = dragRef.current;
    if (!drag) return;
    const dx = event.clientX - drag.startX;
    const dy = event.clientY - drag.startY;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    setOffset({ x: drag.baseX + dx, y: drag.baseY + dy });
  };
  const onPointerUp = (event: ReactPointerEvent) => {
    if (!pointersRef.current.delete(event.pointerId)) return;
    if (pointersRef.current.size === 1) {
      // 双指松开一指：剩下那指接着平移，不跳。
      pinchRef.current = null;
      const [rest] = [...pointersRef.current.values()];
      startDrag(rest.x, rest.y);
      dragRef.current!.moved = true;
      return;
    }
    if (pointersRef.current.size > 0) return;
    pinchRef.current = null;
    const drag = dragRef.current;
    dragRef.current = null;
    if (drag?.moved) { justDraggedRef.current = true; return; }
    // 触屏双击：桌面靠 dblclick；触屏上 touch-action: none 之后 dblclick 不可靠，自己数两次轻点。
    if (event.pointerType !== "mouse") {
      const now = event.timeStamp;
      if (now - lastTapRef.current < 300) {
        lastTapRef.current = 0;
        justDraggedRef.current = true; // 第二下轻点不是「点空白关闭」
        if (viewRef.current.scale > 1.01) reset(); else setScale(2.5);
        return;
      }
      lastTapRef.current = now;
    }
  };
  const onStageClick = (event: ReactMouseEvent) => {
    // 拖拽平移松手不是「点空白关闭」；只有真正点在空白处才关。
    if (justDraggedRef.current) { justDraggedRef.current = false; return; }
    if (event.target === event.currentTarget) onClose();
  };

  return createPortal(
    <div className="chat-lightbox" role="dialog" aria-label={name} style={chromeTop > 0 ? { top: chromeTop } : undefined}>
      <div
        ref={stageRef}
        className={"chat-lightbox-stage" + (caption ? " has-caption" : "")}
        onClick={onStageClick}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={reset}
      >
        {/* no-referrer:远程模式下 src 是带 token 的 /file URL,不许经 Referer 外泄。 */}
        <img
          src={src}
          alt={name}
          draggable={false}
          referrerPolicy="no-referrer"
          style={{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})` }}
        />
      </div>
      <div className="chat-lightbox-bar">
        {actions}
        <button type="button" aria-label={t.chat.zoomOut} data-tip={t.chat.zoomOut} onClick={() => setScale((s) => clampScale(s / 1.25))}>−</button>
        <button type="button" className="chat-lightbox-zoom" aria-label={t.chat.zoomReset} data-tip={t.chat.zoomReset} onClick={reset}>{Math.round(scale * 100)}%</button>
        <button type="button" aria-label={t.chat.zoomIn} data-tip={t.chat.zoomIn} onClick={() => setScale((s) => clampScale(s * 1.25))}>+</button>
        <button type="button" aria-label={t.chat.lightboxClose} data-tip={t.chat.lightboxClose} onClick={onClose}>
          <Icon glyph={icClose} size={16} />
        </button>
      </div>
      {caption && <div className="chat-lightbox-caption">{caption}</div>}
    </div>,
    document.body,
  );
}

/** 窗口自带顶栏的下沿（没有顶栏时是 0）。 */
function measureChromeTop(): number {
  const bar = typeof document === "undefined" ? null : document.querySelector(".chat-topbar");
  return bar ? Math.max(0, Math.round(bar.getBoundingClientRect().bottom)) : 0;
}
