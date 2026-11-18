import { useEffect, useRef, useState, type FormEvent } from "react";
import { answerBrowserPanePermission, browserPaneAction, navigateBrowserPane, openLink, revealBrowserPaneDownload, setBrowserPaneAgentPaused, setBrowserPaneBounds, type BrowserDownload, type BrowserPaneInfo } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { PathMenu, type PathMenuItem } from "./pathMenu";
import { Icon } from "../../icons/Icon";
import { icDownload, icMore, icRefresh } from "../../icons/glyphs";
import { icArrowLeft, icArrowRight, icBan, icExternal } from "../../icons/glyphs.scm";
import { icShield } from "../../icons/glyphs.chat";
import { isWindows } from "../../platform";
import { busyProps } from "../../busyButton";
// 老调用方（测试）从这里拿；ChatWindow 直接引 localUrls，免得把面板浏览器拖进主 chunk。
export { localUrlsIn } from "./localUrls";

/**
 * 这些元素出现且和浏览器区域重叠时，原生 webview 要先藏起来：它是叠在页面上的原生视图，
 * 会盖住一切 HTML，菜单和弹窗画在它下面就看不见、也点不到。
 * Kimi Code 另起一层透明视图把弹层画在上面；我们只是藏，代价是菜单开着时那块是空的。
 */
const OCCLUDERS = '[role="menu"], [role="dialog"], [role="listbox"], .chat-modal-overlay';

function intersects(a: DOMRect, b: DOMRect): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

export function occluded(area: DOMRect, root: ParentNode = document): boolean {
  for (const el of root.querySelectorAll(OCCLUDERS)) {
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0 && intersects(area, rect)) return true;
  }
  return false;
}

/// 接管了多久（m:ss），每秒走一次；没在接管时不起定时器。
function usePausedFor(paused: boolean): string {
  const [since, setSince] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!paused) { setSince(null); return; }
    const start = Date.now();
    setSince(start);
    setNow(start);
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [paused]);
  const seconds = since === null ? 0 : Math.max(0, Math.floor((now - since) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/**
 * 面板「浏览器」页签：地址栏 + 一块占位区域。真正的网页是后端在 chat 窗口里叠的原生
 * 子 webview（browser_pane.rs），这里负责把占位区域的位置实时报过去。
 *
 * about:blank 时不显示原生视图，露出下面的起始页（本会话里出现过的本机地址可以一点就开）。
 */
export function BrowserPane({ info, visible, suggestions }: {
  info: BrowserPaneInfo;
  visible: boolean;
  /** 本会话对话里出现过的本机地址，最近的在前。 */
  suggestions: readonly string[];
}) {
  const t = useT();
  const hostRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const blank = info.url === "about:blank";
  const inputLocked = info.agentActive && !info.agentPaused;
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pausedFor = usePausedFor(!!info.agentPaused);
  const showNative = visible && !blank;

  // 位置同步：面板拖宽、最大化、窗口缩放、DOM 里冒出菜单/弹窗，都重新量一次。
  // 同一帧里的多次变化合并成一次下发。
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    // 不需要显示时只藏一次，不挂观察器：对话流式输出时 DOM 每帧都在变，没必要白量。
    if (!showNative) {
      void setBrowserPaneBounds(info.id, { x: 0, y: 0, width: 0, height: 0 }, false).catch(() => {});
      return;
    }
    let frame = 0;
    let last = "";
    const sync = () => {
      frame = 0;
      const rect = host.getBoundingClientRect();
      const show = rect.width > 0 && rect.height > 0 && !occluded(rect);
      // 原生视图只盖内容区：Agent 控制时占位区域留一圈内边距画边框（见 .browser-pane-view.is-agent）。
      const inset = parseFloat(getComputedStyle(host).paddingTop) || 0;
      const bounds = { x: rect.left + inset, y: rect.top + inset, width: Math.max(0, rect.width - inset * 2), height: Math.max(0, rect.height - inset * 2) };
      const key = show ? `${bounds.x},${bounds.y},${bounds.width},${bounds.height}` : "hidden";
      if (key === last) return;
      last = key;
      void setBrowserPaneBounds(info.id, bounds, show).catch(() => {});
    };
    const schedule = () => { if (!frame) frame = requestAnimationFrame(sync); };
    schedule();
    const resize = new ResizeObserver(schedule);
    resize.observe(host);
    window.addEventListener("resize", schedule);
    const mutations = new MutationObserver(schedule);
    mutations.observe(document.body, { childList: true, subtree: true });
    return () => {
      if (frame) cancelAnimationFrame(frame);
      resize.disconnect();
      mutations.disconnect();
      window.removeEventListener("resize", schedule);
      // 卸载（关面板、换会话）时一定要藏起来：原生视图不会跟着 DOM 消失。
      void setBrowserPaneBounds(info.id, { x: 0, y: 0, width: 0, height: 0 }, false).catch(() => {});
    };
  }, [info.id, showNative, inputLocked]);

  // 新开的空白页签直接把光标放进地址栏。
  useEffect(() => {
    if (visible && blank) inputRef.current?.focus();
  }, [visible, blank]);

  const go = (url: string) => {
    if (inputLocked) return;
    setError(null);
    setDraft(null);
    void navigateBrowserPane(info.id, url).catch((e) => setError(formatBackendError(e, t.locale)));
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const text = (draft ?? "").trim();
    if (text) go(text);
  };
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  useEffect(() => { if (inputLocked) setMenu(null); }, [inputLocked]);
  const act = (action: Parameters<typeof browserPaneAction>[1]) => {
    if (inputLocked) return;
    void browserPaneAction(info.id, action).catch((e) => setError(formatBackendError(e, t.locale)));
  };

  // 权限请求：按钮在答复送达前锁住。后端每次答复的是排在最前的那条，连点两下会把下一条也
  // 一起答掉。按请求编号判断，不按对象引用：标题、加载状态这些无关推送每次都是新对象，会提前
  // 解锁（做空审查）。
  const [answeredId, setAnsweredId] = useState<number | null>(null);
  const answering = info.permission !== null && info.permission?.id === answeredId;
  const answer = (allow: boolean) => {
    if (!info.permission) return;
    setAnsweredId(info.permission.id);
    void answerBrowserPanePermission(info.id, info.permission.id, allow).catch((e) => {
      setAnsweredId(null);
      setError(formatBackendError(e, t.locale));
    });
  };
  const [dismissedBlock, setDismissedBlock] = useState<number | null>(null);
  const blocked = info.blocked && info.blocked.id !== dismissedBlock ? info.blocked : null;
  const permissionText = (prompt: { kind: string; host: string }, blockedText = false) => {
    const what = t.chat.browserPermissionKinds[prompt.kind] ?? t.chat.browserPermissionKinds.other;
    return blockedText ? t.chat.browserPermissionBlocked(prompt.host, what) : t.chat.browserPermissionAsk(prompt.host, what);
  };
  // 只显示最新一次下载；用户点 × 收起的那条不再出现，下一次下载照常出来。
  const [dismissedDownload, setDismissedDownload] = useState<number | null>(null);
  const latestDownload = info.downloads?.at(-1);
  const download = latestDownload && latestDownload.id !== dismissedDownload ? latestDownload : null;

  return (
    <div className="browser-pane" style={visible ? undefined : { display: "none" }}>
      <form className="browser-pane-bar" onSubmit={submit}>
        <button type="button" className="chat-diff-action" data-tip-placement="top" aria-label={t.chat.browserBack} data-tip={t.chat.browserBack} disabled={blank || inputLocked} onClick={() => act("back")}>
          <Icon glyph={icArrowLeft} size={14} />
        </button>
        <button type="button" className="chat-diff-action" data-tip-placement="top" aria-label={t.chat.browserForward} data-tip={t.chat.browserForward} disabled={blank || inputLocked} onClick={() => act("forward")}>
          <Icon glyph={icArrowRight} size={14} />
        </button>
        <button type="button" className={"chat-diff-action" + (info.loading ? " is-loading" : "")} aria-label={t.chat.browserReload} data-tip-placement="top" data-tip={t.chat.browserReload} disabled={blank || inputLocked} onClick={() => act("reload")}>
          <Icon glyph={icRefresh} size={14} />
        </button>
        <input
          ref={inputRef}
          className="browser-pane-url"
          value={draft ?? (blank ? "" : info.url)}
          placeholder={t.chat.browserAddress}
          aria-label={t.chat.browserAddress}
          disabled={inputLocked}
          spellCheck={false}
          onChange={(event) => setDraft(event.target.value)}
          onFocus={(event) => event.target.select()}
          onBlur={() => setDraft(null)}
          onKeyDown={(event) => {
            // Esc 撤销没提交的输入；不往外冒，免得把面板收了。
            if (event.key === "Escape" && draft !== null) {
              event.stopPropagation();
              setDraft(null);
              event.currentTarget.blur();
            }
          }}
        />
        <button type="button" className="chat-diff-action" aria-label={t.chat.browserOpenExternal} data-tip-placement="top" data-tip={t.chat.browserOpenExternal} disabled={blank || inputLocked} onClick={() => void openLink(info.url).catch((e) => setError(formatBackendError(e, t.locale)))}>
          <Icon glyph={icExternal} size={14} />
        </button>
        {/* 「…」菜单：缩放、打印、复制网址。网页里 Ctrl+F / Ctrl+P / Ctrl+滚轮这些 WebView2
            自带的快捷键本来就能用（只有主界面关了浏览器快捷键），这里补个找得到的入口。 */}
        <button
          type="button"
          className="chat-diff-action"
          data-tip-placement="top"
          aria-label={t.chat.browserMore}
          data-tip={t.chat.browserMore}
          aria-haspopup="menu"
          disabled={blank || inputLocked}
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            setMenu(menu ? null : { x: rect.right - 200, y: rect.bottom + 4 });
          }}
        >
          <Icon glyph={icMore} size={14} />
        </button>
      </form>
      {menu && (
        <PathMenu
          x={Math.max(4, menu.x)}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            { key: "zoom_out", label: t.chat.browserZoomOut, icon: null, onSelect: () => act("zoom_out") },
            { key: "zoom_reset", label: t.chat.browserZoomReset, icon: null, hint: `${Math.round((info.zoom ?? 1) * 100)}%`, onSelect: () => act("zoom_reset") },
            { key: "zoom_in", label: t.chat.browserZoomIn, icon: null, onSelect: () => act("zoom_in") },
            "sep",
            { key: "print", label: t.chat.browserPrint, icon: null, hint: "Ctrl+P", onSelect: () => act("print") },
            { key: "copy", label: t.chat.browserCopyUrl, icon: null, onSelect: () => void navigator.clipboard?.writeText(info.url).catch(() => {}) },
          ] satisfies (PathMenuItem | "sep")[]}
        />
      )}
      {info.permission && (
        <div className="browser-pane-notice is-permission" role="alert">
          <Icon glyph={icShield} size={14} className="browser-pane-notice-icon" />
          <span className="browser-pane-notice-text">{permissionText(info.permission)}</span>
          <button type="button" {...busyProps(answering, () => answer(false))}>{t.chat.browserPermissionBlock}</button>
          <button type="button" className="btn-primary" {...busyProps(answering, () => answer(true))}>{t.chat.browserPermissionAllow}</button>
        </div>
      )}
      {/* 阻止之后同一页面不会再问，被拦的下载也悄无声息：留一条，刷新页面就能重新下载。 */}
      {!info.permission && blocked && (
        <div className="browser-pane-notice is-blocked" role="status">
          <Icon glyph={icBan} size={14} className="browser-pane-notice-icon" />
          <span className="browser-pane-notice-text">{permissionText(blocked, true)}</span>
          <button type="button" onClick={() => act("reload")}>{t.chat.browserPermissionRetry}</button>
          <button type="button" className="browser-pane-notice-close" aria-label={t.chat.browserDownloadDismiss} data-tip={t.chat.browserDownloadDismiss} data-tip-placement="top" onClick={() => setDismissedBlock(blocked.id)}>×</button>
        </div>
      )}
      {download && (
        <DownloadNotice
          download={download}
          onReveal={() => void revealBrowserPaneDownload(info.id, download.id).catch((e) => setError(formatBackendError(e, t.locale)))}
          onDismiss={() => setDismissedDownload(download.id)}
        />
      )}
      {error && <div className="browser-pane-error" role="alert">{error}</div>}
      {/* agent 操作时的提示与接管（同 Kimi Code「Agent 正在控制此标签页 / 我来接管 / 已接管时长」）。
          控制中：提示条与网页四周的边框同一个绿色（也是网页里 Agent 鼠标的颜色），连成一体，
          一眼看出「这块现在归 Agent」。紧贴网页放（权限、下载提示在它上面），条和框才连得上；接管后退回中性色，显示已接管多久。 */}
      {(info.agentActive || info.agentPaused) && (
        <div className={"browser-pane-agent" + (info.agentPaused ? " is-paused" : "")} role="status">
          <span className="browser-pane-agent-dot" aria-hidden="true" />
          <span className="browser-pane-agent-text">
            <strong>{info.agentPaused ? t.chat.browserAgentPaused : t.chat.browserAgentActive}</strong>
            {info.agentPaused
              ? <span>{pausedFor} · {t.chat.browserAgentPausedDetail}</span>
              : inputLocked && isWindows() && <span>{t.chat.browserAgentActiveDetail}</span>}
          </span>
          <button type="button" onClick={() => void setBrowserPaneAgentPaused(info.id, !info.agentPaused).catch(() => {})}>
            {info.agentPaused ? t.chat.browserAgentResume : t.chat.browserAgentTakeOver}
          </button>
        </div>
      )}
      <div ref={hostRef} className={"browser-pane-view" + (inputLocked && !blank ? " is-agent" : "")}>
        {blank && (
          <div className="browser-pane-start">
            <p>{t.chat.browserStartHint}</p>
            {suggestions.length > 0 && (
              <ul>
                {suggestions.map((url) => (
                  <li key={url}><button type="button" disabled={inputLocked} onClick={() => go(url)}>{url}</button></li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/** 最新一次下载的提示条：下载中 / 已下载（带「打开文件夹」）/ 失败。 */
function DownloadNotice({ download, onReveal, onDismiss }: { download: BrowserDownload; onReveal: () => void; onDismiss: () => void }) {
  const t = useT();
  const label = download.state === "progress"
    ? t.chat.browserDownloading(download.fileName)
    : download.state === "done"
      ? t.chat.browserDownloaded(download.fileName)
      : t.chat.browserDownloadFailed(download.fileName);
  return (
    <div className={"browser-pane-notice is-download is-" + download.state} role="status">
      <Icon glyph={icDownload} size={14} className="browser-pane-notice-icon" />
      <span className="browser-pane-notice-text" title={download.fileName}>{label}</span>
      {download.state === "done" && <button type="button" onClick={onReveal}>{t.chat.browserDownloadReveal}</button>}
      {download.state !== "progress" && (
        <button type="button" className="browser-pane-notice-close" aria-label={t.chat.browserDownloadDismiss} data-tip={t.chat.browserDownloadDismiss} data-tip-placement="top" onClick={onDismiss}>×</button>
      )}
    </div>
  );
}

/** 页签上显示的名字：标题优先，没有就用主机名，空白页叫「新标签页」。 */
export function browserTabTitle(info: BrowserPaneInfo, blankTitle: string): string {
  if (info.title.trim()) return info.title.trim();
  if (info.url === "about:blank") return blankTitle;
  try {
    return new URL(info.url).host || info.url;
  } catch {
    return info.url;
  }
}

