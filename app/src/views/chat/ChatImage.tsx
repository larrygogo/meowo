// 对话里的图片：agent 发来的图（Meowo 发图工具，ToolKind::Image）、工具回执里带的图、正文 Markdown 里的图、
// 写出的图片文件。四个入口共用这里的加载、失败与灯箱，显示口径一致（前后端约定见任务目录 research/contract.md）。
//
// 懒加载：图片进视口（含上下 600px 的预读）才去拿。长会话里几十上百张图，打开时不能一起去读盘、去下载；
// 不靠浏览器自带的 loading="lazy"：它的预读距离由浏览器定（Chromium 上千像素），且后端解析（复制、下载）
// 要在请求图片之前做，只能自己判。
import { memo, useContext, useEffect, useState, type ReactNode } from "react";
import { isRemotePath, safeFileSrc } from "../../assetSrc";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { exportChatImage, resolveChatImage, revealTerminalPath, type ChatImageError, type ResolveChatImageResult } from "../../api";
import { useT } from "../../i18n";
import type { Dict } from "../../i18n/zh";
import { formatBackendError } from "../../i18n/errors";
import { Icon } from "../../icons/Icon";
import { icImage } from "../../icons/glyphs.chat";
import { icAlert, icDownload, icFolderOpen } from "../../icons/glyphs";
import { FILE_TOKEN_REFRESHED_EVENT, IMAGE_LOAD_FAILED_EVENT, remoteUi } from "../../remoteMode";
import { useFlash } from "../../hooks/useFlash";
import { Lightbox } from "./Lightbox";
import { ChatCwdContext } from "./chatCwd";
import { ChatSessionContext, useRemoteImageGate } from "./remoteImageGate";
import { useToolRunState } from "./toolState";
import type { ToolResultItem, ToolUseItem } from "./shared";

/** 回执里的图片引用行（与用户消息里的同一种写法，见 Message.tsx 的 IMAGE_REF）。 */
const IMAGE_REF = /\[Image(?: #\d+)?: source: ([^\]]+?)\]/g;
/** 发图工具失败时回执正文的固定前缀：`[Image error: <code>] <人话原因>`。 */
const IMAGE_ERROR = /^\s*\[Image error: ([a-z_]+)\]\s*([\s\S]*)$/;
/** 写出、修改时要露缩略图的图片扩展名（验收 #5 列的五种，外加 jpeg 这个同义写法）。 */
const IMAGE_EXT = /\.(?:png|jpe?g|gif|webp|svg)$/i;

export function isImagePath(path: string | null | undefined): boolean {
  return !!path && IMAGE_EXT.test(path.trim());
}

export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** 把回执拆成「去掉图片引用行的正文」和图片路径。没有图片时原样返回同一个对象（memo 友好）。 */
export function splitReceiptImages(result: ToolResultItem | undefined): { result: ToolResultItem | undefined; images: string[] } {
  if (!result || !result.text.includes("[Image")) return { result, images: [] };
  const images = [...result.text.matchAll(IMAGE_REF)].map((match) => match[1].trim());
  if (images.length === 0) return { result, images };
  return {
    result: { ...result, text: result.text.replace(IMAGE_REF, "").replace(/\n{3,}/g, "\n\n").trim() },
    images,
  };
}

/** 发图工具的参数：后端在 detail（kind image）里给出 source、caption；没有 detail 的旧数据退回 summary 里的参数原文 JSON。 */
export function sentImageArgs(item: ToolUseItem): { source: string | null; caption: string | null } {
  const detail = item.detail;
  if (detail?.kind === "image") {
    const pick = (value: string | null) => (value && value.trim() ? value.trim() : null);
    return { source: pick(detail.source), caption: pick(detail.caption) };
  }
  const raw = item.summary.trim();
  if (raw.startsWith("{")) {
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const pick = (value: unknown) => (typeof value === "string" && value.trim() ? value.trim() : null);
      return { source: pick(parsed.source), caption: pick(parsed.caption) };
    } catch { /* 截断的 JSON：当作没有说明 */ }
  }
  return { source: null, caption: null };
}

/** 发图工具的回执：成功时是图片引用行，失败时是带原因码的一句话。 */
export function sentImageReceipt(result: ToolResultItem | undefined): { images: string[]; error: ChatImageError | null } {
  if (!result) return { images: [], error: null };
  const failed = IMAGE_ERROR.exec(result.text);
  if (failed) return { images: [], error: { code: failed[1], message: failed[2].trim() } };
  const images = [...result.text.matchAll(IMAGE_REF)].map((match) => match[1].trim());
  if (result.is_error || images.length === 0) return { images, error: { code: "unknown", message: result.text.trim() } };
  return { images, error: null };
}

/** 原因码 → 人话。认得的码用我们自己的说法（两种语言一致）；不认得的退回后端那句。 */
export function imageErrorText(t: Dict, error: ChatImageError): string {
  return t.chat.imageErrors[error.code] ?? (error.message || t.chat.imageErrors.unknown);
}

/// 进视口才为 true，之后不再变回 false（滚出去不卸载已加载的图，免得来回滚时反复闪）。
/// 没有 IntersectionObserver 的环境（测试的 jsdom）直接当作可见。
/// 用回调 ref 记元素：同一个组件可能先渲染占位（网络图片的「点击加载」）、再换成图片容器，元素换了要重新观察。
export function useInView<T extends Element>(): [(el: T | null) => void, boolean] {
  const [el, ref] = useState<T | null>(null);
  const [seen, setSeen] = useState(() => typeof IntersectionObserver === "undefined");
  useEffect(() => {
    if (seen) return;
    if (!el) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        setSeen(true);
        observer.disconnect();
      }
    }, { rootMargin: "600px 0px" });
    observer.observe(el);
    return () => observer.disconnect();
  }, [seen, el]);
  return [ref, seen];
}

/// 解析结果按「来源 + 会话目录 + 会话」缓存：虚拟列表与流式重建会反复挂载同一张图，不能每次都去复制、去下载。
/// 会话也进键：手机桥按会话限定本地路径的范围，同一个相对路径在两个会话里结果可能不同。
/// 只缓存成功和确定的失败；调用本身抛错（桥断了、后端没这个命令）不缓存，下次挂载再试。
const resolved = new Map<string, Promise<ResolveChatImageResult>>();
const RESOLVE_CACHE_MAX = 400;

export function resolveChatImageCached(src: string, cwd: string | null, sessionId: number | null = null): Promise<ResolveChatImageResult> {
  const key = `${sessionId ?? ""}\u0000${cwd ?? ""}\u0000${src}`;
  const hit = resolved.get(key);
  if (hit) return hit;
  const pending = resolveChatImage(src, cwd, sessionId).catch((error: unknown) => {
    resolved.delete(key);
    throw error;
  });
  resolved.set(key, pending);
  // 先进先出地丢掉最早的，Map 保持插入顺序。
  if (resolved.size > RESOLVE_CACHE_MAX) resolved.delete(resolved.keys().next().value as string);
  return pending;
}

/** 测试用：清空解析缓存。 */
export function resetChatImageCache(): void {
  resolved.clear();
}

/// 图片的加载失败与远程凭据过期：手机端 /file 凭据换代时第一次会失败，transport 重领后派发事件，这里再试一次。
function useImageLoad(path: string | null) {
  const [failed, setFailed] = useState(false);
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  useEffect(() => { setFailed(false); setNatural(null); }, [path]);
  useEffect(() => {
    if (!failed) return;
    const retry = () => setFailed(false);
    window.addEventListener(FILE_TOKEN_REFRESHED_EVENT, retry);
    return () => window.removeEventListener(FILE_TOKEN_REFRESHED_EVENT, retry);
  }, [failed]);
  return {
    failed,
    natural,
    onLoad: (event: React.SyntheticEvent<HTMLImageElement>) => {
      const img = event.currentTarget;
      setNatural({ w: img.naturalWidth, h: img.naturalHeight });
    },
    onError: () => {
      setFailed(true);
      window.dispatchEvent(new CustomEvent(IMAGE_LOAD_FAILED_EVENT));
    },
  };
}

/** 形状：决定缩略图怎么摆。长图（高宽比超过 2.2）裁顶部并标「长图」；小图（长边不超过 96px，图标类）按原尺寸
 *  摆在底板上，不放大成一片马赛克。 */
export type ImageShape = "small" | "long" | "normal";
export function imageShape(natural: { w: number; h: number } | null): ImageShape | null {
  if (!natural || natural.w <= 0 || natural.h <= 0) return null;
  if (Math.max(natural.w, natural.h) <= 96) return "small";
  if (natural.h / natural.w > 2.2) return "long";
  return "normal";
}

/// 灯箱与卡片上的「保存」「在文件夹中显示」。保存：桌面弹系统保存框后交给后端复制；手机端没有宿主文件系统，
/// 用浏览器下载（文件落在手机里，与导出会话同一个做法）。在文件夹中显示只在桌面有。
export function ImageActions({ path, variant = "lightbox" }: { path: string; variant?: "lightbox" | "overlay" }) {
  const t = useT();
  const { shown: saved, flash } = useFlash();
  const [error, setError] = useState<string | null>(null);
  const name = fileNameOf(path);
  const save = async () => {
    setError(null);
    if (remoteUi()) {
      const href = safeFileSrc(path);
      if (!href) {
        setError(t.chat.imageErrors.forbidden);
        return;
      }
      const link = document.createElement("a");
      link.href = href;
      link.download = name;
      link.rel = "noreferrer";
      document.body.appendChild(link);
      link.click();
      link.remove();
      flash();
      return;
    }
    const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase();
    const dest = await saveDialog({ defaultPath: name, filters: ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : undefined }).catch(() => null);
    if (!dest) return;
    try {
      await exportChatImage(path, dest);
      flash();
    } catch (failure) {
      setError(formatBackendError(failure, t.locale));
    }
  };
  const reveal = () => {
    setError(null);
    revealTerminalPath("", path).catch((failure: unknown) => setError(formatBackendError(failure, t.locale)));
  };
  const saveLabel = saved ? t.chat.imageSaved : t.chat.imageSave;
  return (
    <>
      {error && <span className="chat-lightbox-note" role="alert">{error}</span>}
      <button type="button" className={variant === "overlay" ? "chat-sent-action" : "chat-lightbox-action"} aria-label={saveLabel} data-tip={saveLabel}
        onClick={(event) => { event.stopPropagation(); void save(); }}>
        <Icon glyph={icDownload} size={16} />
        {variant === "lightbox" && <span>{saveLabel}</span>}
      </button>
      {!remoteUi() && (
        <button type="button" className={variant === "overlay" ? "chat-sent-action" : "chat-lightbox-action"} aria-label={t.chat.imageReveal} data-tip={t.chat.imageReveal}
          onClick={(event) => { event.stopPropagation(); reveal(); }}>
          <Icon glyph={icFolderOpen} size={16} />
          {variant === "lightbox" && <span>{t.chat.imageReveal}</span>}
        </button>
      )}
    </>
  );
}

/** 失败时的说明卡：图标 + 一句结论 + 原因 + 文件名。不出空白框。 */
function ImageFailure({ title, reason, name }: { title: string; reason: string; name: string | null }) {
  return (
    <div className="chat-sent-failed" role="note">
      <Icon glyph={icAlert} size={16} />
      <div className="chat-sent-failed-text">
        <span className="chat-sent-failed-title">{title}</span>
        <span className="chat-sent-failed-reason">{reason}{name ? ` · ${name}` : ""}</span>
      </div>
    </div>
  );
}

/// 一张发来的图。多图时外层按相册排（data-count），单张按原比例摆。
const SentImageTile = memo(function SentImageTile({ path, caption, album }: { path: string; caption: string | null; album: boolean }) {
  const t = useT();
  const [ref, inView] = useInView<HTMLElement>();
  const [open, setOpen] = useState(false);
  const load = useImageLoad(path);
  const name = fileNameOf(path);
  if (isRemotePath(path)) return <ImageFailure title={t.chat.sentImageFailed} reason={t.chat.imageErrors.forbidden} name={name} />;
  if (load.failed) return <ImageFailure title={t.chat.sentImageFailed} reason={t.chat.imageErrors.unreadable} name={name} />;
  const src = inView ? safeFileSrc(path) : null;
  const shape = imageShape(load.natural);
  return (
    <figure ref={ref} className="chat-sent-tile" data-shape={album ? "album" : shape ?? undefined} data-state={shape ? "ready" : "loading"}>
      <button type="button" className="chat-sent-open" aria-label={`${t.chat.sentImageOpen} ${name}`} onClick={() => setOpen(true)} disabled={!shape}>
        {src && <img className="chat-sent-img" src={src} alt={caption ?? name} referrerPolicy="no-referrer" draggable={false} onLoad={load.onLoad} onError={load.onError} />}
        {!shape && <span className="chat-sent-loading" role="img" aria-label={t.chat.imageLoading}><Icon glyph={icImage} size={20} /></span>}
        {shape === "long" && !album && <span className="chat-sent-badge">{t.chat.imageLong}</span>}
      </button>
      {shape && <div className="chat-sent-actions"><ImageActions path={path} variant="overlay" /></div>}
      {open && src && <Lightbox src={src} name={name} caption={caption} onClose={() => setOpen(false)} actions={<ImageActions path={path} />} />}
    </figure>
  );
});

/// agent 发来的图片消息（ToolKind::Image 的调用）：不是一行收起的工具，而是像 IM 里收到的一张图——图直接可见，
/// 下面是 agent 附的一句说明，点开看大图、保存、在文件夹中显示。没有回执时是「正在发送」，失败时说清原因。
export const SentImageMessage = memo(function SentImageMessage({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const state = useToolRunState(result);
  const { source, caption } = sentImageArgs(item);
  const { images, error } = sentImageReceipt(result);
  const sourceName = source ? fileNameOf(source.replace(/[?#].*$/, "")) : null;
  let content: ReactNode;
  if (state === "running" || state === "waiting") {
    content = (
      <div className="chat-sent-pending" role="status">
        <span className="chat-model-spinner" aria-hidden="true" />
        {t.chat.sentImageSending}
      </div>
    );
  } else if (state === "interrupted") {
    content = <ImageFailure title={t.chat.sentImageFailed} reason={t.chat.interrupted} name={sourceName} />;
  } else if (error) {
    content = <ImageFailure title={t.chat.sentImageFailed} reason={imageErrorText(t, error)} name={sourceName} />;
  } else {
    content = (
      <div className="chat-sent-media" data-count={Math.min(images.length, 4)}>
        {images.map((path, index) => <SentImageTile key={`${index}:${path}`} path={path} caption={caption} album={images.length > 1} />)}
      </div>
    );
  }
  return (
    <article className="chat-message chat-sent" data-enter-id={item.id}>
      {content}
      {caption && <div className="chat-sent-caption">{caption}</div>}
    </article>
  );
});

/// 这次调用读出或写出的图片文件（回执里没带图片引用行时才用）：读图片、写出或修改 png/jpg/gif/webp/svg。
/// 后端在第一次解析到写图调用时就把当时的文件存进 tool/，并在回执里补引用行，所以同一路径写三次是三张不同的图；
/// 只有这条规则之前的旧数据才走到这里，按 detail.path 现解析（显示的是现在的文件内容）。
/// 删除的不算；失败的调用不露图（文件多半没写成）。
export function fileImages(item: ToolUseItem, result: ToolResultItem | undefined): string[] {
  if (!result || result.is_error || result.interrupted) return [];
  const detail = item.detail;
  if (detail?.kind === "patch") return detail.files.filter((file) => file.op !== "delete").map((file) => file.move_to ?? file.path).filter(isImagePath);
  if (detail?.kind === "read" || detail?.kind === "write" || detail?.kind === "edit") return isImagePath(detail.path) ? [detail.path] : [];
  return [];
}

/// 收起的过程块（TurnFold）里各工具带的图：块收着时也把缩略图露在摘要行下面，不用展开两层才看到截图。
export function FoldImageStrip({ tools, results }: { tools: ToolUseItem[]; results: Map<string, ToolResultItem> }) {
  const receipts: string[] = [];
  const files: string[] = [];
  for (const tool of tools) {
    if (tool.kind === "image") continue;
    const result = results.get(tool.id);
    const { images } = splitReceiptImages(result);
    if (images.length > 0) receipts.push(...images);
    else files.push(...fileImages(tool, result));
  }
  if (receipts.length === 0 && files.length === 0) return null;
  return (
    <div className="chat-tool-media is-fold">
      {receipts.map((path, index) => <ToolThumb key={`r${index}:${path}`} path={path} />)}
      {files.map((path, index) => <ResolvedImage key={`f${index}:${path}`} src={path} mode="thumb" />)}
    </div>
  );
}

/// 工具行下面直接露出的缩略图（截图、读图、写出的图片）：不用展开工具行。
export function ToolImageStrip({ paths }: { paths: string[] }) {
  if (paths.length === 0) return null;
  return (
    <div className="chat-tool-media">
      {paths.map((path, index) => <ToolThumb key={`${index}:${path}`} path={path} />)}
    </div>
  );
}

function ToolThumb({ path }: { path: string }) {
  const t = useT();
  const [ref, inView] = useInView<HTMLButtonElement>();
  const [open, setOpen] = useState(false);
  const load = useImageLoad(path);
  const name = fileNameOf(path);
  if (isRemotePath(path)) return <ImageChip name={name} reason={t.chat.imageErrors.forbidden} />;
  if (load.failed) return <ImageChip name={name} reason={t.chat.imageErrors.unreadable} />;
  const src = inView ? safeFileSrc(path) : null;
  const shape = imageShape(load.natural);
  return (
    <>
      <button ref={ref} type="button" className="chat-tool-thumb" data-shape={shape ?? undefined} data-state={shape ? "ready" : "loading"}
        aria-label={`${t.chat.sentImageOpen} ${name}`} data-tip={name} onClick={() => setOpen(true)}>
        {src && <img src={src} alt={name} referrerPolicy="no-referrer" draggable={false} onLoad={load.onLoad} onError={load.onError} />}
      </button>
      {open && src && <Lightbox src={src} name={name} onClose={() => setOpen(false)} actions={<ImageActions path={path} />} />}
    </>
  );
}

/** 显示不了时的文件名徽章，带上原因（与附件的文件名徽章同一个皮）。 */
export function ImageChip({ name, reason }: { name: string; reason: string | null }) {
  return (
    <span className="chat-image-chip" data-tip={reason ? `${name} · ${reason}` : name}>
      <Icon glyph={icImage} size={12} />
      <span>{name}</span>
      {reason && <span className="chat-image-chip-reason">{reason}</span>}
    </span>
  );
}

/// 还没放行的网络图片：和文件名徽章同一个皮，写出域名和「点击加载图片」。点了放行这个域名（当前会话内），
/// 这张图和同域名的其他图一起去加载。悬停说明会写清楚点了之后发生什么。
export function RemoteImagePlaceholder({ host, name, onLoad }: { host: string; name: string; onLoad: () => void }) {
  const t = useT();
  return (
    <button type="button" className="chat-image-chip is-gated" data-tip={t.chat.remoteImageTip(host, name)}
      onClick={(event) => { event.stopPropagation(); onLoad(); }}>
      <Icon glyph={icImage} size={12} />
      <span>{host}</span>
      <span className="chat-image-chip-reason">{t.chat.remoteImageLoad}</span>
    </button>
  );
}

/// 需要先交给后端解析的图：正文 Markdown 里的本地路径与网址、写出或读取的图片文件。进视口才解析，解析出 Meowo
/// 图片目录里的持久路径后再显示；解析失败或读不到退回文件名徽章并写明原因。
/// mode：inline 是正文里的图（随正文排版、限宽）；thumb 是工具行下的缩略图（与 ToolThumb 同尺寸）。
export function ResolvedImage({ src, alt, mode }: { src: string; alt?: string | null; mode: "inline" | "thumb" }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const sessionId = useContext(ChatSessionContext);
  const gate = useRemoteImageGate(src);
  const [ref, inView] = useInView<HTMLSpanElement>();
  const [outcome, setOutcome] = useState<ResolveChatImageResult | null>(null);
  const [open, setOpen] = useState(false);
  const path = outcome && "path" in outcome ? outcome.path : null;
  const load = useImageLoad(path);
  const name = alt?.trim() || fileNameOf(src.replace(/[?#].*$/, "")) || src;
  useEffect(() => {
    setOutcome(null);
    // 网络图片没放行前不去解析：解析就是下载（见 remoteImageGate.ts）。
    if (!inView || !gate.ready) return;
    let alive = true;
    resolveChatImageCached(src, cwd, sessionId)
      .then((value) => { if (alive) setOutcome(value); })
      .catch(() => { if (alive) setOutcome({ error: { code: "unknown", message: "" } }); });
    return () => { alive = false; };
  }, [src, cwd, sessionId, inView, gate.ready]);
  if (!gate.ready && gate.host) return <RemoteImagePlaceholder host={gate.host} name={name} onLoad={gate.allow} />;
  if (outcome && "error" in outcome) return <ImageChip name={name} reason={imageErrorText(t, outcome.error)} />;
  if (load.failed) return <ImageChip name={name} reason={t.chat.imageErrors.unreadable} />;
  const url = path ? safeFileSrc(path) : null;
  const shape = imageShape(load.natural);
  const cls = mode === "inline" ? "chat-md-image" : "chat-tool-thumb";
  return (
    <span ref={ref} className={cls} data-shape={shape ?? undefined} data-state={shape ? "ready" : "loading"}>
      <button type="button" className="chat-md-image-open" aria-label={`${t.chat.sentImageOpen} ${name}`} data-tip={name} onClick={() => setOpen(true)} disabled={!shape}>
        {url && <img src={url} alt={name} referrerPolicy="no-referrer" draggable={false} onLoad={load.onLoad} onError={load.onError} />}
      </button>
      {!shape && mode === "inline" && <span className="chat-sent-loading" role="img" aria-label={t.chat.imageLoading}><Icon glyph={icImage} size={16} /></span>}
      {open && url && path && <Lightbox src={url} name={name} onClose={() => setOpen(false)} actions={<ImageActions path={path} />} />}
    </span>
  );
}
