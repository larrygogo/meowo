import { useEffect, useRef, useState, type FormEvent } from "react";
import { answerBrowserPanePermission, browserPaneAction, navigateBrowserPane, openLink, revealBrowserPaneDownload, setBrowserPaneAgentPaused, setBrowserPaneBounds, type BrowserDownload, type BrowserPaneInfo } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { PathMenu, type PathMenuItem } from "./pathMenu";
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
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
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
      const bounds = { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
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
  }, [info.id, showNative]);

  // 新开的空白页签直接把光标放进地址栏。
  useEffect(() => {
    if (visible && blank) inputRef.current?.focus();
  }, [visible, blank]);

  const go = (url: string) => {
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
  const act = (action: Parameters<typeof browserPaneAction>[1]) => {
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
        <button type="button" className="chat-diff-action" data-tip-placement="top" aria-label={t.chat.browserBack} data-tip={t.chat.browserBack} disabled={blank} onClick={() => act("back")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M19 12H5" /><path d="M12 19l-7-7 7-7" /></svg>
        </button>
        <button type="button" className="chat-diff-action" data-tip-placement="top" aria-label={t.chat.browserForward} data-tip={t.chat.browserForward} disabled={blank} onClick={() => act("forward")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M5 12h14" /><path d="M12 5l7 7-7 7" /></svg>
        </button>
        <button type="button" className={"chat-diff-action" + (info.loading ? " is-loading" : "")} aria-label={t.chat.browserReload} data-tip-placement="top" data-tip={t.chat.browserReload} disabled={blank} onClick={() => act("reload")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M21 12a9 9 0 1 1-3-6.7L21 8" /><path d="M21 3v5h-5" /></svg>
        </button>
        <input
          ref={inputRef}
          className="browser-pane-url"
          value={draft ?? (blank ? "" : info.url)}
          placeholder={t.chat.browserAddress}
          aria-label={t.chat.browserAddress}
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
        <button type="button" className="chat-diff-action" aria-label={t.chat.browserOpenExternal} data-tip-placement="top" data-tip={t.chat.browserOpenExternal} disabled={blank} onClick={() => void openLink(info.url).catch((e) => setError(formatBackendError(e, t.locale)))}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M19 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5" /></svg>
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
          disabled={blank}
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            setMenu(menu ? null : { x: rect.right - 200, y: rect.bottom + 4 });
          }}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="19" cy="12" r="1.6" /></svg>
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
      {/* agent 操作时的提示与接管（同 Kimi Code「你可以随时查看 Agent 的操作并接管」）。 */}
      {(info.agentActive || info.agentPaused) && (
        <div className={"browser-pane-agent" + (info.agentPaused ? " is-paused" : "")} role="status">
          <span className="browser-pane-agent-dot" aria-hidden="true" />
          <span>{info.agentPaused ? t.chat.browserAgentPaused : t.chat.browserAgentActive}</span>
          <button type="button" onClick={() => void setBrowserPaneAgentPaused(info.id, !info.agentPaused).catch(() => {})}>
            {info.agentPaused ? t.chat.browserAgentResume : t.chat.browserAgentTakeOver}
          </button>
        </div>
      )}
      {info.permission && (
        <div className="browser-pane-notice is-permission" role="alert">
          <span className="browser-pane-notice-text">{permissionText(info.permission)}</span>
          <button type="button" disabled={answering} onClick={() => answer(false)}>{t.chat.browserPermissionBlock}</button>
          <button type="button" className="btn-primary" disabled={answering} onClick={() => answer(true)}>{t.chat.browserPermissionAllow}</button>
        </div>
      )}
      {/* 阻止之后同一页面不会再问，被拦的下载也悄无声息：留一条，刷新页面就能重新下载。 */}
      {!info.permission && blocked && (
        <div className="browser-pane-notice is-blocked" role="status">
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
      <div ref={hostRef} className="browser-pane-view">
        {blank && (
          <div className="browser-pane-start">
            <p>{t.chat.browserStartHint}</p>
            {suggestions.length > 0 && (
              <ul>
                {suggestions.map((url) => (
                  <li key={url}><button type="button" onClick={() => go(url)}>{url}</button></li>
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
      <svg className="browser-pane-notice-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 4v11" /><path d="M7 10l5 5 5-5" /><path d="M5 20h14" /></svg>
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

