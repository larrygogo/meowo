import { memo, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import type { BrowserPaneInfo, GitDiffSummaryDto, RepoChangesDto, ShellTerminalInfo } from "../../api";
import { pushEscLayer } from "../../escLayers";
import { useT } from "../../i18n";
import { BrowserPane } from "./BrowserPane";
import type { ReviewComment } from "./diffComments";
import { singleRepo } from "./diffShared";
import { LazyFilesView as FilesView } from "./lazyDiff";
import { PathMenu, PathMentionContext, type PathMenuItem } from "./pathMenu";
import { pickRepo, SourceControl } from "./SourceControl";

const sameDir = (a: string, b: string) => a.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase() === b.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
import type { PanelState, PanelTab } from "./panelTabs";
import { PANEL_ICONS as ICONS, PANEL_SHORTCUTS, shortcutLabel as mod } from "./panelIcons";
import { ShellTerminal } from "./ShellTerminal";
import { tabTitle, type PanelActions } from "./usePanel";
import { Icon } from "../../icons/Icon";
import { icCollapse, icExpand, icSidebarRight } from "../../icons/glyphs.chat";
import { icChevronDown, icClose, icPlus } from "../../icons/glyphs";

const CLOSE_ICON = <Icon glyph={icClose} size={12} />;

type Props = {
  state: PanelState;
  actions: PanelActions;
  shells: ReadonlyMap<number, ShellTerminalInfo>;
  panes: ReadonlyMap<number, BrowserPaneInfo>;
  cwd: string;
  summary: GitDiffSummaryDto | null;
  /** 多仓库「更改」（见 SourceControl）；null = 还没拉到，按 summary 单仓库显示。 */
  repos?: readonly RepoChangesDto[] | null;
  width: number;
  comments: readonly ReviewComment[];
  onAddComment: (comment: Omit<ReviewComment, "id" | "dir">, dir: string) => void;
  onRemoveComment: (id: string) => void;
  touchedPaths: readonly string[];
  turnTouchedPaths: readonly string[];
  browserSuggestions: readonly string[];
  /** 文件/改动的路径菜单里「添加到对话」：把相对路径插进输入框。 */
  onMention?: (rel: string) => void;
  /** 「更改」「文件」共用的选中仓库（会话开在主目录、改动在 worktree 里时两边一起切过去，用户要求 2026-11-09）。 */
  selectedRoot: string | null;
  onSelectRoot: (root: string) => void;
  /** git 写操作之后立刻重拉清单。 */
  onChanged: () => void;
  /** 会话 id：PR 合并后提示清理它的 worktree（见 ForgeBar）。 */
  sessionId?: number | null;
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
  const isRepo = !!summary?.isRepo || (props.repos?.length ?? 0) > 0;
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

  const [overflowing, setOverflowing] = useState(false);
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
        <TabBar
          state={state}
          actions={actions}
          title={(tab) => tabTitle(tab, shells, panes, t)}
          onContextMenu={tabMenu}
          closeLabel={t.chat.tabClose}
          agentBusy={(tab) => tab.type === "browser" && tab.paneId !== null && !!panes.get(tab.paneId)?.agentActive}
          agentLabel={t.chat.browserAgentActive}
          waiting={(tab) => tab.type === "browser" && tab.paneId !== null && !!panes.get(tab.paneId)?.permission}
          waitingLabel={t.chat.browserPermissionWaiting}
          onOverflow={setOverflowing}
        />
        <span className="chat-diff-spacer" data-tauri-drag-region />
        {/* 页签放不下时：列出全部页签，点哪个跳到哪个。 */}
        {overflowing && (
          <button
            type="button"
            className="chat-diff-action"
            data-tip={t.chat.panelAllTabs}
            aria-label={t.chat.panelAllTabs}
            aria-haspopup="menu"
            onClick={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              setMenu(menu ? null : {
                x: rect.right - 240,
                y: rect.bottom + 4,
                items: state.tabs.map((tab) => ({
                  key: tab.key,
                  label: tabTitle(tab, shells, panes, t),
                  icon: ICONS[tab.type],
                  hint: tab.key === state.active ? t.chat.panelCurrentTab : undefined,
                  onSelect: () => actions.activate(tab.key),
                })),
              });
            }}
          >
            <Icon glyph={icChevronDown} size={14} />
          </button>
        )}
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
          <Icon glyph={icPlus} size={14} />
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
              <Icon glyph={icCollapse} size={14} />
            ) : (
              <Icon glyph={icExpand} size={14} />
            )}
          </button>
        )}
        <button type="button" className="chat-diff-action" data-tip={t.chat.diffCollapse} aria-label={t.chat.diffCollapse} onClick={actions.hide}>
          <Icon glyph={icSidebarRight} size={14} />
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
      <PathMentionContext.Provider value={props.onMention ?? null}>
      {active?.type === "diff" && ((props.repos && props.repos.length > 0) || (summary?.isRepo)
        ? (
          <SourceControl
            key={cwd}
            cwd={cwd}
            repos={props.repos && props.repos.length > 0 ? props.repos : singleRepo(cwd, summary!)}
            selectedRoot={props.selectedRoot}
            onSelectRoot={props.onSelectRoot}
            onChanged={props.onChanged}
            comments={props.comments}
            touchedPaths={props.touchedPaths}
            turnTouchedPaths={props.turnTouchedPaths}
            onAddComment={props.onAddComment}
            onRemoveComment={props.onRemoveComment}
            escOverrideRef={escOverrideRef}
            live={!collapsed}
            sessionId={props.sessionId ?? null}
            visible={!collapsed}
          />
        )
        : <p className="side-panel-empty">{t.chat.diffNoRepo}</p>)}
      {active?.type === "files" && (() => {
        // 「文件」跟着「更改」选中的仓库走：worktree 不在工作目录下，不切过去就看不到它的文件。
        // 选的是工作目录所在仓库（或显式选了「工作目录」）就浏览工作目录本身——它可能是仓库的子目录，
        // 也可能根本不是仓库（会话开在 workspace）。顶上一排可切换，与「更改」共用同一个选中（做空：此前
        // 「文件」里切不了，工作目录不是仓库时还再也回不去）。
        const others = (props.repos ?? []).filter((repo) => !repo.isCwdRepo);
        const workdirPicked = !props.selectedRoot || sameDir(props.selectedRoot, cwd);
        const picked = !workdirPicked && props.repos && props.repos.length > 0 ? pickRepo(props.repos, props.selectedRoot) : null;
        const root = picked && !picked.isCwdRepo ? picked.root : cwd;
        return (
          <>
            {others.length > 0 && (
              <div className="files-roots" role="group" aria-label={t.chat.filesRepoSwitch}>
                <button type="button" aria-pressed={root === cwd} data-tip={cwd} onClick={() => props.onSelectRoot(cwd)}>{t.chat.filesWorkdir}</button>
                {others.map((repo) => (
                  <button key={repo.root} type="button" aria-pressed={root !== cwd && sameDir(repo.root, root)} data-tip={repo.root} onClick={() => props.onSelectRoot(repo.root)}>{repo.name}</button>
                ))}
              </div>
            )}
            <FilesView key={root} cwd={root} live={!collapsed} />
          </>
        );
      })()}
      </PathMentionContext.Provider>
    </aside>
  );
});

/**
 * 页签栏：点击切换，叉或中键关闭，右键菜单（关闭 / 关闭其他 / 关闭右侧 / 关闭所有），
 * 拖动排序，键盘 ←/→/Home/End 切换、Alt+←/→ 挪位置（同 Kimi Code）。
 *
 * 拖动用 pointer 事件自己做：对话窗开着 Tauri 的文件拖放，WebView2 上 HTML5 拖放会被它截走。
 */
/// 页签栏要不要收成只留图标：面板头宽度装不下「每个页签 96px（图标 + 三四个字 + 关闭叉）+ 右侧
/// 操作钮约 150px」就收。数值按 100% 缩放计，乘界面缩放。量不出宽度（jsdom、面板隐藏）时不收。
/// 只读面板头宽度：页签自己变窄、「更多」钮出没都不改变结果，收放不会自激。
export function tabsCompact(headWidth: number, tabs: number, ui: number): boolean {
  if (headWidth <= 0 || tabs <= 0) return false;
  return headWidth < (tabs * 96 + 150) * ui;
}

function TabBar({ state, actions, title, onContextMenu, closeLabel, agentBusy, agentLabel, waiting, waitingLabel, onOverflow }: {
  state: PanelState;
  actions: PanelActions;
  title: (tab: PanelTab) => string;
  onContextMenu: (tab: PanelTab, x: number, y: number) => void;
  closeLabel: string;
  /** 这个页签此刻是不是 agent 在操作（浏览器页签上亮一个点）。 */
  agentBusy: (tab: PanelTab) => boolean;
  agentLabel: string;
  /** 页签里的网页在等用户答复权限请求。 */
  waiting: (tab: PanelTab) => boolean;
  waitingLabel: string;
  /** 页签放不下（需要横向滚动）时通知外面，好显示「全部页签」按钮。 */
  onOverflow: (overflowing: boolean) => void;
}) {
  const barRef = useRef<HTMLDivElement | null>(null);
  /** 拖动中：被拖的页签、落点下标、指示线位置（相对页签栏内容的坐标，随横向滚动走）。 */
  const [drag, setDrag] = useState<{ key: string; index: number; x: number; top: number; height: number } | null>(null);
  const dragRef = useRef<{ key: string; startX: number; moved: boolean; pointerId: number } | null>(null);

  // 放不下时：两端渐隐提示还有内容；竖向滚轮转成横向滚动（之前滚动条隐藏、滚轮又只会
  // 上下滚，后面的页签根本看不到，实拍反馈）。
  const [edges, setEdges] = useState({ left: false, right: false });
  // 面板头放不下「每个页签都带名字」时收成只留图标（名字进悬停提示与无障碍名）：否则页签被压到
  // 72px 下限，名字只剩一个字加省略号，还滚到面板操作钮底下（做空审查：620 宽开面板，名字可视宽
  // 14～21px）。判据见 tabsCompact：只看面板头宽度，不看页签自身，收放不会来回跳。
  const [compact, setCompact] = useState(false);
  const onOverflowRef = useRef(onOverflow);
  onOverflowRef.current = onOverflow;
  useEffect(() => {
    const bar = barRef.current;
    if (!bar) return;
    const measure = () => {
      const left = bar.scrollLeft > 1;
      const right = bar.scrollLeft + bar.clientWidth < bar.scrollWidth - 1;
      setEdges((prev) => (prev.left === left && prev.right === right ? prev : { left, right }));
      onOverflowRef.current(left || right);
      const head = bar.parentElement;
      if (head) {
        const ui = parseFloat(getComputedStyle(head).getPropertyValue("--cc-ui")) || 1;
        setCompact(tabsCompact(head.clientWidth, state.tabs.length, ui));
      }
    };
    const wheel = (event: WheelEvent) => {
      if (bar.scrollWidth <= bar.clientWidth || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;
      event.preventDefault();
      bar.scrollLeft += event.deltaY;
    };
    measure();
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    resize?.observe(bar);
    if (bar.parentElement) resize?.observe(bar.parentElement);
    bar.addEventListener("scroll", measure);
    bar.addEventListener("wheel", wheel, { passive: false });
    return () => {
      resize?.disconnect();
      bar.removeEventListener("scroll", measure);
      bar.removeEventListener("wheel", wheel);
    };
  }, [state.tabs]);

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
  // 指示线落在两个页签的缝正中；首尾两端贴着页签外沿，再往里收 1px，不被滚动容器裁掉
  // （实拍：插到第一个位置时线画在容器外看不见，插到末尾时线飘在叉号空位外面）。
  const dropLine = (index: number) => {
    const bar = barRef.current;
    const nodes = Array.from(bar?.querySelectorAll<HTMLElement>(".side-panel-tab") ?? []);
    const ref = nodes[Math.min(index, nodes.length - 1)];
    if (!bar || !ref) return { index, x: 0, top: 0, height: 0 };
    let x: number;
    if (index <= 0) x = ref.offsetLeft + 1;
    else if (index >= nodes.length) x = ref.offsetLeft + ref.offsetWidth - 1;
    else {
      const prev = nodes[index - 1];
      x = (prev.offsetLeft + prev.offsetWidth + ref.offsetLeft) / 2;
    }
    return { index, x: Math.max(1, Math.min(x, bar.scrollWidth - 1)), top: ref.offsetTop + 4, height: ref.offsetHeight - 8 };
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
    setDrag({ key: current.key, ...dropLine(dropIndex(event.clientX)) });
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
    <div ref={barRef} className={"side-panel-tabs" + (edges.left ? " has-left" : "") + (edges.right ? " has-right" : "") + (compact ? " is-compact" : "")} role="tablist">
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
            className={"side-panel-tab" + (isActive ? " is-active" : "") + (drag?.key === tab.key ? " is-dragging" : "")}
            // 只留图标时名字不可见：无障碍名显式给，悬停走应用内提示（data-tip），不再叠一个原生 title。
            aria-label={name}
            title={compact ? undefined : tab.type === "browser" ? tab.url : name}
            data-tip={compact ? name : undefined}
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
            {waiting(tab)
              ? <span className="side-panel-tab-waiting" title={waitingLabel} aria-label={waitingLabel} />
              : agentBusy(tab) && <span className="side-panel-tab-agent" title={agentLabel} aria-label={agentLabel} />}
            {/* 关闭叉在页签按钮里面：按钮的子元素对读屏是「纯展示」，嵌套的 role=button 本来就
                读不到（axe nested-interactive）。读屏/键盘关页签走右键菜单（菜单键同样触发）。 */}
            <span
              aria-hidden="true"
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
      {drag && <span className="side-panel-drop-line" style={{ left: drag.x - 1, top: drag.top, height: drag.height }} />}
    </div>
  );
}
