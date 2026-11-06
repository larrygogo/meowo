import { useEffect, useRef, useState, type FormEvent } from "react";
import { browserPaneAction, navigateBrowserPane, openLink, setBrowserPaneAgentPaused, setBrowserPaneBounds, type BrowserPaneInfo } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { PathMenu, type PathMenuItem } from "./GitDiffView";

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

/** 从对话文本里挑出本机地址（dev server），去重、最近出现的在前，最多 5 个。 */
export function localUrlsIn(texts: readonly string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  // 路径里遇到中文标点就停：agent 的中文回复里网址后面常直接跟「。」「，」。
  const pattern = /https?:\/\/(?:localhost|127\.\d+\.\d+\.\d+|0\.0\.0\.0|\[::1\])(?::\d+)?(?:\/[^\s"'`<>)\]，。；：！？、（）「」《》]*)?/gi;
  for (let i = texts.length - 1; i >= 0 && out.length < 5; i -= 1) {
    const matches = texts[i].match(pattern) ?? [];
    for (let k = matches.length - 1; k >= 0 && out.length < 5; k -= 1) {
      const url = matches[k].replace(/[.,;:!?]+$/, "");
      if (seen.has(url)) continue;
      seen.add(url);
      out.push(url);
    }
  }
  return out;
}
