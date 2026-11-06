/**
 * 对话窗右侧面板的页签模型（纯逻辑，无 React）。
 *
 * 照 Kimi Code 1.3.1 的面板做（2026-11-06 拆包 + 实操，用户要求「整个侧栏的架构和交互参考
 * Kimi Code」）：
 * - 改动、文件、终端、浏览器都是同一排页签，都能关。改动和文件同时只开一个（再打开就是切过去），
 *   终端和浏览器可以开多个。
 * - 关掉一个页签落到它右边那个，没有右边就落到左边；关掉最后一个，面板跟着收起。
 * - 页签、当前页签、面板开没开、是否展开，按会话各存一份；面板宽度全局一份（见 useDiffPanelWidth）。
 *
 * 终端和浏览器背后是后端进程（shell_terminal.rs / browser_pane.rs）。这里只记编号：
 * 编号为空或后端已经没有这个进程（应用重启过）时，切到这个页签再按存下的目录 / 网址重新起一个。
 */

export type PanelTab =
  | { key: string; type: "diff" }
  | { key: string; type: "files" }
  | { key: string; type: "term"; shellId: number | null; cwd: string }
  | { key: string; type: "browser"; paneId: number | null; url: string; title: string };

export type PanelTabType = PanelTab["type"];

export type PanelState = {
  tabs: PanelTab[];
  active: string | null;
  visible: boolean;
  expanded: boolean;
};

export const EMPTY_PANEL: PanelState = { tabs: [], active: null, visible: false, expanded: false };

/** 同时只开一个的页签类型：再打开就是切过去。 */
export const SINGLETON_TYPES: readonly PanelTabType[] = ["diff", "files"];

let seq = 0;
export function newTabKey(type: PanelTabType): string {
  seq += 1;
  return `${type}-${Date.now().toString(36)}-${seq}`;
}

export function activeTab(state: PanelState): PanelTab | null {
  return state.tabs.find((tab) => tab.key === state.active) ?? null;
}

/** 打开改动 / 文件：已经开着就切过去，没开就加在最右边。面板随之显示。 */
export function openSingleton(state: PanelState, type: "diff" | "files"): PanelState {
  const existing = state.tabs.find((tab) => tab.type === type);
  if (existing) return { ...state, active: existing.key, visible: true };
  const tab: PanelTab = { key: newTabKey(type), type };
  return { ...state, tabs: [...state.tabs, tab], active: tab.key, visible: true };
}

/** 新开一个终端 / 浏览器页签，加在最右边并切过去。 */
export function addTab(state: PanelState, tab: PanelTab): PanelState {
  return { ...state, tabs: [...state.tabs, tab], active: tab.key, visible: true };
}

export function activate(state: PanelState, key: string): PanelState {
  if (!state.tabs.some((tab) => tab.key === key)) return state;
  return { ...state, active: key, visible: true };
}

export function updateTab(state: PanelState, key: string, change: (tab: PanelTab) => PanelTab): PanelState {
  let changed = false;
  const tabs = state.tabs.map((tab) => {
    if (tab.key !== key) return tab;
    const next = change(tab);
    if (next !== tab) changed = true;
    return next;
  });
  return changed ? { ...state, tabs } : state;
}

/** 关掉一批页签。当前页签被关时落到原位置右边那个，没有就落到左边；一个不剩就收起面板。 */
export function closeTabs(state: PanelState, keys: readonly string[]): PanelState {
  if (keys.length === 0) return state;
  const drop = new Set(keys);
  const tabs = state.tabs.filter((tab) => !drop.has(tab.key));
  if (tabs.length === state.tabs.length) return state;
  if (tabs.length === 0) return { ...state, tabs, active: null, visible: false, expanded: false };
  let active = state.active;
  if (active === null || drop.has(active)) {
    const at = state.tabs.findIndex((tab) => tab.key === state.active);
    const right = state.tabs.slice(at + 1).find((tab) => !drop.has(tab.key));
    const left = [...state.tabs.slice(0, Math.max(at, 0))].reverse().find((tab) => !drop.has(tab.key));
    active = (right ?? left ?? tabs[0]).key;
  }
  return { ...state, tabs, active };
}

/** 右键菜单的「关闭其他」「关闭右侧」「关闭所有」要关掉哪些。 */
export function keysToClose(state: PanelState, key: string, scope: "one" | "others" | "right" | "all"): string[] {
  const at = state.tabs.findIndex((tab) => tab.key === key);
  if (at < 0) return [];
  switch (scope) {
    case "one":
      return [key];
    case "others":
      return state.tabs.filter((tab) => tab.key !== key).map((tab) => tab.key);
    case "right":
      return state.tabs.slice(at + 1).map((tab) => tab.key);
    case "all":
      return state.tabs.map((tab) => tab.key);
  }
}

/** 把页签挪到 index 位置（拖拽排序、Alt+←/→）。 */
export function moveTab(state: PanelState, key: string, index: number): PanelState {
  const from = state.tabs.findIndex((tab) => tab.key === key);
  if (from < 0) return state;
  const to = Math.max(0, Math.min(index, state.tabs.length - 1));
  if (from === to) return state;
  const tabs = [...state.tabs];
  const [tab] = tabs.splice(from, 1);
  tabs.splice(to, 0, tab);
  return { ...state, tabs };
}

/** Ctrl+Tab / Ctrl+Shift+Tab：循环切换。 */
export function cycle(state: PanelState, step: 1 | -1): PanelState {
  if (state.tabs.length === 0) return state;
  const at = Math.max(0, state.tabs.findIndex((tab) => tab.key === state.active));
  const next = state.tabs[(at + step + state.tabs.length) % state.tabs.length];
  return { ...state, active: next.key, visible: true };
}

/** 开合面板：收起时页签都留着；打开时没有当前页签就落到第一个（一个都没有就显示启动列表）。 */
export function setVisible(state: PanelState, visible: boolean): PanelState {
  if (!visible) return { ...state, visible: false, expanded: false };
  return { ...state, visible: true, active: state.active ?? state.tabs[0]?.key ?? null };
}

// ── 持久化 ──────────────────────────────────────────────

const STORE_PREFIX = "meowo-panel-tabs.v1:";
/** 存多少个会话的面板。超出按最近保存时间淘汰最旧的。 */
const STORE_LIMIT = 100;

type Stored = { tabs: PanelTab[]; active: string | null; visible: boolean; expanded: boolean; savedAt: number };

function isTab(value: unknown): value is PanelTab {
  if (!value || typeof value !== "object") return false;
  const tab = value as Record<string, unknown>;
  if (typeof tab.key !== "string") return false;
  switch (tab.type) {
    case "diff":
    case "files":
      return true;
    case "term":
      return typeof tab.cwd === "string";
    case "browser":
      return typeof tab.url === "string";
    default:
      return false;
  }
}

export function loadPanel(sessionId: number, storage: Storage | null = safeStorage()): PanelState {
  if (!storage) return EMPTY_PANEL;
  try {
    const raw = JSON.parse(storage.getItem(STORE_PREFIX + sessionId) || "null") as Partial<Stored> | null;
    if (!raw || !Array.isArray(raw.tabs)) return EMPTY_PANEL;
    const tabs = raw.tabs.filter(isTab).map((tab) => {
      if (tab.type === "term") return { ...tab, shellId: typeof tab.shellId === "number" ? tab.shellId : null };
      if (tab.type === "browser") return { ...tab, paneId: typeof tab.paneId === "number" ? tab.paneId : null, title: typeof tab.title === "string" ? tab.title : "" };
      return tab;
    });
    const active = tabs.some((tab) => tab.key === raw.active) ? (raw.active as string) : tabs[0]?.key ?? null;
    return { tabs, active, visible: !!raw.visible && tabs.length > 0, expanded: !!raw.expanded && tabs.length > 0 };
  } catch {
    return EMPTY_PANEL;
  }
}

export function savePanel(sessionId: number, state: PanelState, storage: Storage | null = safeStorage()): void {
  if (!storage) return;
  const key = STORE_PREFIX + sessionId;
  try {
    if (state.tabs.length === 0) {
      storage.removeItem(key);
      return;
    }
    const stored: Stored = { ...state, savedAt: Date.now() };
    storage.setItem(key, JSON.stringify(stored));
    evict(storage);
  } catch { /* 存不下就只在内存里，下次打开从空面板开始 */ }
}

function evict(storage: Storage): void {
  const entries: { key: string; savedAt: number }[] = [];
  for (let i = 0; i < storage.length; i += 1) {
    const key = storage.key(i);
    if (!key?.startsWith(STORE_PREFIX)) continue;
    let savedAt = 0;
    try { savedAt = Number(JSON.parse(storage.getItem(key) || "{}").savedAt) || 0; } catch { /* 坏数据排最旧 */ }
    entries.push({ key, savedAt });
  }
  if (entries.length <= STORE_LIMIT) return;
  entries.sort((a, b) => a.savedAt - b.savedAt);
  for (const entry of entries.slice(0, entries.length - STORE_LIMIT)) storage.removeItem(entry.key);
}

function safeStorage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}
