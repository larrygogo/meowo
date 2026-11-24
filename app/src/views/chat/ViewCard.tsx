// 对话里的 HTML 视图卡（Meowo 的 MCP 工具 show_html，以及方案卡里的 html 预览）。任务 20261124-html-cards。
//
// 后端把页面存成快照（chat_views.rs），回执最后一行 `[View: {JSON}]` 是标题、指纹、大小、可选高度。
// 页面从独立的源出（桌面 meowo-view 协议、手机 /view/ 路由），iframe 只给 allow-scripts：页面碰不到
// Meowo 的 DOM、存储和接口。宿主和页面只走 postMessage，消息名用 MCP Apps 的子集：
//   页面 → 宿主：ui/notifications/size-changed（高度）、ui/message（放进输入框，不替用户发）、
//               ui/open-link（打开链接）、meowo/view-missing（快照已被回收）
//   宿主 → 页面：ui/notifications/host-context-changed（主题）
// 只认来自本卡 iframe 的消息（event.source 比对），字段逐个校验。
import { memo, useCallback, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { viewSrc } from "../../assetSrc";
import { useT } from "../../i18n";
import { openLink } from "../../api";
import { pushEscLayer } from "../../escLayers";
import { remoteUi } from "../../remoteMode";
import { ChatActionsContext } from "./chatActions";
import { ImageFailure } from "./ChatImage";
import { openInPanel } from "./panelLinks";
import { openPhonePreview } from "./phonePreview";
import { useToolRunState } from "./toolState";
import type { ToolResultItem, ToolUseItem } from "./shared";

export type ViewReceipt = { title: string; id: string; bytes: number; height: number | null };

const MARKER = "[View: ";
/** 页面放进输入框的一句话最长多少字：够写「选方案 2，按钮再圆一点」，挡住页面一次塞进几万字。 */
const SEND_MAX = 2000;
/** 没收到页面报高度之前的占位高度（页面加载通常几十毫秒，先占一块不至于跳得太狠）。 */
const INITIAL_HEIGHT = 200;
const MIN_HEIGHT = 48;
/** 两次放进输入框之间至少隔多久：更快的那次不丢，排到间隔满了再放（只留最新一句），挡住刷屏又不吞掉改主意的点击。 */
const ACTION_GAP_MS = 600;
/** 点过「打开 / 不打开」后，这张卡多久内不再接受开链接请求。 */
const LINK_COOLDOWN_MS = 3000;

export function parseViewReceipt(text: string): ViewReceipt | null {
  const line = text.split("\n").map((entry) => entry.trim()).reverse().find((entry) => entry.startsWith(MARKER) && entry.endsWith("]"));
  if (!line) return null;
  try {
    const raw = JSON.parse(line.slice(MARKER.length, -1)) as Record<string, unknown>;
    if (typeof raw.title !== "string" || typeof raw.id !== "string" || !/^[0-9a-f]{32}$/.test(raw.id)) return null;
    const height = typeof raw.height === "number" && raw.height > 0 ? raw.height : null;
    return { title: raw.title, id: raw.id, bytes: typeof raw.bytes === "number" ? raw.bytes : 0, height };
  } catch {
    return null;
  }
}

/** 当前生效的主题（appearance.ts 写在 <html data-theme>），变了就重渲染：页面跟着换颜色。 */
function subscribeTheme(listener: () => void) {
  const observer = new MutationObserver(listener);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  return () => observer.disconnect();
}
function readTheme(): "light" | "dark" {
  return document.documentElement.getAttribute("data-theme") === "light" ? "light" : "dark";
}
export function useDocumentTheme(): "light" | "dark" {
  return useSyncExternalStore(subscribeTheme, readTheme, readTheme);
}

type Message = { method: string; params: Record<string, unknown> };
function asMessage(data: unknown): Message | null {
  if (typeof data !== "object" || data === null) return null;
  const record = data as Record<string, unknown>;
  if (record.jsonrpc !== "2.0" || typeof record.method !== "string") return null;
  const params = typeof record.params === "object" && record.params !== null ? record.params as Record<string, unknown> : {};
  return { method: record.method, params };
}

/** 只认 http(s) 绝对地址，返回规范化后的写法；其余（javascript:、file:、相对地址）一律不认。 */
function safeLink(href: string): string | null {
  try {
    const url = new URL(href);
    // 带账号密码的地址（https://github.com@evil.example/）专门用来骗眼睛，不认。
    if (url.username || url.password) return null;
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

/** 页面里 http(s) 链接：本机地址桌面进右侧面板、手机走预览代理，其余交给系统浏览器（与正文链接同一套）。
 *  只在用户点了卡片上的「打开」时调用（用户手势里，手机浏览器不会拦新页面）。 */
function openFromView(href: string, opening: string) {
  const safe = safeLink(href);
  if (!safe) return;
  const url = new URL(safe);
  if (openInPanel(url.href)) return;
  const preview = openPhonePreview(url.href, opening);
  if (preview) {
    void preview.catch(() => {});
    return;
  }
  if (remoteUi()) {
    window.open(url.href, "_blank", "noopener,noreferrer");
    return;
  }
  void openLink(url.href).catch(() => {});
}

/**
 * 一块沙箱页面。高度跟着页面报的走（上限 maxHeight，超出页面自己滚）；fixedHeight 给了就固定。
 * 放大不换 iframe（换了页面状态就丢了）：外层卡片切成铺满窗口的定位，iframe 原地变大。
 */
export function ViewFrame({ id, title, fixedHeight, maxHeight, onMissing, onEscape, onLink, className, interactive = true, fill = false }: {
  id: string;
  title: string;
  /** 撑满外层（缩略图、放大层）：高度交给 CSS，不跟页面报的走。 */
  fill?: boolean;
  fixedHeight?: number | null;
  maxHeight?: number;
  onMissing?: () => void;
  /** 焦点在页面里时按了 Esc（桥转告）：放大态用它收回。 */
  onEscape?: () => void;
  /** 页面想打开一个 http(s) 链接：由卡片显示确认条，用户点了才开。 */
  onLink?: (url: string) => void;
  className?: string;
  /** false：缩略图，页面照常渲染但不接收点击（方案卡里点缩略图是「选它」，不是点页面里的按钮）。 */
  interactive?: boolean;
}) {
  const t = useT();
  const theme = useDocumentTheme();
  const actions = useContext(ChatActionsContext);
  const frame = useRef<HTMLIFrameElement>(null);
  const [reported, setReported] = useState<number | null>(null);
  // 地址只按首次的主题定：之后换主题发消息过去，不重载页面（重载会丢掉用户在页面里点出来的状态）。
  const [src] = useState(() => viewSrc(id, theme));
  const fillPrompt = actions?.fillPrompt;
  const missing = useRef(onMissing);
  missing.current = onMissing;
  const lastFill = useRef(Number.NEGATIVE_INFINITY);
  const queued = useRef(0);
  useEffect(() => () => clearTimeout(queued.current), []);
  const escape = useRef(onEscape);
  escape.current = onEscape;
  const link = useRef(onLink);
  link.current = onLink;
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (!frame.current || event.source !== frame.current.contentWindow) return;
      const message = asMessage(event.data);
      if (!message) return;
      switch (message.method) {
        case "ui/notifications/size-changed": {
          const height = Number(message.params.height);
          if (Number.isFinite(height) && height > 0) setReported(Math.ceil(height));
          break;
        }
        case "ui/message": {
          // 只放进输入框、不发送；要「用户正在用这块页面」：焦点在这个 iframe 上、不是缩略图、两次间隔 ≥1 秒。
          // 放进去以后焦点移到输入框，页面要再放一句就得等用户点回来。不再猜「刚点过没有」：父页看不到跨源
          // iframe 里的点击，靠焦点事件推断会漏判（审查第 3 轮：先点「保存」再点「选这个」被静默丢掉）。
          // 剩余风险：页面自己抢焦点后能放进一句话——看得见、不会自动发送（decisions.md）。
          if (!interactive || document.activeElement !== frame.current) break;
          const content = message.params.content as Record<string, unknown> | undefined;
          const text = typeof content?.text === "string" ? content.text.trim().slice(0, SEND_MAX) : "";
          if (!text || !fillPrompt) break;
          const wait = ACTION_GAP_MS - (performance.now() - lastFill.current);
          if (queued.current) clearTimeout(queued.current);
          const put = () => {
            queued.current = 0;
            lastFill.current = performance.now();
            fillPrompt(text);
          };
          if (wait <= 0) put();
          else queued.current = window.setTimeout(put, wait);
          break;
        }
        case "ui/open-link": {
          // 不自动打开：交给卡片画一条宿主自己的「打开」确认（页面伪造不了宿主的按钮；iOS 也只在用户手势里放行新页面）。
          const url = typeof message.params.url === "string" ? safeLink(message.params.url) : null;
          if (interactive && url) link.current?.(url);
          break;
        }
        case "meowo/view-missing":
          missing.current?.();
          break;
        case "meowo/escape":
          escape.current?.();
          break;
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [fillPrompt, interactive]);

  // 主题变了告诉页面（桥把它写到 <html data-theme> 与 color-scheme 上）。
  useEffect(() => {
    frame.current?.contentWindow?.postMessage({ jsonrpc: "2.0", method: "ui/notifications/host-context-changed", params: { theme } }, "*");
  }, [theme]);

  if (!src) return null;
  const natural = reported ?? INITIAL_HEIGHT;
  const height = fixedHeight ?? Math.max(MIN_HEIGHT, maxHeight ? Math.min(natural, maxHeight) : natural);
  return (
    <iframe
      ref={frame}
      className={className}
      src={src}
      title={title}
      // 只给脚本：不给 allow-same-origin（否则页面与 Meowo 同源级别可触达接口）、不给弹窗 / 顶层跳转 / 表单。
      sandbox="allow-scripts"
      referrerPolicy="no-referrer"
      loading="lazy"
      tabIndex={interactive ? undefined : -1}
      aria-hidden={interactive ? undefined : true}
      // 页面的 prefers-color-scheme 跟着 iframe 元素的 color-scheme 走。
      style={{ height: fill ? "100%" : height, colorScheme: theme, pointerEvents: interactive ? undefined : "none" }}
    />
  );
}

/** 链接确认的状态：先到先得（条显示期间页面换地址不理，审查第 4 轮实测能轮换欺骗）；点过「打开 / 不打开」后
 *  冷却 3 秒，被拒过的地址在这张卡上不再弹（不然点了「不打开」，页面立刻又弹出来）。
 *  不用「指针重新进入」放行：Chromium 会补发 pointerenter，手机触摸则根本收不到（审查第 5 轮，两端都失效）。 */
function useLinkAsk() {
  const [url, setUrl] = useState<string | null>(null);
  const until = useRef(Number.NEGATIVE_INFINITY);
  const refused = useRef(new Set<string>());
  const request = useCallback((next: string) => {
    if (performance.now() < until.current || refused.current.has(next)) return;
    setUrl((current) => current ?? next);
  }, []);
  const settle = (current: string | null, accepted: boolean) => {
    if (current && !accepted) refused.current.add(current);
    until.current = performance.now() + LINK_COOLDOWN_MS;
    setUrl(null);
  };
  return { url, request, settle };
}

/** 页面想打开的链接：卡片上一条宿主自己画的确认，点「打开」才开（页面伪造不了这个按钮）。 */
function LinkAsk({ url, onOpen, onDismiss }: { url: string; onOpen: () => void; onDismiss: () => void }) {
  const t = useT();
  // 主机名单独显示、不截断，太长时保留末尾（真正的域名在末尾：github.com.login.xxx.evil.example）；路径弱化可截断。
  const parsed = new URL(url);
  const limit = remoteUi() ? 22 : 48;
  const host = parsed.host.length > limit ? `…${parsed.host.slice(-(limit - 1))}` : parsed.host;
  const rest = `${parsed.pathname === "/" ? "" : parsed.pathname}${parsed.search}`;
  return (
    <div className="chat-view-linkask" role="status">
      <span className="chat-view-linkask-text">{t.chat.viewLinkAsk}</span>
      <span className="chat-view-linkask-url" title={url}><b>{host}</b>{rest && <span>{rest}</span>}</span>
      <button type="button" className="chat-view-linkask-open" onClick={() => { openFromView(url, t.chat.previewOpening); onOpen(); }}>{t.chat.viewLinkOpen}</button>
      <button type="button" className="chat-view-btn" aria-label={t.chat.viewLinkDismiss} data-tip={t.chat.viewLinkDismiss} onClick={onDismiss}>✕</button>
    </div>
  );
}

/** 放大态：注册 Esc 层（对话窗的「Esc = 拒绝审批」让位），按 Esc 收回。 */
function useZoomEsc(zoomed: boolean, close: () => void) {
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    if (!zoomed) return;
    const pop = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeRef.current();
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      pop();
      document.removeEventListener("keydown", onKey, true);
    };
  }, [zoomed]);
}

/** 一条 show_html 调用：还在存时转圈，失败给原因，成功出视图卡。 */
export const ViewMessage = memo(function ViewMessage({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const state = useToolRunState(result);
  const receipt = useMemo(() => (result && !result.is_error ? parseViewReceipt(result.text) : null), [result]);
  let content;
  if (state === "running" || state === "waiting") {
    content = (
      <div className="chat-sent-pending chat-view-pending" role="status">
        <span className="chat-model-spinner" aria-hidden="true" />
        {t.chat.viewPreparing}
      </div>
    );
  } else if (state === "interrupted") content = <ImageFailure title={t.chat.viewFailed} reason={t.chat.interrupted} name={null} />;
  else if (!receipt) content = <ImageFailure title={t.chat.viewFailed} reason={result?.text.trim() || t.chat.imageErrors.unknown} name={null} />;
  else content = <ViewCard view={receipt} />;
  return (
    <article className="chat-message chat-sent chat-view-message" data-enter-id={item.id}>
      {content}
    </article>
  );
});

export function ViewCard({ view }: { view: ViewReceipt }) {
  const t = useT();
  const [zoomed, setZoomed] = useState(false);
  const [missing, setMissing] = useState(false);
  const [reload, setReload] = useState(0);
  const close = useCallback(() => setZoomed(false), []);
  const ask = useLinkAsk();
  useZoomEsc(zoomed, close);
  // 上限约半屏：一张卡不该把对话挤得只剩它；页面更高时在卡内滚。
  const maxHeight = Math.max(240, Math.round(window.innerHeight * 0.55));

  if (missing) {
    return (
      <div className="chat-view is-missing" data-missing>
        <div className="chat-view-head">
          <span className="chat-view-title">{view.title}</span>
        </div>
        <div className="chat-view-gone">{t.chat.viewMissing}</div>
      </div>
    );
  }
  return (
    <>
      {zoomed && <div className="chat-view-backdrop" onClick={close} aria-hidden="true" />}
      <div className={"chat-view" + (zoomed ? " is-zoomed" : "")} role={zoomed ? "dialog" : undefined} aria-label={zoomed ? view.title : undefined} aria-modal={zoomed || undefined}>
        <div className="chat-view-head">
          <span className="chat-view-tag">{t.chat.viewTag}</span>
          <span className="chat-view-title" title={view.title}>{view.title}</span>
          <span className="chat-view-actions">
            <button type="button" className="chat-view-btn" aria-label={t.chat.viewReload} data-tip={t.chat.viewReload} onClick={() => setReload((n) => n + 1)}>↻</button>
            <button type="button" className="chat-view-btn" aria-label={zoomed ? t.chat.viewShrink : t.chat.viewZoom} data-tip={zoomed ? t.chat.viewShrink : t.chat.viewZoom}
              aria-pressed={zoomed} onClick={() => setZoomed((v) => !v)}>{zoomed ? "✕" : "⤢"}</button>
          </span>
        </div>
        <div className="chat-view-body">
          {ask.url && <LinkAsk url={ask.url} onOpen={() => ask.settle(ask.url, true)} onDismiss={() => ask.settle(ask.url, false)} />}
          <ViewFrame key={reload} id={view.id} title={view.title} className="chat-view-frame"
            fixedHeight={zoomed ? null : view.height} maxHeight={zoomed ? undefined : maxHeight}
            onMissing={() => setMissing(true)} onEscape={zoomed ? close : undefined} onLink={ask.request} />
        </div>
      </div>
    </>
  );
}

/** 方案卡里的页面缩略：按两倍宽渲染再缩一半（页面按 400~500px 宽排版，不至于挤成一条），不接收点击。 */
export function ViewThumb({ id, title }: { id: string; title: string }) {
  const t = useT();
  const [missing, setMissing] = useState(false);
  if (missing) return <span className="chat-choice-shot-missing">{t.chat.viewMissing}</span>;
  return (
    <span className="chat-view-thumb">
      <ViewFrame id={id} title={title} fill interactive={false} className="chat-view-thumb-frame" onMissing={() => setMissing(true)} />
    </span>
  );
}

/** 方案的放大层：铺满窗口，页面能点能试；action 是「选这个」之类的按钮（卡片已答过就不给）。 */
export function ViewZoom({ id, title, caption, action, onClose }: {
  id: string;
  title: string;
  caption?: string | null;
  action?: ReactNode;
  onClose: () => void;
}) {
  const t = useT();
  const [missing, setMissing] = useState(false);
  const ask = useLinkAsk();
  useZoomEsc(true, onClose);
  return createPortal(
    <>
      <div className="chat-view-backdrop" onClick={onClose} aria-hidden="true" />
      <div className="chat-view is-zoomed" role="dialog" aria-modal="true" aria-label={title}>
        <div className="chat-view-head">
          <span className="chat-view-title" title={title}>{title}</span>
          {caption && <span className="chat-view-meta">{caption}</span>}
          <span className="chat-view-actions">
            {action}
            <button type="button" className="chat-view-btn" aria-label={t.chat.viewShrink} data-tip={t.chat.viewShrink} onClick={onClose}>✕</button>
          </span>
        </div>
        <div className="chat-view-body">
          {ask.url && <LinkAsk url={ask.url} onOpen={() => ask.settle(ask.url, true)} onDismiss={() => ask.settle(ask.url, false)} />}
          {missing
            ? <div className="chat-view-gone">{t.chat.viewMissing}</div>
            : <ViewFrame id={id} title={title} fill className="chat-view-frame" onMissing={() => setMissing(true)} onEscape={onClose} onLink={ask.request} />}
        </div>
      </div>
    </>,
    document.body,
  );
}
