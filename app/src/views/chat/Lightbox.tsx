import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { pushEscLayer } from "../../escLayers";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icCheck, icClose } from "../../icons/glyphs";
// 右箭头用左箭头镜像（CSS），不从 glyphs.scm 引：那一档不许被拖进对话页的 chunk（icons.test）。
import { icChevronLeft, icCopy } from "../../icons/glyphs.chat";
import { safeFileSrc } from "../../assetSrc";

/** 大图查看层里的一张图。path 有值时可保存、在文件夹中显示（由调用方的 renderActions 决定）。 */
export type GalleryItem = { src: string; name: string; caption?: string | null; path?: string | null };

/// 相册（2026-11-22，图片模式打磨）：点开一张图时，把同一个对话里所有能点开的图按页面顺序收成一组，左右切换。
/// 不另建登记表：每个打开按钮带 data-gallery-* 属性，打开时从对话滚动区里按 DOM 顺序收集——顺序天然与时间线一致，
/// 加载失败、还没解析出来的图本来就没有这个属性，不会混进来。
export function galleryAttrs(item: { path?: string | null; src?: string | null; name: string; caption?: string | null }): Record<string, string> {
  const attrs: Record<string, string> = { "data-gallery-name": item.name };
  if (item.path) attrs["data-gallery-path"] = item.path;
  if (item.src) attrs["data-gallery-src"] = item.src;
  if (item.caption) attrs["data-gallery-caption"] = item.caption;
  return attrs;
}

export function collectGallery(opener: HTMLElement | null, self: GalleryItem): { items: GalleryItem[]; index: number } {
  const root = opener?.closest(".chat-scroll") ?? null;
  if (!opener || !root) return { items: [self], index: 0 };
  const items: GalleryItem[] = [];
  let index = -1;
  for (const el of root.querySelectorAll<HTMLElement>("[data-gallery-name]")) {
    if (el.closest(".chat-lightbox")) continue;
    const path = el.dataset.galleryPath ?? null;
    const src = el.dataset.gallerySrc ?? (path ? safeFileSrc(path) : null);
    if (!src) continue;
    if (el === opener) index = items.length;
    items.push({ src, name: el.dataset.galleryName ?? "", caption: el.dataset.galleryCaption ?? null, path });
  }
  if (index < 0) return { items: [self], index: 0 };
  items[index] = { ...items[index], src: self.src };
  return { items, index };
}

const MIN_SCALE = 0.2;
const MAX_SCALE = 8;
const clampScale = (value: number) => Math.min(MAX_SCALE, Math.max(MIN_SCALE, value));
const SWIPE_PX = 60;
const DISMISS_PX = 110;

/** 大图查看层：沉浸式（对照 Telegram、iOS 照片）。图是主角，界面只在上下两条渐变里。
 *  - 多张图：左右箭头、← →、手机左右滑切换，顶部「2 / 5」，手机底部圆点。
 *  - 缩放：读数按原图像素算（适应窗口时写「适应 · 70%」，不再把缩小显示写成 100%）；滚轮与双击以鼠标位置为中心；
 *    手机双指缩放、单指平移，轻点两下在适应与放大之间切换。
 *  - 手机轻点图片切换界面显隐；在适应大小时下滑关闭。
 *  - 复制图片（Ctrl+C），保存、在文件夹中显示由调用方给（renderActions）。
 *  点击空白处或 Esc 关闭（拖拽松手不算点击）。 */
export function Lightbox({ items, index: startIndex = 0, onClose, renderActions }: {
  items: GalleryItem[];
  index?: number;
  onClose: () => void;
  renderActions?: (item: GalleryItem) => ReactNode;
}) {
  const t = useT();
  const [index, setIndex] = useState(() => Math.min(Math.max(0, startIndex), Math.max(0, items.length - 1)));
  const item = items[index] ?? items[0];
  const multi = items.length > 1;
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [fitRatio, setFitRatio] = useState<number | null>(null);
  const [chromeHidden, setChromeHidden] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "done" | "failed">("idle");
  const [dragY, setDragY] = useState(0);
  const stageRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number; moved: boolean; axis: "x" | "y" | null } | null>(null);
  const justDraggedRef = useRef(false);
  const pointersRef = useRef(new Map<number, { x: number; y: number }>());
  const pinchRef = useRef<{ dist: number; midX: number; midY: number; baseScale: number; baseX: number; baseY: number } | null>(null);
  const lastTapRef = useRef(0);
  const tapTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const viewRef = useRef({ scale, offset });
  viewRef.current = { scale, offset };
  // 桌面 Windows/Linux 的顶栏（拖拽区 + 最小化/最大化/关闭）留在灯箱外面：三颗窗口按钮上盖着原生覆盖子窗
  // （Windows 上，见 WindowControls.tsx 的 snap_layout），那片区域的点击进不了网页——工具栏压在那里时，
  // 点灯箱的「关闭」实际按到的是窗口的最大化或关闭。
  // 灯箱从顶栏下沿开始，窗口按钮照常可见可用。macOS（红绿灯在左、没有这条顶栏）与手机页量出来是 0。
  const [chromeTop, setChromeTop] = useState(measureChromeTop);
  useEffect(() => {
    const onResize = () => { setChromeTop(measureChromeTop()); measureFit(); };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
    // measureFit 只读 ref、只调 setState，首次的闭包一直有效。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const reset = useCallback(() => { setScale(1); setOffset({ x: 0, y: 0 }); }, []);
  const go = useCallback((delta: number) => {
    if (items.length < 2) return;
    setIndex((current) => (current + delta + items.length) % items.length);
    setScale(1);
    setOffset({ x: 0, y: 0 });
    setFitRatio(null);
    setCopyState("idle");
  }, [items.length]);

  /** 适应大小时，屏幕上的宽 ÷ 原图宽。transform 不影响 clientWidth，任何缩放下量出来都是适应时的尺寸。 */
  const measureFit = () => {
    const img = imgRef.current;
    if (!img || !img.naturalWidth || !img.clientWidth) return;
    setFitRatio(img.clientWidth / img.naturalWidth);
  };

  /** 以舞台里的某一点为中心缩放：那一点下的像素缩放前后不动（macOS 预览、Figma 的做法）。 */
  const zoomAt = useCallback((next: number, clientX?: number, clientY?: number) => {
    const { scale: current, offset: base } = viewRef.current;
    const target = clampScale(next);
    const stage = stageRef.current;
    if (clientX === undefined || clientY === undefined || !stage) {
      setScale(target);
      return;
    }
    const rect = stage.getBoundingClientRect();
    const px = clientX - (rect.left + rect.width / 2);
    const py = clientY - (rect.top + rect.height / 2);
    const k = target / current;
    setScale(target);
    setOffset({ x: px - (px - base.x) * k, y: py - (py - base.y) * k });
  }, []);

  /** 双击/轻点两下：适应 ⇄ 原图 1:1（小图原本就不放大的，放大到 2.5 倍）。 */
  const toggleZoom = (clientX?: number, clientY?: number) => {
    if (viewRef.current.scale > 1.01) { reset(); return; }
    const oneToOne = fitRatio && fitRatio < 0.95 ? 1 / fitRatio : 2.5;
    zoomAt(oneToOne, clientX, clientY);
  };

  const copy = useCallback(async () => {
    try {
      await copyImageToClipboard(item.src);
      setCopyState("done");
    } catch {
      setCopyState("failed");
    }
  }, [item.src]);
  useEffect(() => {
    if (copyState === "idle") return;
    const timer = setTimeout(() => setCopyState("idle"), 1800);
    return () => clearTimeout(timer);
  }, [copyState]);

  // 键盘：Esc 关、← → 切换、+ − 0 缩放、Ctrl/⌘+C 复制图片。挂捕获段并拦住传播：ChatWindow 有一个全局 Esc=拒绝审批的
  // 监听，用户按 Esc 只想关图，不能顺手把审批拒了。同时注册 Esc 层（escLayers.ts）。
  useEffect(() => {
    const popLayer = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      const handled = (() => {
        if (event.key === "Escape") { onClose(); return true; }
        if (event.key === "ArrowLeft") { go(-1); return multi; }
        if (event.key === "ArrowRight") { go(1); return multi; }
        if (event.key === "+" || event.key === "=") { zoomAt(viewRef.current.scale * 1.25); return true; }
        if (event.key === "-") { zoomAt(viewRef.current.scale / 1.25); return true; }
        if (event.key === "0") { reset(); return true; }
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "c" && !window.getSelection()?.toString() && canCopyImage()) { void copy(); return true; }
        return false;
      })();
      if (!handled) return;
      event.preventDefault();
      event.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose, go, multi, zoomAt, reset, copy]);

  // 滚轮缩放，以鼠标位置为中心。React 的 onWheel 在根容器上是 passive 监听，preventDefault 无效
  // （页面会跟着滚）——自己挂非 passive 的原生监听。
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      zoomAt(viewRef.current.scale * (event.deltaY < 0 ? 1.15 : 1 / 1.15), event.clientX, event.clientY);
    };
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => stage.removeEventListener("wheel", onWheel);
  }, [zoomAt]);

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
  useEffect(() => () => { if (tapTimerRef.current) clearTimeout(tapTimerRef.current); }, []);

  // 指针（鼠标、手指）统一走 Pointer Events。舞台 touch-action: none，手势全归灯箱：原先只有鼠标事件，
  // 手机上双指缩放放大的是整个网页（leader 实拍，2026-11-22）。
  const startPinch = () => {
    const [a, b] = [...pointersRef.current.values()];
    pinchRef.current = { dist: Math.hypot(a.x - b.x, a.y - b.y) || 1, midX: (a.x + b.x) / 2, midY: (a.y + b.y) / 2, baseScale: viewRef.current.scale, baseX: viewRef.current.offset.x, baseY: viewRef.current.offset.y };
  };
  const startDrag = (x: number, y: number) => {
    dragRef.current = { startX: x, startY: y, baseX: viewRef.current.offset.x, baseY: viewRef.current.offset.y, moved: false, axis: null };
  };
  const onPointerDown = (event: ReactPointerEvent) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    if ((event.target as HTMLElement).closest?.("button")) return;
    if (event.pointerType === "mouse") event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    pointersRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (pointersRef.current.size === 2) {
      dragRef.current = null;
      setDragY(0);
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
    // 适应大小时的触屏单指：横向是翻页、竖向下拉是关闭，跟手走；放大后才是平移。
    if (event.pointerType !== "mouse" && viewRef.current.scale <= 1.01) {
      if (!drag.axis && Math.abs(dx) + Math.abs(dy) > 8) drag.axis = Math.abs(dx) > Math.abs(dy) ? "x" : "y";
      if (drag.axis === "x") { setOffset({ x: multi ? dx : dx / 4, y: 0 }); return; }
      if (drag.axis === "y") { setDragY(Math.max(0, dy)); setOffset({ x: 0, y: Math.max(0, dy) }); return; }
      return;
    }
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
    if (viewRef.current.scale < 1) reset(); // 捏得比适应还小，松手回弹
    const drag = dragRef.current;
    dragRef.current = null;
    if (drag?.axis) {
      const dx = event.clientX - drag.startX;
      const dy = event.clientY - drag.startY;
      justDraggedRef.current = true;
      setDragY(0);
      if (drag.axis === "x" && multi && Math.abs(dx) > SWIPE_PX) { go(dx < 0 ? 1 : -1); return; }
      if (drag.axis === "y" && dy > DISMISS_PX) { onClose(); return; }
      setOffset({ x: 0, y: 0 });
      return;
    }
    if (drag?.moved) { justDraggedRef.current = true; return; }
    if (event.pointerType === "mouse") return;
    // 触屏轻点：两下在适应与放大之间切换；一下切换界面显隐（等一拍确认不是双击）。
    // touch-action: none 之后 dblclick 不可靠，自己数。
    justDraggedRef.current = true; // 触屏轻点不走「点空白关闭」，关闭用关闭键或下滑
    const now = event.timeStamp;
    const { clientX, clientY } = event;
    if (now - lastTapRef.current < 300) {
      lastTapRef.current = 0;
      if (tapTimerRef.current) clearTimeout(tapTimerRef.current);
      tapTimerRef.current = null;
      toggleZoom(clientX, clientY);
      return;
    }
    lastTapRef.current = now;
    if (tapTimerRef.current) clearTimeout(tapTimerRef.current);
    tapTimerRef.current = setTimeout(() => { tapTimerRef.current = null; setChromeHidden((hidden) => !hidden); }, 300);
  };
  const onStageClick = (event: ReactMouseEvent) => {
    // 拖拽平移松手不是「点空白关闭」；只有真正点在空白处才关。
    if (justDraggedRef.current) { justDraggedRef.current = false; return; }
    if (event.target === event.currentTarget) onClose();
  };

  // 读数按原图像素算；图还没量到尺寸时先按适应大小算。
  const zoomPct = Math.round((fitRatio ?? 1) * scale * 100);
  const fitted = Math.abs(scale - 1) < 0.01;
  const zoomLabel = fitted && fitRatio !== null && fitRatio < 0.995 ? `${t.chat.zoomFit} · ${zoomPct}%` : `${zoomPct}%`;
  const dismissFade = dragY > 0 ? Math.max(0.35, 1 - dragY / 400) : 1;
  const copyable = canCopyImage();
  const copyLabel = copyState === "done" ? t.chat.imageCopied : copyState === "failed" ? t.chat.imageCopyFailed : t.chat.imageCopy;

  return createPortal(
    <div className="chat-lightbox" role="dialog" aria-label={item.name} data-chrome={chromeHidden ? "hidden" : undefined}
      style={{ ...(chromeTop > 0 ? { top: chromeTop } : {}), ...(dismissFade < 1 ? { background: `rgba(8, 8, 10, ${0.94 * dismissFade})` } : {}) }}>
      <div
        ref={stageRef}
        className={"chat-lightbox-stage" + (item.caption ? " has-caption" : "")}
        onClick={onStageClick}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={(event) => toggleZoom(event.clientX, event.clientY)}
      >
        {/* no-referrer:远程模式下 src 是带 token 的 /file URL,不许经 Referer 外泄。key 换图时重挂，不沿用上一张的尺寸。 */}
        <img
          key={item.src}
          ref={imgRef}
          src={item.src}
          alt={item.name}
          draggable={false}
          referrerPolicy="no-referrer"
          onLoad={measureFit}
          style={{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})` }}
        />
      </div>
      <div className="chat-lightbox-top">
        {multi && <span className="chat-lightbox-count" aria-live="polite">{index + 1} / {items.length}</span>}
        <span className="chat-lightbox-name">{item.name}</span>
        <div className="chat-lightbox-bar">
          {copyable && (
            <button type="button" className="chat-lightbox-action" aria-label={t.chat.imageCopy} data-tip={t.chat.imageCopy} onClick={() => void copy()} data-state={copyState}>
              <Icon glyph={copyState === "done" ? icCheck : icCopy} size={14} />
              <span>{copyLabel}</span>
            </button>
          )}
          {renderActions?.(item)}
          <button type="button" className="chat-lightbox-close" aria-label={t.chat.lightboxClose} data-tip={t.chat.lightboxClose} onClick={onClose}>
            <Icon glyph={icClose} size={16} />
          </button>
        </div>
      </div>
      {multi && (
        <>
          <button type="button" className="chat-lightbox-nav is-prev" aria-label={t.chat.lightboxPrev} data-tip={t.chat.lightboxPrev} onClick={() => go(-1)}>
            <Icon glyph={icChevronLeft} size={20} />
          </button>
          <button type="button" className="chat-lightbox-nav is-next" aria-label={t.chat.lightboxNext} data-tip={t.chat.lightboxNext} onClick={() => go(1)}>
            <Icon glyph={icChevronLeft} size={20} className="is-mirrored" />
          </button>
        </>
      )}
      <div className="chat-lightbox-bottom">
        {item.caption && <div className="chat-lightbox-caption">{item.caption}</div>}
        {multi && items.length <= 12 && (
          <div className="chat-lightbox-dots" aria-hidden="true">
            {items.map((entry, i) => <span key={`${i}:${entry.src}`} data-current={i === index || undefined} />)}
          </div>
        )}
        <div className="chat-lightbox-zoombar">
          <button type="button" aria-label={t.chat.zoomOut} data-tip={t.chat.zoomOut} onClick={() => zoomAt(scale / 1.25)}>−</button>
          <button type="button" className="chat-lightbox-zoom" aria-label={t.chat.zoomReset} data-tip={t.chat.zoomReset} onClick={reset}>{zoomLabel}</button>
          <button type="button" aria-label={t.chat.zoomIn} data-tip={t.chat.zoomIn} onClick={() => zoomAt(scale * 1.25)}>+</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

/** 窗口自带顶栏的下沿（没有顶栏时是 0）。 */
function measureChromeTop(): number {
  const bar = typeof document === "undefined" ? null : document.querySelector(".chat-topbar");
  return bar ? Math.max(0, Math.round(bar.getBoundingClientRect().bottom)) : 0;
}

/** 剪贴板写图要 ClipboardItem + 安全上下文：桌面 WebView2/WKWebView 有；手机远程页走局域网 http 时没有，按钮不出现。 */
export function canCopyImage(): boolean {
  return typeof window !== "undefined" && typeof window.ClipboardItem === "function" && !!navigator.clipboard?.write && window.isSecureContext !== false;
}

/** 复制图片：剪贴板只认 PNG，其他格式（JPEG、WebP、GIF 首帧）先画到 canvas 转成 PNG。 */
async function copyImageToClipboard(src: string): Promise<void> {
  const blob = await fetch(src).then((response) => {
    if (!response.ok) throw new Error(String(response.status));
    return response.blob();
  });
  const png = blob.type === "image/png" ? blob : await toPng(blob);
  await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
}

async function toPng(blob: Blob): Promise<Blob> {
  const bitmap = await createImageBitmap(blob);
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext("2d")?.drawImage(bitmap, 0, 0);
  bitmap.close();
  return new Promise((resolve, reject) => canvas.toBlob((out) => (out ? resolve(out) : reject(new Error("png"))), "image/png"));
}
