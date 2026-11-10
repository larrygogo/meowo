import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  closeBrowserPane, closeShellTerminal, listBrowserPanes, listShellTerminals, openBrowserPane, openShellTerminal, shellTerminalBusy,
  type BrowserPaneInfo, type ShellTerminalInfo,
} from "../../api";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { remoteUi } from "../../remoteMode";
import {
  activate, activeTab, addTab, closeTabs, cycle, EMPTY_PANEL, forgetPanel, keysToClose, loadPanel, movePanel, moveTab, newTabKey, openSingleton,
  savePanel, setVisible, updateTab, type PanelState, type PanelTab,
} from "./panelTabs";
import { confirmCloseShells, moveShellOwner, trackShell, untrackShell, untrackShellOwner } from "./windowClose";

export type PanelActions = ReturnType<typeof usePanel>["actions"];

/**
 * 对话窗右侧面板的状态与动作（模型见 panelTabs.ts）。
 *
 * 页签清单按会话存在 localStorage；终端和浏览器的进程在后端，这里按编号对上号：
 * 切到一个没有活进程的终端 / 浏览器页签（新开的，或应用重启过），就按存下的目录 / 网址重新起一个。
 */
export function usePanel(sessionId: number, cwd: string | null, onError: (message: string) => void, ended = false) {
  const t = useT();
  const [panel, setPanel] = useState<{ sessionId: number; state: PanelState }>(() => ({ sessionId, state: loadPanel(sessionId) }));
  // 换会话：先存旧会话的（保存 effect 已经在每次变化时存过），再读新会话的。
  // 渲染期间直接换掉，避免新会话先用旧会话的页签渲染一帧。
  let current = panel;
  if (panel.sessionId !== sessionId) {
    current = { sessionId, state: loadPanel(sessionId) };
    setPanel(current);
  }
  const state = current.state;
  useEffect(() => {
    if (remoteUi()) return;
    savePanel(panel.sessionId, panel.state);
  }, [panel]);

  const stateRef = useRef(state);
  stateRef.current = state;
  const update = useCallback((change: (state: PanelState) => PanelState) => {
    setPanel((prev) => ({ ...prev, state: change(prev.state) }));
  }, []);

  // ── 后端进程的实时信息 ──
  const [shells, setShells] = useState<ReadonlyMap<number, ShellTerminalInfo>>(new Map());
  const [panes, setPanes] = useState<ReadonlyMap<number, BrowserPaneInfo>>(new Map());
  // 已经从后端取到进程清单的是哪个会话。必须按会话记：换会话的那一帧，清单还是上一个
  // 会话的，这个会话的终端 / 浏览器页签在里面对不上号，会被当成「进程没了」再起一个；
  // 等本会话清单回来，原来那个又被当成没人认领补成页签——每切回来一次多一个（实拍）。
  const [liveFor, setLiveFor] = useState<number | null>(null);
  // 会话换了编号后要重新取一次本会话的进程清单（见 panel-session-moved）。
  const [reloadTick, setReloadTick] = useState(0);
  const liveLoaded = liveFor === sessionId;
  useEffect(() => {
    if (remoteUi()) return;
    let cancelled = false;
    Promise.all([listShellTerminals(sessionId), listBrowserPanes(sessionId)])
      .then(([shellList, paneList]) => {
        if (cancelled) return;
        // 关窗确认要问全部会话的终端，这里登记到窗口级账本（windowClose.ts）。
        for (const info of shellList) trackShell(info.id, info.owner);
        setShells(new Map(shellList.map((info) => [info.id, info])));
        setPanes(new Map(paneList.map((info) => [info.id, info])));
        // 后端开着、页签里却没有的（比如存储被清过），补成页签，别让进程无处可关。
        update((prev) => {
          let next = prev;
          for (const info of shellList) {
            if (!next.tabs.some((tab) => tab.type === "term" && tab.shellId === info.id)) {
              next = { ...next, tabs: [...next.tabs, { key: newTabKey("term"), type: "term", shellId: info.id, cwd: cwd ?? "" }] };
            }
          }
          for (const info of paneList) {
            if (!next.tabs.some((tab) => tab.type === "browser" && tab.paneId === info.id)) {
              next = { ...next, tabs: [...next.tabs, { key: newTabKey("browser"), type: "browser", paneId: info.id, url: info.url, title: info.title }] };
            }
          }
          return next;
        });
        setLiveFor(sessionId);
      })
      .catch(() => { if (!cancelled) setLiveFor(sessionId); });
    return () => { cancelled = true; };
  // cwd 只在补页签时兜底用，不因它重拉。
  }, [sessionId, update, reloadTick]);

  // agent 第一次用浏览器时后端自己建的页签（browser_agent.rs）：加进这个会话的面板并切过去，
  // 用户能看着它操作。不是正在看的会话就等切过去时由列表补上。
  useTauriEvent<BrowserPaneInfo>("browser-pane-created", ({ payload: info }) => {
    if (info.owner !== sessionId) return;
    setPanes((prev) => new Map(prev).set(info.id, info));
    update((prev) => {
      const existing = prev.tabs.find((tab) => tab.type === "browser" && tab.paneId === info.id);
      if (existing) return activate(prev, existing.key);
      return addTab(prev, { key: newTabKey("browser"), type: "browser", paneId: info.id, url: info.url, title: info.title });
    });
  });
  // 会话结束或被关闭（标记完成）：后端已经关了它的终端和浏览器（browser_agent.rs 的
  // close_session_panels），这里清掉页签和记录（用户要求「会话结束了就同步关闭里面的窗口」）。
  // 会话换了编号（新会话认领真实编号、/clear 换代）：后端已把进程归属换过去，这里把页签
  // 记录也挪过去。对话窗还没切过去的，切过去时按新编号读到；已经切过去的当场重读。
  useTauriEvent<{ from: number; to: number }>("panel-session-moved", ({ payload: { from, to } }) => {
    if (from === sessionId) savePanel(from, stateRef.current);
    movePanel(from, to);
    moveShellOwner(from, to);
    if (to !== sessionId) return;
    setPanel({ sessionId, state: loadPanel(sessionId) });
    setLiveFor(null);
    setReloadTick((tick) => tick + 1);
  });
  useTauriEvent<number>("panel-session-closed", ({ payload: closed }) => {
    forgetPanel(closed);
    untrackShellOwner(closed);
    // 后端已经关了这个会话的进程，本地缓存里它们的状态也作废。
    setPanes((prev) => new Map([...prev].filter(([, info]) => info.owner !== closed)));
    setShells((prev) => new Map([...prev].filter(([, info]) => info.owner !== closed)));
    if (closed !== sessionId) return;
    setPanel({ sessionId, state: EMPTY_PANEL });
  });
  useTauriEvent<BrowserPaneInfo>("browser-pane-state", ({ payload: info }) => {
    setPanes((prev) => (prev.has(info.id) ? new Map(prev).set(info.id, info) : prev));
    // 网址和标题也记进页签，应用重启后按它重新打开。
    update((prev) => {
      const tab = prev.tabs.find((item) => item.type === "browser" && item.paneId === info.id);
      if (!tab) return prev;
      return updateTab(prev, tab.key, (item) => (item.type === "browser" && (item.url !== info.url || item.title !== info.title)
        ? { ...item, url: info.url, title: info.title }
        : item));
    });
  });

  // ── 会话已经结束：进程早没了的终端 / 浏览器页签直接去掉，不再按记录重新打开 ──
  // 实拍：会话结束后 dev 重启过，面板记录还在，一点开就把网页和终端又全开了回来。
  // 只去掉「起过、现在没了」的；结束后用户自己新开的（编号还是空的）照常起。
  useEffect(() => {
    if (remoteUi() || !ended || !liveLoaded) return;
    const stale = state.tabs.filter((tab) => (tab.type === "term" && tab.shellId !== null && !shells.has(tab.shellId))
      || (tab.type === "browser" && tab.paneId !== null && !panes.has(tab.paneId)));
    if (stale.length > 0) update((prev) => closeTabs(prev, stale.map((tab) => tab.key)));
  }, [ended, liveLoaded, state.tabs, shells, panes, update]);

  // ── 切到没有活进程的终端 / 浏览器页签：按存下的目录 / 网址起一个 ──
  const spawning = useRef(new Set<string>());
  const active = activeTab(state);
  useEffect(() => {
    if (remoteUi() || !liveLoaded || !state.visible || !active) return;
    if (spawning.current.has(active.key)) return;
    // 已结束的会话不补开旧进程（上面的 effect 会把这种页签去掉）。
    if (ended && ((active.type === "term" && active.shellId !== null) || (active.type === "browser" && active.paneId !== null))) return;
    const owner = sessionId;
    const key = active.key;
    if (active.type === "term" && (active.shellId === null || !shells.has(active.shellId))) {
      const dir = active.cwd || cwd;
      if (!dir) return;
      spawning.current.add(key);
      openShellTerminal(owner, dir, 80, 24)
        .then((info) => {
          trackShell(info.id, info.owner);
          setShells((prev) => new Map(prev).set(info.id, info));
          update((prev) => updateTab(prev, key, (tab) => (tab.type === "term" ? { ...tab, shellId: info.id, cwd: dir } : tab)));
        })
        .catch((e) => onError(formatBackendError(e, t.locale)))
        .finally(() => spawning.current.delete(key));
    } else if (active.type === "browser" && (active.paneId === null || !panes.has(active.paneId))) {
      spawning.current.add(key);
      openBrowserPane(owner, active.url === "about:blank" ? undefined : active.url)
        .then((info) => {
          setPanes((prev) => new Map(prev).set(info.id, info));
          update((prev) => updateTab(prev, key, (tab) => (tab.type === "browser" ? { ...tab, paneId: info.id } : tab)));
        })
        .catch((e) => onError(formatBackendError(e, t.locale)))
        .finally(() => spawning.current.delete(key));
    }
  }, [active, state.visible, liveLoaded, shells, panes, sessionId, cwd, update, onError, t.locale, ended]);

  // ── 关闭：终端里还有程序在跑先确认（参照 Kimi Code「关闭正在运行的终端？」）──
  const close = useCallback(async (keys: readonly string[]) => {
    const tabs = stateRef.current.tabs.filter((tab) => keys.includes(tab.key));
    const shellIds = tabs.flatMap((tab) => (tab.type === "term" && tab.shellId !== null ? [tab.shellId] : []));
    // 与关整扇窗共用同一份确认（windowClose.ts）。
    if (!(await confirmCloseShells(shellIds, t))) return;
    for (const tab of tabs) {
      if (tab.type === "term" && tab.shellId !== null) {
        untrackShell(tab.shellId);
        void closeShellTerminal(tab.shellId).catch(() => {});
      }
      if (tab.type === "browser" && tab.paneId !== null) void closeBrowserPane(tab.paneId).catch(() => {});
    }
    // 关掉的进程从本地缓存里删掉：留着的话，关之前那份「agent 正在操作」会一直亮在标题栏上（实拍）。
    const closedPanes = new Set(tabs.flatMap((tab) => (tab.type === "browser" && tab.paneId !== null ? [tab.paneId] : [])));
    const closedShells = new Set(shellIds);
    if (closedPanes.size > 0) setPanes((prev) => new Map([...prev].filter(([id]) => !closedPanes.has(id))));
    if (closedShells.size > 0) setShells((prev) => new Map([...prev].filter(([id]) => !closedShells.has(id))));
    update((prev) => closeTabs(prev, keys));
  }, [t, update]);

  const actions = useMemo(() => ({
    activate: (key: string) => update((prev) => activate(prev, key)),
    move: (key: string, index: number) => update((prev) => moveTab(prev, key, index)),
    close: (key: string, scope: "one" | "others" | "right" | "all" = "one") => void close(keysToClose(stateRef.current, key, scope)),
    openDiff: () => update((prev) => openSingleton(prev, "diff")),
    openFiles: () => update((prev) => openSingleton(prev, "files")),
    newTerminal: () => {
      if (!cwd) return;
      update((prev) => addTab(prev, { key: newTabKey("term"), type: "term", shellId: null, cwd }));
    },
    newBrowser: (url?: string) => update((prev) => addTab(prev, { key: newTabKey("browser"), type: "browser", paneId: null, url: url ?? "about:blank", title: "" })),
    show: () => update((prev) => setVisible(prev, true)),
    hide: () => update((prev) => setVisible(prev, false)),
    toggle: () => update((prev) => setVisible(prev, !prev.visible)),
    toggleExpanded: () => update((prev) => ({ ...prev, expanded: !prev.expanded, visible: true })),
    cycle: (step: 1 | -1) => update((prev) => cycle(prev, step)),
    /** 文件：当前就在看文件就收起面板，否则打开或切过去（Kimi 的 Shift+Ctrl+E）。 */
    toggleFiles: () => update((prev) => {
      const tab = activeTab(prev);
      return prev.visible && tab?.type === "files" ? setVisible(prev, false) : openSingleton(prev, "files");
    }),
    /** 面板终端：正在看终端就收起；有终端切到最后一个；一个都没有就新开（Ctrl+`）。 */
    toggleTerminal: () => update((prev) => {
      const tab = activeTab(prev);
      if (prev.visible && tab?.type === "term") return setVisible(prev, false);
      const last = [...prev.tabs].reverse().find((item) => item.type === "term");
      if (last) return activate(prev, last.key);
      if (!cwd) return prev;
      return addTab(prev, { key: newTabKey("term"), type: "term", shellId: null, cwd });
    }),
    /** 对话里的本机链接：已经有页签开着同一个地址就切过去，否则新开。 */
    openUrl: (url: string) => update((prev) => {
      const same = prev.tabs.find((tab) => tab.type === "browser" && tab.url.replace(/\/$/, "") === url.replace(/\/$/, ""));
      return same ? activate(prev, same.key) : addTab(prev, { key: newTabKey("browser"), type: "browser", paneId: null, url, title: "" });
    }),
  }), [update, close, cwd]);

  // 终端页签标题显示正在跑的程序（同 Kimi Code：前台程序名，空闲时是 shell 名）。
  // 面板开着、有终端时每 3 秒查一次（就是关页签前那个「忙不忙」的接口，进程快照一次几毫秒）。
  const termIds = state.tabs.flatMap((tab) => (tab.type === "term" && tab.shellId !== null ? [tab.shellId] : [])).join(",");
  useEffect(() => {
    // 等本会话的进程清单到了再查，否则第一轮结果对不上号被丢掉，要干等 3 秒。
    if (remoteUi() || !state.visible || !termIds || !liveLoaded) return;
    const ids = termIds.split(",").map(Number);
    let cancelled = false;
    const poll = () => {
      shellTerminalBusy(ids)
        .then((busy) => {
          if (cancelled) return;
          const running = new Map(busy.map((item) => {
            // 进程按 pid 排，取最后一个：npm run dev 这种一路套下去，最后起的才是真正在跑的那个。
            const last = item.processes[item.processes.length - 1]?.name ?? "";
            return [item.id, last.replace(/\.exe$/i, "")] as const;
          }));
          setShells((prev) => {
            let changed = false;
            const next = new Map(prev);
            for (const id of ids) {
              const info = prev.get(id);
              if (!info) continue;
              const name = running.get(id) || undefined;
              if (info.running !== name) {
                next.set(id, { ...info, running: name });
                changed = true;
              }
            }
            return changed ? next : prev;
          });
        })
        .catch(() => {});
    };
    poll();
    // 每拍后端都要给全系统进程拍快照（shell_terminal.rs process_table），窗口最小化时没人看
    // 标题栏的运行点，跳过；重新可见那刻补一次。关窗确认自己现查，不依赖这里的结果。
    const timer = window.setInterval(() => {
      if (!document.hidden) poll();
    }, 3000);
    const onVisible = () => {
      if (!document.hidden) poll();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [state.visible, termIds, liveLoaded]);

  // agent 正在操作「这个会话面板里开着的」浏览器——只认当前页签对得上号的，
  // 别的会话残留的、已经关掉的都不算（实拍：关掉后标题栏的点一直亮着、切会话会串）。
  const agentBusy = liveLoaded && state.tabs.some((tab) => tab.type === "browser" && tab.paneId !== null
    && panes.get(tab.paneId)?.owner === sessionId && !!panes.get(tab.paneId)?.agentActive);

  // 网页在等用户答复权限请求：提示画在浏览器页签里，页签藏着、面板收起时用户看不到，网页就
  // 一直卡着（做空审查）。标题栏的面板按钮据此挂一个标记。
  const permissionWaiting = liveLoaded && state.tabs.some((tab) => tab.type === "browser" && tab.paneId !== null
    && panes.get(tab.paneId)?.owner === sessionId && !!panes.get(tab.paneId)?.permission);

  return { state, active, shells, panes, actions, agentBusy, permissionWaiting };
}

/** 页签上显示的名字。 */
export function tabTitle(tab: PanelTab, shells: ReadonlyMap<number, ShellTerminalInfo>, panes: ReadonlyMap<number, BrowserPaneInfo>, t: ReturnType<typeof useT>): string {
  switch (tab.type) {
    case "diff":
      return t.chat.diffChangesTab;
    case "files":
      return t.chat.filesTab;
    case "term":
    {
      const info = tab.shellId !== null ? shells.get(tab.shellId) : undefined;
      return info?.running || info?.title || t.chat.shellTab;
    }
    case "browser": {
      const info = tab.paneId !== null ? panes.get(tab.paneId) : undefined;
      const title = (info?.title || tab.title).trim();
      if (title) return title;
      const url = info?.url ?? tab.url;
      if (url === "about:blank") return t.chat.browserNewTab;
      try {
        return new URL(url).host || url;
      } catch {
        return url;
      }
    }
  }
}

export { EMPTY_PANEL };
