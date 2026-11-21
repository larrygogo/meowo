// 改前改后对照卡（Meowo 的 MCP 工具 compare_images）：像发来一张图那样排在正文里，一张图的位置放两张，
// 拖动中间的把手左右比较；红框圈出变化范围；切到「差异」看标红的像素。对照 GitHub 图片 diff 的 swipe 视图、
// Chromatic 的变化标注。方案对照见 .lead/20261122-image-tools/decisions.md。
//
// 后端回执（chat_images.rs compare_receipt）：依次是改前、改后、（有变化时）差异图的引用行，最后一行
// `[Image compare: {JSON}]` 是变化比例、两张图尺寸、变化范围和说明。界面只认统一工具名，不按 Provider 分支。
import { memo, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { isRemotePath, safeFileSrc } from "../../assetSrc";
import { useT } from "../../i18n";
import { fileNameOf, ImageFailure, imageErrorText, lightboxActions, sentImageReceipt, useImageLoad, useInView } from "./ChatImage";
import { Lightbox, type GalleryItem } from "./Lightbox";
import { useToolRunState } from "./toolState";
import type { ToolResultItem, ToolUseItem } from "./shared";

export type CompareSummary = {
  changed: string;
  before: [number, number];
  after: [number, number];
  bounds: [number, number, number, number] | null;
  caption: string | null;
};

const MARKER = "[Image compare: ";

/** 从回执里取对照：三张图（差异图可能没有）和摘要。不是对照回执（或摘要读不出来）返回 null。 */
export function parseCompareReceipt(text: string): { before: string; after: string; diff: string | null; summary: CompareSummary } | null {
  const line = text.split("\n").map((entry) => entry.trim()).reverse().find((entry) => entry.startsWith(MARKER) && entry.endsWith("]"));
  if (!line) return null;
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(line.slice(MARKER.length, -1)) as Record<string, unknown>;
  } catch {
    return null;
  }
  const pair = (value: unknown): [number, number] | null =>
    Array.isArray(value) && value.length === 2 && value.every((n) => typeof n === "number" && n > 0) ? [value[0], value[1]] : null;
  const before = pair(raw.before);
  const after = pair(raw.after);
  if (!before || !after) return null;
  const bounds = Array.isArray(raw.bounds) && raw.bounds.length === 4 && raw.bounds.every((n) => typeof n === "number")
    ? raw.bounds as [number, number, number, number]
    : null;
  const images = [...text.matchAll(/\[Image(?: #\d+)?: source: ([^\]]+?)\]/g)].map((match) => match[1].trim());
  if (images.length < 2) return null;
  return {
    before: images[0],
    after: images[1],
    diff: images[2] ?? null,
    summary: {
      changed: typeof raw.changed === "string" ? raw.changed : "",
      before,
      after,
      bounds,
      caption: typeof raw.caption === "string" && raw.caption.trim() ? raw.caption.trim() : null,
    },
  };
}

/** 拖动多远才算拖（像素）。短于它的按下抬起是「点一下看大图」。 */
const DRAG_PX = 4;

export const CompareMessage = memo(function CompareMessage({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const state = useToolRunState(result);
  const parsed = useMemo(() => (result && !result.is_error ? parseCompareReceipt(result.text) : null), [result]);
  let content;
  if (state === "running" || state === "waiting") {
    content = (
      <div className="chat-sent-pending" role="status">
        <span className="chat-model-spinner" aria-hidden="true" />
        {t.chat.comparing}
      </div>
    );
  } else if (state === "interrupted") {
    content = <ImageFailure title={t.chat.compareFailed} reason={t.chat.interrupted} name={null} />;
  } else if (!parsed) {
    const { error } = sentImageReceipt(result);
    content = <ImageFailure title={t.chat.compareFailed} reason={error ? imageErrorText(t, error) : t.chat.imageErrors.unknown} name={null} />;
  } else {
    content = <CompareCard {...parsed} />;
  }
  const caption = parsed?.summary.caption ?? null;
  return (
    <article className="chat-message chat-sent" data-enter-id={item.id}>
      {content}
      {caption && <div className="chat-sent-caption">{caption}</div>}
    </article>
  );
});

function CompareCard({ before, after, diff, summary }: { before: string; after: string; diff: string | null; summary: CompareSummary }) {
  const t = useT();
  const [ref, inView] = useInView<HTMLElement>();
  const [mode, setMode] = useState<"swipe" | "diff">("swipe");
  const [split, setSplit] = useState(50);
  const [gallery, setGallery] = useState<number | null>(null);
  const beforeLoad = useImageLoad(before);
  const afterLoad = useImageLoad(after);
  const drag = useRef<{ id: number; x: number; moved: boolean } | null>(null);
  const stageRef = useRef<HTMLDivElement>(null);

  // 画布按两张图的最大宽高：尺寸不同时，较小的那张从左上角摆起，空出来的地方就是「多出来的部分」。
  const width = Math.max(summary.before[0], summary.after[0]);
  const height = Math.max(summary.before[1], summary.after[1]);
  const pct = (value: number, total: number) => `${(value / total) * 100}%`;
  const place = (size: [number, number]) => ({ width: pct(size[0], width), height: pct(size[1], height) });

  const changed = summary.bounds !== null;
  const items: GalleryItem[] = useMemo(() => {
    const entry = (path: string, label: string): GalleryItem => ({ src: safeFileSrc(path) ?? "", name: `${label} · ${fileNameOf(path)}`, caption: label, path });
    const list = [entry(before, t.chat.compareBefore), entry(after, t.chat.compareAfter)];
    if (diff) list.push(entry(diff, t.chat.compareDiff));
    return list.filter((item) => item.src);
  }, [before, after, diff, t]);

  if ([before, after, diff].some((path) => path && isRemotePath(path))) {
    return <ImageFailure title={t.chat.compareFailed} reason={t.chat.imageErrors.forbidden} name={null} />;
  }
  if (beforeLoad.failed || afterLoad.failed) {
    return <ImageFailure title={t.chat.compareFailed} reason={t.chat.imageErrors.unreadable} name={fileNameOf(beforeLoad.failed ? before : after)} />;
  }

  const show = inView;
  const beforeSrc = show ? safeFileSrc(before) : null;
  const afterSrc = show ? safeFileSrc(after) : null;
  const diffSrc = show && diff ? safeFileSrc(diff) : null;
  const ready = !!beforeLoad.natural && !!afterLoad.natural;

  const moveTo = (clientX: number) => {
    const stage = stageRef.current;
    if (!stage) return;
    const rect = stage.getBoundingClientRect();
    if (rect.width <= 0) return;
    setSplit(Math.min(100, Math.max(0, ((clientX - rect.left) / rect.width) * 100)));
  };
  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    drag.current = { id: event.pointerId, x: event.clientX, moved: false };
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current || current.id !== event.pointerId || mode !== "swipe") return;
    if (!current.moved && Math.abs(event.clientX - current.x) < DRAG_PX) return;
    if (!current.moved) {
      current.moved = true;
      event.currentTarget.setPointerCapture?.(event.pointerId);
    }
    moveTo(event.clientX);
  };
  const onPointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    drag.current = null;
    if (!current || current.id !== event.pointerId) return;
    // 没拖动就是点一下：看大图，从当前看的那张开始。
    if (!current.moved && ready) setGallery(mode === "diff" && diff ? 2 : split < 50 ? 1 : 0);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 10 : 2;
    if (event.key === "ArrowLeft") setSplit((value) => Math.max(0, value - step));
    else if (event.key === "ArrowRight") setSplit((value) => Math.min(100, value + step));
    else if (event.key === "Home") setSplit(0);
    else if (event.key === "End") setSplit(100);
    else if (event.key === "Enter" || event.key === " ") setGallery(split < 50 ? 1 : 0);
    else return;
    event.preventDefault();
  };

  const [bx, by, bw, bh] = summary.bounds ?? [0, 0, 0, 0];
  const sizes = summary.before[0] !== summary.after[0] || summary.before[1] !== summary.after[1];
  return (
    <figure ref={ref} className="chat-compare" data-mode={mode} data-state={ready ? "ready" : "loading"}>
      <div
        ref={stageRef}
        className="chat-compare-stage"
        style={{ aspectRatio: `${width} / ${height}` }}
        role="slider"
        tabIndex={0}
        aria-label={t.chat.compareSlider}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(split)}
        aria-valuetext={t.chat.compareSliderValue(Math.round(100 - split))}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => { drag.current = null; }}
        onKeyDown={onKeyDown}
      >
        {mode === "swipe" ? (
          <>
            {beforeSrc && <img className="chat-compare-img" src={beforeSrc} alt={t.chat.compareBefore} style={place(summary.before)} draggable={false} onLoad={beforeLoad.onLoad} onError={beforeLoad.onError} />}
            <div className="chat-compare-after" style={{ clipPath: `inset(0 0 0 ${split}%)` }}>
              {afterSrc && <img className="chat-compare-img" src={afterSrc} alt={t.chat.compareAfter} style={place(summary.after)} draggable={false} onLoad={afterLoad.onLoad} onError={afterLoad.onError} />}
            </div>
            <span className="chat-compare-handle" style={{ left: `${split}%` }} aria-hidden="true"><span /></span>
            <span className="chat-compare-tag is-before">{t.chat.compareBefore}</span>
            <span className="chat-compare-tag is-after">{t.chat.compareAfter}</span>
          </>
        ) : (
          <>
            {diffSrc && <img className="chat-compare-img" src={diffSrc} alt={t.chat.compareDiff} style={{ width: "100%", height: "100%" }} draggable={false} />}
            <span className="chat-compare-tag is-before">{t.chat.compareDiffTag}</span>
          </>
        )}
        {changed && (
          <span className="chat-compare-box" aria-hidden="true"
            style={{ left: pct(bx, width), top: pct(by, height), width: pct(bw, width), height: pct(bh, height) }} />
        )}
      </div>
      <figcaption className="chat-compare-bar">
        <span className="chat-compare-stat" data-changed={changed || undefined}>
          {changed ? t.chat.compareChanged(summary.changed) : t.chat.compareNone}
        </span>
        {changed && <span className="chat-compare-meta">{t.chat.compareRegion(bw, bh)}</span>}
        {sizes && <span className="chat-compare-meta">{t.chat.compareSizes(`${summary.before[0]}×${summary.before[1]}`, `${summary.after[0]}×${summary.after[1]}`)}</span>}
        {diff && (
          <span className="chat-compare-seg" role="group" aria-label={t.chat.compareView}>
            <button type="button" aria-pressed={mode === "swipe"} onClick={() => setMode("swipe")}>{t.chat.compareSwipe}</button>
            <button type="button" aria-pressed={mode === "diff"} onClick={() => setMode("diff")}>{t.chat.compareDiff}</button>
          </span>
        )}
      </figcaption>
      {gallery !== null && items.length > 0 && (
        <Lightbox items={items} index={Math.min(gallery, items.length - 1)} onClose={() => setGallery(null)} renderActions={lightboxActions} />
      )}
    </figure>
  );
}
