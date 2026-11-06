import { memo, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import type { BrowserPaneInfo, GitDiffSummaryDto, ShellTerminalInfo } from "../../api";
import { pushEscLayer } from "../../escLayers";
import { useT } from "../../i18n";
import { isMac } from "../../platform";
import { BrowserPane } from "./BrowserPane";
import type { ReviewComment } from "./diffComments";
import { ChangesView, FilesView, PathMenu, type PathMenuItem } from "./GitDiffView";
import type { PanelState, PanelTab, PanelTabType } from "./panelTabs";
import { ShellTerminal } from "./ShellTerminal";
import { tabTitle, type PanelActions } from "./usePanel";

/** 快捷键（与 usePanel 的窗口级监听同一套；照 Kimi Code 1.3.1）。 */
export const PANEL_SHORTCUTS: Record<PanelTabType, string> = {
  diff: "Ctrl+Shift+G",
  browser: "Ctrl+T",
  term: "Ctrl+`",
  files: "Ctrl+Shift+E",
};
const mod = (combo: string) => (isMac() ? combo.replace("Ctrl+", "⌘") : combo);

const ICONS: Record<PanelTabType, ReactNode> = {
  diff: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="9" r="2.5" />
      <path d="M6 8.5v7" /><path d="M18 11.5c0 4-4 4.5-9.2 4.6" />
    </svg>
  ),
  files: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  ),
  term: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 17l6-6-6-6" /><path d="M12 19h8" />
    </svg>
  ),
  browser: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18" />
    </svg>
  ),
};
const CLOSE_ICON = <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>;

type Props = {
  state: PanelState;
  actions: PanelActions;
  shells: ReadonlyMap<number, ShellTerminalInfo>;
  panes: ReadonlyMap<number, BrowserPaneInfo>;
  cwd: string;
  summary: GitDiffSummaryDto | null;
  width: number;
  comments: readonly ReviewComment[];
  onAddComment: (comment: Omit<ReviewComment, "id" | "dir">) => void;
  onRemoveComment: (id: string) => void;
  touchedPaths: readonly string[];
  turnTouchedPaths: readonly string[];
  browserSuggestions: readonly string[];
};

/**
 * 对话窗右侧面板：改动、文件、终端、浏览器同在一排页签里，照 Kimi Code 1.3.1 的面板做
 * （用户要求「整个侧栏的架构和交互参考 Kimi Code」）。模型和规则见 panelTabs.ts / usePanel.ts。
 *
 * 收起只是 display:none，页签内容都不卸载：终端屏幕、文件树展开、diff 缓存再展开原样回来。
 */
export const SidePanel = memo(function SidePanel(props: Props) {
  const { state, actions, shells, panes, cwd, summary, width } = props;
  const t = useT();
  const collapsed = !state.visible;
  const active = state.tabs.find((tab) => tab.key === state.active) ?? null;
  const isRepo = !!summary?.isRepo;
  // 评论框开着时 Esc 只关评论框（同 GitDiffView）。
  const escOverrideRef = useRef<(() => void) | null>(null);

  // Esc 收起面板。挂捕获段：对话窗有「Esc = 拒绝审批」的全局监听，焦点又可能在输入框里。
  // 这些情况让给别人：终端（vim、less 要 Esc）、地址栏（撤销输入）、面板里的菜单、页内弹层。
  useEffect(() => {
    if (collapsed) return;
    const popLayer = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest(".shell-term, .browser-pane-url")) return;
      if (document.querySelector(".chat-modal-overlay")) return;
      if (document.querySelector('.chat-diff-panel [role="menu"]')) return;
      event.stopPropagation();
      if (escOverrideRef.current) escOverrideRef.current();
      else actions.hide();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey, true);
    };
  }, [collapsed, actions]);

  const [menu, setMenu] = useState<{ x: number; y: number; items: (PathMenuItem | "sep")[] } | null>(null);
  const launchItems = (): PathMenuItem[] => {
    const open = new Set(state.tabs.map((tab) => tab.type));
    const items: PathMenuItem[] = [];
    // 改动和文件同时只开一个，开着的就不再列（Kimi 同样如此）；改动只在 git 仓库里有。
    if (isRepo && !open.has("diff")) items.push({ key: "diff", label: t.chat.diffChangesTab, icon: ICONS.diff, hint: mod(PANEL_SHORTCUTS.diff), onSelect: actions.openDiff });
    items.push({ key: "browser", label: t.chat.browserNew, icon: ICONS.browser, hint: mod(PANEL_SHORTCUTS.browser), onSelect: () => actions.newBrowser() });
    items.push({ key: "term", label: t.chat.shellNew, icon: ICONS.term, hint: mod(PANEL_SHORTCUTS.term), onSelect: actions.newTerminal });
    if (!open.has("files")) items.push({ key: "files", label: t.chat.filesTab, icon: ICONS.files, hint: mod(PANEL_SHORTCUTS.files), onSelect: actions.openFiles });
    return items;
  };
  const tabMenu = (tab: PanelTab, x: number, y: number) => {
    const items: (PathMenuItem | "sep")[] = [];
    const info = tab.type === "browser" && tab.paneId !== null ? panes.get(tab.paneId) : undefined;
    if (tab.type === "browser" && info && info.url !== "about:blank") {
      items.push({ key: "copy-url", label: t.chat.browserCopyUrl, icon: null, onSelect: () => void navigator.clipboard?.writeText(info.url).catch(() => {}) });
      items.push("sep");
    }
    items.push(
      { key: "close", label: t.chat.tabClose, icon: null, onSelect: () => actions.close(tab.key) },
      { key: "others", label: t.chat.tabCloseOthers, icon: null, onSelect: () => actions.close(tab.key, "others") },
      { key: "right", label: t.chat.tabCloseRight, icon: null, onSelect: () => actions.close(tab.key, "right") },
      { key: "all", label: t.chat.tabCloseAll, icon: null, onSelect: () => actions.close(tab.key, "all") },
    );
    setMenu({ x, y, items });
  };

  return (
    <aside
      className={"chat-diff-panel side-panel" + (collapsed ? " is-collapsed" : "")}
      style={state.expanded ? undefined : { width }}
      aria-label={t.chat.panelLabel}
    >
      <header className="chat-diff-head" data-tauri-drag-region>
        <TabBar state={state} actions={actions} title={(tab) => tabTitle(tab, shells, panes, t)} onContextMenu={tabMenu} closeLabel={t.chat.tabClose} />
        <span className="chat-diff-spacer" data-tauri-drag-region />
        <button
          type="button"
          className="chat-diff-action"
          data-tip={t.chat.panelNewTab}
          aria-label={t.chat.panelNewTab}
          aria-haspopup="menu"
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            setMenu(menu ? null : { x: rect.right - 220, y: rect.bottom + 4, items: launchItems() });
          }}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
        </button>
        {state.tabs.length > 0 && (
          <button
            type="button"
            className="chat-diff-action"
            data-tip={state.expanded ? t.chat.diffRestore : t.chat.diffMaximize}
            aria-label={state.expanded ? t.chat.diffRestore : t.chat.diffMaximize}
            onClick={actions.toggleExpanded}
          >
            {state.expanded ? (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M4 14h6v6" /><path d="M20 10h-6V4" /><path d="M14 10l7-7" /><path d="M3 21l7-7" />
              </svg>
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M15 3h6v6" /><path d="M9 21H3v-6" /><path d="M21 3l-7 7" /><path d="M3 21l7-7" />
              </svg>
            )}
          </button>
        )}
        <button type="button" className="chat-diff-action" data-tip={t.chat.diffCollapse} aria-label={t.chat.diffCollapse} onClick={actions.hide}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
            <rect x="3.5" y="4.5" width="17" height="15" rx="3" />
            <path d="M14.5 4.5v15" />
          </svg>
        </button>
      </header>
      {menu && <PathMenu x={Math.max(4, menu.x)} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}

      {state.tabs.length === 0 && (
        <nav className="side-panel-launcher" aria-label={t.chat.panelLaunch}>
          <p>{t.chat.panelLaunch}</p>
          {launchItems().map((item) => (
            <button key={item.key} type="button" onClick={item.onSelect}>
              {item.icon}
              <span>{item.label}</span>
              {item.hint && <kbd>{item.hint}</kbd>}
            </button>
          ))}
        </nav>
      )}

      {/* 终端、浏览器常驻挂载只切显隐：切走再回来，屏幕、滚动位置、网页都还在。 */}
      {state.tabs.map((tab) => {
        const visible = !collapsed && tab.key === state.active;
        if (tab.type === "term") {
          return tab.shellId !== null && shells.has(tab.shellId)
            ? <ShellTerminal key={tab.key} id={tab.shellId} visible={visible} />
            : visible ? <div key={tab.key} className="side-panel-pending" /> : null;
        }
        if (tab.type === "browser") {
          const info = tab.paneId !== null ? panes.get(tab.paneId) : undefined;
          return info
            ? <BrowserPane key={tab.key} info={info} visible={visible} suggestions={props.browserSuggestions} />
            : visible ? <div key={tab.key} className="side-panel-pending" /> : null;
        }
        return null;
      })}
      {active?.type === "diff" && (isRepo && summary
        ? (
          <ChangesView
            key={cwd}
            cwd={cwd}
            summary={summary}
            comments={props.comments.filter((comment) => comment.dir === cwd)}
            touchedPaths={props.touchedPaths}
            turnTouchedPaths={props.turnTouchedPaths}
            onAddComment={props.onAddComment}
            onRemoveComment={props.onRemoveComment}
            escOverrideRef={escOverrideRef}
          />
        )
        : <p className="side-panel-empty">{t.chat.diffNoRepo}</p>)}
      {active?.type === "files" && <FilesView key={cwd} cwd={cwd} />}
    </aside>
  );
});

/**
 * 页签栏：点击切换，叉或中键关闭，右键菜单（关闭 / 关闭其他 / 关闭右侧 / 关闭所有），
 * 拖动排序，键盘 ←/→/Home/End 切换、Alt+←/→ 挪位置（同 Kimi Code）。
 *
 * 拖动用 pointer 事件自己做：对话窗开着 Tauri 的文件拖放，WebView2 上 HTML5 拖放会被它截走。
 */
function TabBar({ state, actions, title, onContextMenu, closeLabel }: {
  state: PanelState;
  actions: PanelActions;
  title: (tab: PanelTab) => string;
  onContextMenu: (tab: PanelTab, x: number, y: number) => void;
  closeLabel: string;
}) {
  const barRef = useRef<HTMLDivElement | null>(null);
  const [drag, setDrag] = useState<{ key: string; index: number } | null>(null);
  const dragRef = useRef<{ key: string; startX: number; moved: boolean; pointerId: number } | null>(null);

  // 当前页签滚进可见区域（页签多到横向滚动时）。
  useEffect(() => {
    barRef.current?.querySelector<HTMLElement>(".side-panel-tab.is-active")?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [state.active]);

  const dropIndex = (clientX: number): number => {
    const nodes = Array.from(barRef.current?.querySelectorAll<HTMLElement>(".side-panel-tab") ?? []);
    const at = nodes.findIndex((node) => {
      const rect = node.getBoundingClientRect();
      return clientX < rect.left + rect.width / 2;
    });
    return at < 0 ? nodes.length : at;
  };
  const onPointerDown = (tab: PanelTab) => (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    dragRef.current = { key: tab.key, startX: event.clientX, moved: false, pointerId: event.pointerId };
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const current = dragRef.current;
    if (!current) return;
    if (!current.moved && Math.abs(event.clientX - current.startX) < 5) return;
    if (!current.moved) {
      current.moved = true;
      event.currentTarget.setPointerCapture(current.pointerId);
    }
    setDrag({ key: current.key, index: dropIndex(event.clientX) });
  };
  const onPointerUp = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const current = dragRef.current;
    dragRef.current = null;
    if (!current?.moved) return;
    const index = dropIndex(event.clientX);
    const from = state.tabs.findIndex((tab) => tab.key === current.key);
    // 落点下标是「插到第 index 个之前」，挪走自己后右侧下标要减一。
    actions.move(current.key, index > from ? index - 1 : index);
    setDrag(null);
  };
  const onKeyDown = (tab: PanelTab, index: number) => (event: ReactKeyboardEvent) => {
    const last = state.tabs.length - 1;
    if (event.altKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault();
      actions.move(tab.key, index + (event.key === "ArrowLeft" ? -1 : 1));
      return;
    }
    const target = event.key === "ArrowLeft" ? index - 1 : event.key === "ArrowRight" ? index + 1 : event.key === "Home" ? 0 : event.key === "End" ? last : null;
    if (target === null || event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    const next = state.tabs[Math.max(0, Math.min(target, last))];
    actions.activate(next.key);
    barRef.current?.querySelectorAll<HTMLElement>(".side-panel-tab")[state.tabs.indexOf(next)]?.focus();
  };

  return (
    <div ref={barRef} className="side-panel-tabs" role="tablist">
      {state.tabs.map((tab, index) => {
        const isActive = tab.key === state.active;
        const name = title(tab);
        return (
          <button
            key={tab.key}
            type="button"
            role="tab"
            aria-selected={isActive}
            tabIndex={isActive ? 0 : -1}
            className={"side-panel-tab" + (isActive ? " is-active" : "") + (drag?.key === tab.key ? " is-dragging" : "") + (drag && drag.index === index && drag.key !== tab.key ? " is-drop-before" : "")}
            title={tab.type === "browser" ? tab.url : name}
            onClick={() => { if (!dragRef.current?.moved) actions.activate(tab.key); }}
            onAuxClick={(event) => { if (event.button === 1) { event.preventDefault(); actions.close(tab.key); } }}
            onContextMenu={(event) => { event.preventDefault(); onContextMenu(tab, event.clientX, event.clientY); }}
            onPointerDown={onPointerDown(tab)}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={() => { dragRef.current = null; setDrag(null); }}
            onKeyDown={onKeyDown(tab, index)}
          >
            {ICONS[tab.type]}
            <span className="side-panel-tab-name">{name}</span>
            <span
              role="button"
              tabIndex={-1}
              className="side-panel-tab-close"
              aria-label={closeLabel}
              onPointerDown={(event) => event.stopPropagation()}
              onClick={(event) => { event.stopPropagation(); actions.close(tab.key); }}
            >
              {CLOSE_ICON}
            </span>
          </button>
        );
      })}
      {drag && drag.index === state.tabs.length && <span className="side-panel-drop-end" />}
    </div>
  );
}
