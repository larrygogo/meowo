import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MutableRefObject, type UIEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { dropNeighbors, moveNextTo, orderKey, sidebarDirGroups, usePointerReorder, useSessionDrag, type DirGroup as SidebarDirGroup } from "./sidebarDrag";
import { WorkspaceDirectory } from "./chat/WorkspaceDirectory";
import { useTablist } from "./useTablist";
import { tipWithKeys } from "./chat/WindowModals";
import { confirmStopSession, getLiveSessionsPage, getSettings, moveSessionRank, setWorkspaceOrder as saveWorkspaceOrder, openNewSessionWindow, openProjectDir, recentCwds, renameSession, searchChatTranscripts, sessionTone, setArchived, type CardMenuMode, type LiveSession, type SessionTone, type TranscriptSearchHit } from "../api";
import { noteSessionMeta } from "./chat/historyCache";
import { attentionTone } from "./chat/nextAttention";
import { remoteUi } from "../remoteMode";
import { useAgents } from "../useAgents";
import { openSettingsAt } from "../settingsNav";
import { useBoardRefresh } from "../hooks/useBoardRefresh";
import { confirmMarkDone, markDoneEndsWork, pickNextAfterArchive, useSessionActions } from "../hooks/useSessionActions";
import { useSettingsEffect } from "../hooks/useSettings";
import { folderName, parentSegment, pathKey, worktreeNameOf } from "../paths";
import { useMenuPopup } from "./menu";
import { useDismissable } from "../hooks/useDismissable";
import { CardContextMenu } from "./sticker/CardContextMenu";
import { WorkspaceHeader } from "./chat/WorkspaceHeader";
import { SidebarGitBadgeSlot, useSidebarGitBadges } from "./chat/SidebarGitBadge";
import { editorKeyDown, fmtAgo, fmtShortAgo, fmtWaited, readSidebarGroupMode, readSidebarSortMode, SIDEBAR_GROUP_MODE_KEY, SIDEBAR_SORT_MODE_KEY, useStarred, type SidebarGroupMode, type SidebarSortMode } from "./sticker/helpers";
import { UNNAMED_SESSION_SENTINEL } from "./sticker/types";
import { CheckIcon, MoreIcon, TopIcon } from "./sticker/icons";
import { useT } from "../i18n";
import { formatBackendError } from "../i18n/errors";
import { isMac } from "../platform";
import { RemoteSettings } from "../mobile/RemoteSettings";
import { reconcileRows, sameRefs, type RowCache } from "../rowCache";
import type { Dict } from "../i18n/zh";
import { ArchivedWorktreePrompt, WorktreeRemoveButton, runBatchCleanup, type RemovalOutcome } from "../worktreeCleanup";
import { Icon, type Glyph } from "../icons/Icon";
import { icChevronsDownUp, icChevronsUpDown, icClock, icFilter, icFolderTree, icGrip, icMenu, icMinus, icPlusCircle, icSidebarLeft, icSliders } from "../icons/glyphs.chat";
import { icCheck, icFolder, icFolderOpen, icSearch, icSettings } from "../icons/glyphs";
import { busyProps } from "../busyButton";

/** 每翻一页新增的会话数。滚到底自动加载下一页，直到后端带回 next_cursor = null 为止。 */
const PAGE_LIMIT = 60;
const SIDEBAR_TABS = ["open", "done", "workspaces"] as const;

/** 目录下拉里最多列几个工作目录（后端按最近活跃排序，超出的靠搜索/滚动够不着的本就是冷目录）。 */
const DIR_LIMIT = 24;

const GROUP_MODE_KEY = SIDEBAR_GROUP_MODE_KEY;
const SORT_MODE_KEY = SIDEBAR_SORT_MODE_KEY;
const FOLDED_KEY = "meowo-chat-sidebar-folded-dirs";

/** 分组方式（Claude Code 侧栏 Group by 同款）：不分组 / 按目录 / 按日期 / 按状态。 */
type GroupMode = SidebarGroupMode;

/** 排序方式（对照 Kimi Code 侧栏：手动排序 / 按最近活动）：
 *  - manual：手动排序，默认。后端按手动键分页，没拖过的按创建时间倒序，新会话在最上面；
 *    桌面按住拖、手机长按后拖都能调位置（见 sidebarDrag）。
 *  - recent：最近活动，后端 connected-first + 时间倒序原样透传。
 *  - name：按名称，前端排**已加载的这一页**。 */
type SortMode = SidebarSortMode;


/** 组头汇总点的召唤强度序:待处理（Agent 正卡着等你决定）> 出错 > 在等 > 在跑（用户 2026-10-29
 *  定，对照 Kimi 的优先级，取代此前「出错排第一」）。组折起来时用户只剩这一个点可看,
 *  取组内最强的那个,否则「折叠即失明」。 */
const TONE_RANK: Record<SessionTone, number> = {
  pending: 4, error: 3, waiting: 2, running: 1, offline: 0, ended: 0,
};

type DirGroup = SidebarDirGroup<LiveSession>;

/** 侧栏底部的失败/提示条（S-4/S-5/S-6）：text 一句结果，reason 原因（另起一行），action 下一步。 */
type SidebarNotice = { text: string; reason?: string; action?: { label: string; run: () => void } };


/** 就地编辑输入框。草稿是**本组件的**局部状态：此前草稿放在 ChatSidebar 里，每次按键
 *  setState 都让整个侧栏（无虚拟化，几百条全在 DOM）重渲染一遍——重命名逐字卡顿的根因。
 *  提交/取消语义与原实现一致：Enter/失焦提交、Esc 取消（Esc 卸载组件，React 不再发 blur）。 */
function EditorInput({ initial, placeholder, onSubmit, onCancel }: {
  initial: string;
  placeholder: string;
  /** byKey = 回车提交（焦点该回到那一行）；失焦提交时用户已点去别处，不抢焦点（S-3）。 */
  onSubmit: (value: string, byKey: boolean) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <input
      className="chat-sidebar-edit"
      autoFocus
      value={value}
      placeholder={placeholder}
      onClick={(event) => event.stopPropagation()}
      onChange={(event) => setValue(event.target.value)}
      onKeyDown={editorKeyDown(() => onSubmit(value, true), onCancel)}
      onBlur={() => onSubmit(value, false)}
    />
  );
}

/** 视图/排序菜单里每个选项前的图标（对照 Kimi Code 侧栏菜单：图标 + 文案 + 右侧勾选）。 */
const SF_ICON_PATHS: Record<GroupMode | SortMode, Glyph> = {
  none: icMenu,
  dir: icFolderTree,
  recent: icClock,
  manual: icGrip,
};
function SfIcon({ name }: { name: keyof typeof SF_ICON_PATHS }) {
  return (
    <Icon glyph={SF_ICON_PATHS[name]} size={16} className="sf-ico" />
  );
}

/** 侧栏的视图/排序弹层（对照 Kimi Code 侧栏菜单，选项也照它）：「视图」平铺列表 / 按工作区分组，
 *  「排序」手动排序 / 按最近活动；每项图标 + 文案，选中项只在右侧打勾、文字不变色，选中即应用并收起。
 *  按日期 / 按状态分组、按名称排序、工作区筛选已去掉——筛选改由「工作空间」页签里的
 *  「只看这个工作区的会话」进入，生效时列表上方有一枚可关闭的标签。 */
function SidebarFilterMenu({ groupMode, groupLabels, sortMode, sortLabels, onGroup, onSort, onAdmin, t }: {
  groupMode: GroupMode;
  groupLabels: Record<GroupMode, string>;
  sortMode: SortMode;
  sortLabels: Record<SortMode, string>;
  onGroup: (mode: GroupMode) => void;
  onSort: (mode: SortMode) => void;
  /// 分隔线下的「会话管理」（对照 Kimi）：打开会话管理页。
  onAdmin?: () => void;
  t: Dict;
}) {
  const { open, setOpen, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({});
  const check = <CheckIcon className="dd-check" />;
  // 选中即应用并收起：切视图 / 排序是「一次性的快速动作」，留着面板反而多一步关闭。
  const pick = (apply: () => void) => { apply(); setOpen(false); btnRef.current?.focus(); };
  const option = <M extends string>(mode: M, selected: boolean, label: string, onPick: () => void, icon: keyof typeof SF_ICON_PATHS) => (
    <button
      key={mode}
      type="button"
      role="option"
      aria-selected={selected}
      className={"dd-item sf-opt" + (selected ? " sel" : "")}
      onClick={() => pick(onPick)}
    >
      <SfIcon name={icon} />
      <span className="dd-label">{label}</span>
      {selected && check}
    </button>
  );
  return (
    <div className="dd" ref={ref} onKeyDown={onKeyDown}>
      <button
        ref={btnRef}
        type="button"
        className="chat-sidebar-filter"
        aria-label={t.chat.sidebarFilterTip}
        data-tip={t.chat.sidebarFilterTip}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={toggle}
      >
        <Icon glyph={icFilter} size={14} />
      </button>
      {open && (
        <div className="dd-menu sf-menu" role="menu" ref={menuRef} style={{ position: "fixed", top: pos.top, bottom: pos.bottom, right: pos.right }}>
          <div className="sf-caption">{t.chat.sidebarFilterGroup}</div>
          {(["none", "dir"] as const).map((mode) => option(mode, groupMode === mode, groupLabels[mode], () => onGroup(mode), mode))}
          <div className="sf-caption">{t.chat.sidebarFilterSort}</div>
          {(["manual", "recent"] as const).map((mode) => option(mode, sortMode === mode, sortLabels[mode], () => onSort(mode), mode))}
          {onAdmin && <>
            <div className="sf-sep" role="separator" />
            <button type="button" role="menuitem" className="dd-item sf-opt" onClick={() => { setOpen(false); onAdmin(); }}>
              <Icon glyph={icSliders} size={16} className="sf-ico" />
              <span className="dd-label">{t.admin.entry}</span>
            </button>
          </>}
        </div>
      )}
    </div>
  );
}

/// 目录组的键是 pathKey(cwd)（Windows 路径带 `C:`，所以不能按「含冒号」判）；
/// 日期/状态桶的键是固定前缀。只有前者值得落盘（7C-6）。
function isDirGroupKey(key: string): boolean {
  return !key.startsWith("date:") && !key.startsWith("state:");
}

function readFolded(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(FOLDED_KEY) || "[]");
    return new Set(Array.isArray(raw) ? raw.filter((k): k is string => typeof k === "string").filter(isDirGroupKey) : []);
  } catch {
    return new Set();
  }
}

/**
 * 对话窗口左侧的会话切换列表，与右侧内容列并排、占满整窗高度。数据与看板同源
 * （get_live_sessions_page），靠 board-changed 广播刷新——它已经过后端合流去抖，
 * 这里不再自设轮询。折叠状态由 ChatWindow 持有（收起后展开入口在标题栏），
 * 本组件收起时整个卸载，数据加载随之停止。
 * memo：ChatWindow 每次击键/每秒审批倒计时都重渲染，不隔断的话上百条 item 的整棵
 * 子树跟着全量重建；父层回调已换稳定引用（resetTo/collapseSidebar 等）配合生效。
 */
export const ChatSidebar = memo(ChatSidebarImpl);

// 标题 hover 滚动的动画/复位定时器按内层 span 记账（WeakMap，条目卸载自动回收）。
// 为什么不用纯 CSS 动画 + mouseenter 挂类：悬停过程中浏览器会因 transform 移动了
// 命中目标、「⋯」按钮显隐、轮询重渲染等补发 mouseleave/mouseenter 对，每次都重启
// CSS 动画会把进度打回 0——用户实拍到的「滚不起来、卡住」就是这么来的（实测复现：
// 悬停 6s 后文字仍停在开头十几像素）。WAAPI 动画按元素持有，mouseenter 时若还在跑
// 直接返回不重启；mouseleave 也不立刻复位，留 160ms 宽限——成对抖动里的 enter 会
// 取消复位并因守卫返回，动画不间断，真正的离开才收拾干净。
const titleScrollAnims = new WeakMap<HTMLElement, Animation>();
const titleScrollTimers = new WeakMap<HTMLElement, number>();

/** 空列表的下一步。没装任何 CLI 时给「去安装」（新建了也开不起来）；新建页已经开着时只留文字——
 *  原先这里再放一个新建按钮，同屏三个新建入口（UI 调研 shell #16）。拆成子组件：只在空态挂载时
 *  才去取 agent 名单，有会话的常态零开销。手机上没有设置窗（open_settings 远程返 null），不给去安装。 */
function SidebarEmptyNext({ newSessionActive, t }: { newSessionActive: boolean; t: Dict }) {
  const { agents } = useAgents();
  // 名单没取到（旧后端/演示返回非数组）时按「装了」处理，照旧给新建入口。
  const noCli = Array.isArray(agents) && !agents.some((a) => a.installed);
  if (noCli && !remoteUi()) {
    return <button type="button" className="chat-sidebar-retry" onClick={() => openSettingsAt("account")}>{t.newSession.goInstall}</button>;
  }
  if (newSessionActive || noCli) return null;
  return (
    <button
      type="button"
      className="chat-sidebar-retry"
      onClick={() => void openNewSessionWindow().catch(() => {})}
    >{t.sticker.newSession}</button>
  );
}

function ChatSidebarImpl({ activeId, newSessionActive, approvalAwaitingIds, visibleOrderRef, attentionIdsRef, searchInputRef, width, remoteSettings = false, onSelect, onPrefetch, onCollapse, onOpenAdmin }: {
  activeId: number;
  /** 主区正开着新建会话页：「新建会话」行高亮（对照 Kimi），会话行都不高亮。 */
  newSessionActive?: boolean;
  /** 有待授权请求的非当前会话：靠这里的徽标召唤用户。后端 **会** 为 broker 接管的审批把
   *  窗口切到目标会话（见 pty.rs 的 ensure_approval_window），这里是切换竞态与
   *  「消费者已注册在别的会话」时的兜底。 */
  approvalAwaitingIds: ReadonlySet<number>;
  /** 当前显示顺序的镜像（父层归档跳转用）：归档当前会话后跳「用户看到的下一条」，
   *  两个归档入口（标题菜单/侧栏菜单）取序自此一致。卸载时清空，父层退回后端现查。 */
  visibleOrderRef?: MutableRefObject<number[]>;
  /** 显示顺序里待处理或出错的会话（含等授权的），父层 Alt+Shift+↓ 跳转用。口径同状态点。 */
  attentionIdsRef?: MutableRefObject<Set<number>>;
  /** 搜索框句柄交给父层：Ctrl/Cmd+F 监听在 ChatWindow（侧栏收起后它仍要能把人带回来）。 */
  searchInputRef?: MutableRefObject<HTMLInputElement | null>;
  /** 停靠形态的栏宽（分栏柄拖拽，ChatWindow 持久化）。抽屉形态不传：CSS 的
   *  width:100% 必须生效，内联值会盖掉它。 */
  width?: number;
  /** 宽屏远程页（iPad 横屏、桌面浏览器）在侧栏头放手机设置齿轮：齿轮原本只在手机列表页头，
   *  宽屏走停靠侧栏、不进列表页，主题和保持唤醒找不到入口（调研 m7）。 */
  remoteSettings?: boolean;
  onSelect: (id: number) => void;
  /** 预取首屏（父层 historyCache）：手指按下、鼠标悬停片刻即触发，点进去时多半已有快照。 */
  onPrefetch?: (id: number) => void;
  onCollapse: () => void;
  /** 打开会话管理页（视图与排序菜单底部的「会话管理」）。不传就不显示这一项。 */
  onOpenAdmin?: () => void;
}) {
  const t = useT();
  // 悬停预取的延时：鼠标划过列表不该每条都读一遍，停留这么久才算「可能要点」。
  const prefetchTimerRef = useRef(0);
  useEffect(() => () => window.clearTimeout(prefetchTimerRef.current), []);
  // null = 首次加载尚未完成：与「真空」区分，避免首帧误闪「暂无会话」。
  const [sessions, setSessions] = useState<LiveSession[] | null>(null);
  // 首载失败（sessions 仍是 null）不是「暂无会话」：记失败态渲染可重试的错误行，
  // 否则后端故障会被读成「会话全没了」。已有列表时的刷新失败不进此态（保留现状，
  // 下一轮 board-changed 再试）。
  const [loadFailed, setLoadFailed] = useState(false);
  // 已有列表时的刷新连续失败 ≥3 次才挂「同步中断」（与对话页 C-18 同阈值同文案）：
  // 不提示的话列表照旧显示，状态点却定格在断开前那一刻，手机上桌面一睡，绿点能一直
  // 亮着骗人。一次抖动不算，下一轮刷新成功即撤。
  const [syncLost, setSyncLost] = useState(false);
  const refreshFailStreakRef = useRef(0);
  // 接续链弹层（跨 provider 切换的回看入口）：点链徽标时惰性取链上各段。
  // 翻页用「从头取 limit 条」而不是游标续传：整段重取让 board-changed 刷新走同一条
  // 路径，不必维护「已加载的页」这份额外状态。到底与否看后端带回的 next_cursor。
  const [limit, setLimit] = useState(PAGE_LIMIT);
  const [reachedEnd, setReachedEnd] = useState(false);
  // 翻页请求在途：state 供 loading 行渲染；ref 同步镜像挡快速连滚——两个 scroll 事件
  // 可能落在同一次渲染提交前，state 版守卫看不出第一发已经把 limit 抬上去了。
  const [growing, setGrowing] = useState(false);
  const growingRef = useRef(false);
  const mountedRef = useRef(true);
  const limitRef = useRef(limit);
  limitRef.current = limit;
  // 行级结构共享的缓存（键 = session.id，只增不清，量级是本窗口见过的会话数）。
  const rowCacheRef = useRef<RowCache<LiveSession>>(new Map());
  const listRef = useRef<HTMLElement>(null);
  // 搜索框句柄：父层的 searchInputRef 是可选的，侧栏自己的焦点兜底（S-2/S-8）另持一份。
  const searchBoxRef = useRef<HTMLInputElement | null>(null);
  // 外部切换（贴纸/快捷键/审批召唤等不经由本列表点击的路径）后，当前项可能滚在视口外，
  // 高亮看不见等于没切。activeId 变化时把它滚回可视区；用户自己点击的切换本来就在
  // 视口内，block:"nearest" 下是 no-op，不打断浏览。
  useEffect(() => {
    // jsdom 无 scrollIntoView，可选调用兜底（与 composerCompletion 同法）。
    (listRef.current?.querySelector(".chat-sidebar-item.is-active") as HTMLElement | null)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [activeId]);
  // 目录筛选:选中的 cwd 走后端的**专用 cwd 参数**(斜杠归一后精确比较,子目录一并命中),
  // 不是 search 的子串 LIKE——理由见下面 load 里的注释。分页与排序仍全在后端做:前端过滤
  // 只能过滤「已加载的这一页」,筛出来的清单是残缺的。
  const [dirFilter, setDirFilter] = useState<string | null>(null);
  const dirFilterRef = useRef(dirFilter);
  dirFilterRef.current = dirFilter;
  // 已归档视图:数据走后端现成的 "archived" 过滤(与设置页 ArchivedSessions 同一通道),
  // 条目菜单里自然出现「取消归档」——归档(卡片菜单)与取回自此对称。不落盘:临时视图。
  const [archivedView, setArchivedView] = useState(false);
  // 「工作空间」页签（对照 Kimi Code 的第三个页签）：列表换成工作区目录，不落盘。
  const [workspacesView, setWorkspacesView] = useState(false);
  const sidebarTab = workspacesView ? "workspaces" : archivedView ? "done" : "open";
  const selectSidebarTab = useCallback((key: (typeof SIDEBAR_TABS)[number]) => {
    setWorkspacesView(key === "workspaces");
    if (key !== "workspaces") setArchivedView(key === "done");
  }, []);
  const sidebarTabs = useTablist({ keys: SIDEBAR_TABS, active: sidebarTab, onSelect: selectSidebarTab });
  const archivedViewRef = useRef(archivedView);
  archivedViewRef.current = archivedView;
  const [dirs, setDirs] = useState<string[]>([]);
  // 目录筛选下拉的显示名：同名目录（路径不同）带上上一级目录消歧——不消歧时用户面对
  // 一排「codebase」无从分辨（实拍反馈）。上一级仍同名的（祖父级才不同）极罕见，不再加深。
  const dirOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const cwd of dirs) {
      const name = folderName(cwd);
      counts.set(name, (counts.get(name) ?? 0) + 1);
    }
    return dirs.map((cwd) => {
      const name = folderName(cwd);
      if ((counts.get(name) ?? 0) <= 1) return { value: cwd, label: name };
      const parent = parentSegment(cwd);
      return { value: cwd, label: parent ? `${parent}/${name}` : name };
    });
  }, [dirs]);
  // 分组方式默认不分组:不用的人一个像素都感知不到。旧布尔开关(只有按目录一档)自动迁移。
  const [groupMode, setGroupMode] = useState<GroupMode>(readSidebarGroupMode);
  // 排序方式默认「创建时间」，同分组一样落盘。不默认「最近活跃」：几个会话同时在跑时，
  // 每来一条事件就换一次位置，列表顶部一直在跳，点都点不准（用户反馈）。按创建时间，
  // 新会话出现在顶上，之后位置不动。
  const [sortMode, setSortMode] = useState<SortMode>(readSidebarSortMode);
  // 「创建时间」交给后端分页（后端按 started_at 倒序扫），否则只能重排已加载的一页：
  // 新建但一直没动的会话可能根本不在第一页。load 是稳定回调，读 ref。
  const sortModeRef = useRef(sortMode);
  sortModeRef.current = sortMode;
  // 折叠的组(按 pathKey)。侧栏收起时本组件整个卸载,不落盘的话回来全展开了。
  const [folded, setFolded] = useState<Set<string>>(readFolded);

  // 会话搜索:输入防抖 300ms 后下沉后端(searchRef 是 load 的同步读源)。
  // 不落盘——搜索是临时视图,残留的词会让下次打开的列表被静默过滤(看板踩过同款坑)。
  const [search, setSearch] = useState("");
  // 工作空间页签里顶部搜索框改为过滤工作区（对照 Kimi「搜索会话或工作区」）：名称或路径包含即中。
  const workspaceOptions = useMemo(() => {
    const q = search.trim().toLowerCase();
    return q ? dirOptions.filter((option) => option.label.toLowerCase().includes(q) || option.value.toLowerCase().includes(q)) : dirOptions;
  }, [dirOptions, search]);
  const searchRef = useRef("");
  // 当前列表是按哪个词拉回来的（S-8 的 Enter 打开首条命中要它：词与结果对得上才开）。
  const loadedQueryRef = useRef("");
  // searchRef 是防抖后（300ms）才落的 load 读源;深度搜索判活性要的是**当前**词,故另用
  // 一个每渲染同步的镜像 ref(本仓 xxxRef.current = xxx 惯例),回包时与它比即可。
  const searchLiveRef = useRef(search);
  searchLiveRef.current = search;
  // 对话内容全文搜索(显式动作):LIKE 只覆盖标题/目录/最近往来,正文要靠后端扫
  // transcript 文件——点「搜索对话内容」才发,不随击键。结果与词绑定,词一变即作废。
  const [deepHits, setDeepHits] = useState<TranscriptSearchHit[] | null>(null);
  const [deepSearching, setDeepSearching] = useState(false);
  useEffect(() => { setDeepHits(null); }, [search]);
  const runDeepSearch = () => {
    const q = search.trim();
    if (!q || deepSearching) return;
    setActionError(null);
    setDeepSearching(true);
    searchChatTranscripts(q)
      // 词已变则丢弃回包(与 load() 的词校验同款):在途期间用户改了搜索词,
      // useEffect([search]) 已把 deepHits 清成 null,旧词结果落地会在新词视图下显错命中。
      .then((hits) => { if (mountedRef.current && searchLiveRef.current.trim() === q) setDeepHits(hits); })
      .catch((error) => { if (mountedRef.current) fail(t.chat.sidebarSearchFailed, error, { label: t.chat.sidebarRetry, run: () => runDeepSearchRef.current() }); })
      .finally(() => { if (mountedRef.current) setDeepSearching(false); });
  };
  const runDeepSearchRef = useRef(runDeepSearch);
  runDeepSearchRef.current = runDeepSearch;

  // refresh = 整段重取替换（首载 / board-changed）；grow = 翻页，只把新条目**追加到尾部**。
  // 追加而非替换：后端对整页做 connected-first 排序，扩大 limit 可能把更深处的活会话
  // 顶到列表最上方——用户正停在底部等下一页，上方插行会让视口内容跳走。已加载段的
  // 重排交给下一次 refresh（那时用户多半不在翻页途中）。
  const load = useCallback((mode: "refresh" | "grow") => {
    const lim = limitRef.current;
    const dir = dirFilterRef.current;
    // 目录走专用 cwd 参数(后端斜杠归一后精确比较),不复用 search 的子串 LIKE——
    // 那会让另一种斜杠写法的会话整批消失,还把兄弟目录/标题命中漏进来。
    // 搜索走 search 参数(标题/cwd/项目名 LIKE,与看板同一条后端通道):会话上百条时
    // 靠滚动翻页找会话的路是断的,此前只能退回看板去搜。
    const q = searchRef.current.trim() || null;
    const arch = archivedViewRef.current;
    const order = sortModeRef.current === "manual" ? "manual" : "activity";
    return getLiveSessionsPage(arch ? "archived" : "all", q, null, lim, dir, false, false, order)
      .then((page) => {
        if (!mountedRef.current) return;
        // 排序切换期间发出的旧请求：装的是另一种顺序的首页，丢弃。
        if ((sortModeRef.current === "manual" ? "manual" : "activity") !== order) return;
        // 切目录/改搜索词/切归档视图期间发出的旧请求回来了:它装的是旧条件的会话,
        // 原样落盘会让列表与筛选状态对不上。丢弃。
        if (dirFilterRef.current !== dir) return;
        if (archivedViewRef.current !== arch) return;
        if ((searchRef.current.trim() || null) !== q) return;
        setReachedEnd(page.next_cursor === null);
        if (mode === "refresh") {
          loadedQueryRef.current = q ?? "";
          setLoadFailed(false);
          refreshFailStreakRef.current = 0;
          setSyncLost(false);
        }
        // 行级结构共享(与看板共用 reconcileRows):board-changed 多为空转刷新,不做这层的话
        // 每 400ms 就是「整表重取 + 重建几百个卡片元素 + 全表 DOM reconcile」,而侧栏没有
        // 虚拟化、几百条全在 DOM 里。命中时整表引用不变,setState 直接短路。
        const fresh = reconcileRows(rowCacheRef.current, page.items);
        // 记下列表已知的标题/agent/目录：点进去首读未回时顶栏先用它（historyCache.peekSessionMeta）。
        for (const row of page.items) {
          noteSessionMeta(row.session.id, { title: row.task_title, provider: row.provider, cwd: row.cwd });
        }
        if (mode === "grow") {
          setSessions((prev) => {
            if (!prev) return fresh;
            const seen = new Set(prev.map((s) => s.session.id));
            return [...prev, ...fresh.filter((s) => !seen.has(s.session.id))];
          });
        } else {
          setSessions((prev) => (prev && sameRefs(prev, fresh) ? prev : fresh));
        }
      })
      .catch(() => {
        if (!mountedRef.current || dirFilterRef.current !== dir) return;
        // 翻页失败：limit 退回上一页的量。不退的话 loading 行永远挂着、重滚也发不出
        // 请求（limit 已被抬高，唯一能救场的只剩恰好路过的 board-changed）。
        if (mode === "grow") setLimit((n) => Math.max(PAGE_LIMIT, n - PAGE_LIMIT));
        // 首载失败保留 sessions === null 并进失败态（渲染「加载失败·重试」）：
        // 降级成空列表会把「后端故障」读成「会话全没了」；已有列表时的刷新失败
        // 保留现状即可，下一轮 board-changed 会再试。
        if (mode === "refresh") {
          setLoadFailed(true);
          refreshFailStreakRef.current += 1;
          if (refreshFailStreakRef.current >= 3) setSyncLost(true);
        }
      })
      .finally(() => {
        if (mode === "grow") {
          growingRef.current = false;
          if (mountedRef.current) setGrowing(false);
        }
      });
  }, []);
  const loadRef = useRef(load);
  loadRef.current = load;

  // 目录清单与会话列表同源于库。随 board-changed 一起刷（节流由 useBoardRefresh 统一做）:
  // 只在挂载取一次的话，新目录里刚建的会话在筛选下拉里找不到，直到重开侧栏。
  const refreshDirs = useCallback(() => {
    recentCwds(DIR_LIMIT)
      .then((list) => {
        if (!mountedRef.current) return;
        // 后端按原始字符串去重;同一目录可能因历史数据斜杠方向不同而重复。这里按归一
        // key 再去一次重,保留首次出现(最近活跃)的原始写法做显示;筛选走后端的 cwd
        // 参数,双方都斜杠归一后精确比较,选哪种写法都能命中两种存法的会话。
        const seen = new Set<string>();
        setDirs(list.filter((cwd) => {
          const key = pathKey(cwd);
          if (!cwd || seen.has(key)) return false;
          seen.add(key);
          return true;
        }));
      })
      .catch(() => {});
  }, []);
  const refreshDirsRef = useRef(refreshDirs);
  refreshDirsRef.current = refreshDirs;

  // board-changed 订阅 + 节流统一在 useBoardRefresh（订阅只建一次、不随 limit 重建：
  // unlisten → 新 listen 之间有异步空窗，恰落在空窗里的事件会被吞掉）。
  useBoardRefresh(() => {
    void loadRef.current("refresh");
    refreshDirsRef.current();
  });
  useEffect(() => {
    mountedRef.current = true;
    void loadRef.current("refresh");
    refreshDirsRef.current();
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // limit 变大（滚动翻页）→ 拉一次 grow。失败回退会把 limit 改小，不能据此再发请求，
  // 否则「失败 → 回退 → 又触发加载」原地打转。
  const prevLimitRef = useRef(PAGE_LIMIT);
  useEffect(() => {
    const grew = limit > prevLimitRef.current;
    prevLimitRef.current = limit;
    if (grew) void loadRef.current("grow");
  }, [limit]);

  // 切目录 = 换一份数据源:整段重取,并把翻页状态清回首页(不清的话新目录一上来就
  // 顶着上一个目录翻到的 limit,一次拉几百条)。首挂载不重复触发(那次由挂载 effect 发)。
  const prevDirRef = useRef<string | null>(null);
  const dirTouchedRef = useRef(false);
  useEffect(() => {
    if (!dirTouchedRef.current) {
      dirTouchedRef.current = true;
      prevDirRef.current = dirFilter;
      return;
    }
    if (prevDirRef.current === dirFilter) return;
    prevDirRef.current = dirFilter;
    // limitRef 必须**当场**改回去:load 立刻就读它,而 setLimit 要等下一次渲染才落到
    // limitRef 上——只 setLimit 的话,切目录后的首个请求仍带着上一个目录翻到的 limit。
    prevLimitRef.current = PAGE_LIMIT;
    limitRef.current = PAGE_LIMIT;
    setLimit(PAGE_LIMIT);
    setReachedEnd(false);
    setSessions(null);
    // 换数据源即回到首载语义：失败态一并清零，否则新条件的首载会先闪一行旧错误。
    setLoadFailed(false);
    // 在途的 grow 归属上一个目录:它的响应已被 search 守卫丢弃,这里把闸门也放开,
    // 否则新目录要等那个作废的请求 settle 才能翻页。
    growingRef.current = false;
    setGrowing(false);
    void loadRef.current("refresh");
  }, [dirFilter]);

  // 在「手动排序」与其它排序之间切换 = 后端换了扫描顺序,首页内容可能完全不同:回到
  // 首页重取(与切目录同一套重置语义)。最近活动 ↔ 按名称只是前端重排,不必重取。
  const prevOrderRef = useRef(sortMode === "manual");
  useEffect(() => {
    const byCreated = sortMode === "manual";
    if (prevOrderRef.current === byCreated) return;
    prevOrderRef.current = byCreated;
    prevLimitRef.current = PAGE_LIMIT;
    limitRef.current = PAGE_LIMIT;
    setLimit(PAGE_LIMIT);
    setReachedEnd(false);
    setLoadFailed(false);
    growingRef.current = false;
    setGrowing(false);
    void loadRef.current("refresh");
  }, [sortMode]);

  // 切换已归档视图 = 换数据源:与切目录同一套重置语义。
  const prevArchRef = useRef(false);
  useEffect(() => {
    if (prevArchRef.current === archivedView) return;
    prevArchRef.current = archivedView;
    prevLimitRef.current = PAGE_LIMIT;
    limitRef.current = PAGE_LIMIT;
    setLimit(PAGE_LIMIT);
    setReachedEnd(false);
    setSessions(null);
    // 换数据源即回到首载语义：失败态一并清零，否则新条件的首载会先闪一行旧错误。
    setLoadFailed(false);
    growingRef.current = false;
    setGrowing(false);
    void loadRef.current("refresh");
  }, [archivedView]);

  // 搜索词变化:防抖 300ms 重取首页(翻页状态清回首页,与切目录同一套语义)。
  // 不清 sessions——保留旧列表到新结果落地,避免每敲一个字闪一次加载占位。
  const searchTouchedRef = useRef(false);
  useEffect(() => {
    if (!searchTouchedRef.current) {
      searchTouchedRef.current = true;
      return;
    }
    const timer = window.setTimeout(() => {
      searchRef.current = search;
      prevLimitRef.current = PAGE_LIMIT;
      limitRef.current = PAGE_LIMIT;
      setLimit(PAGE_LIMIT);
      setReachedEnd(false);
      growingRef.current = false;
      setGrowing(false);
      void loadRef.current("refresh");
    }, 300);
    return () => window.clearTimeout(timer);
  }, [search]);

  // Ctrl/Cmd+F 聚焦搜索框的监听在 ChatWindow（侧栏收起时本组件整体卸载，挂在这里的
  // 快捷键会跟着静默失效）；本组件只把搜索框句柄经 searchInputRef 交出去。

  // 会话操作菜单:与看板卡片是同一个 CardContextMenu、同一批动作(置顶/重命名/归档/
  // 新建会话/打开目录/结束会话)。两处共用一个组件是刻意的——菜单项一旦各写一份,迟早
  // 会一边有、另一边没有。触发方式:条目右键,或条目上的「⋯」按钮。
  const [ctxMenu, setCtxMenu] = useState<{ sid: number; x: number; y: number } | null>(null);
  // 触发方式跟随「卡片菜单」设置(card_menu_mode),与看板同一个开关、同一套语义:
  // button = 条目上的「⋯」按钮,context = 右键。**二选一**,不同时存在——两个入口并存
  // 时,改设置的人会以为没生效(另一个还在),不改的人则平白多一个不知从哪来的按钮。
  // 首帧占位与后端真实默认(button)一致,与看板同步改过——占位 context 会让 ⋯ 按钮延迟弹入。
  const [menuMode, setMenuMode] = useState<CardMenuMode>("button");
  // 远程钉死 button 档:桌面设置若是 context(仅右键),触屏根本够不着菜单。
  // 工作区组的手动顺序（设置里的 workspace_order）：桌面 / 手机拖分组头写入，两端读同一份。
  const [workspaceOrder, setWorkspaceOrder] = useState<readonly string[]>([]);
  useSettingsEffect((s) => {
    setMenuMode(remoteUi() ? "button" : (s.card_menu_mode ?? "button"));
    setWorkspaceOrder(s.workspace_order ?? []);
  });
  // 菜单内容按 id 现查:刷新后置顶/归档态自动跟上,会话消失则菜单自然收起。
  const ctxItem = ctxMenu ? (sessions ?? []).find((s) => s.session.id === ctxMenu.sid) ?? null : null;
  // 置顶与看板共用同一份 localStorage(键沿用 meowo-starred,改键会丢用户已有数据),
  // 两个界面看到的是同一批置顶会话,且都排到最前(见下方 ordered)。
  // 置顶集：与看板/对话窗标题菜单共用 useStarred（同一份写入口 + 双通道同步）。
  const { starred, toggleStar } = useStarred();
  // 重命名:与看板一样就地编辑。
  // 草稿文本不在这里——住在 EditorInput 的局部状态里,按键不触发侧栏整表重渲染。
  const [editingId, setEditingId] = useState<number | null>(null);
  // 重命名失败时带回编辑框的草稿（S-4）：编辑框先关、再异步失败，不留住的话用户输的新名字就没了。
  const [editDraft, setEditDraft] = useState<string | null>(null);
  // 动作失败必须可见:静默吞掉的话,用户点了「重命名」只会看到名字没变,以为是自己没点上。
  // S-4/S-5（走查 2026-11-12）：原先 4s 自动消失、多数只有「操作失败，请稍后再试」。现在
  // text 说哪个动作没成，reason 是 formatBackendError 映射的原因，能重来的给 action 按钮；
  // 不再计时消失，留到用户点 × 或做下一个动作（含原因的提示 4 秒读不完，worktree 删除失败
  // 那种「某程序占着目录」更是要读完才知道下一步）。
  const [actionError, setActionError] = useState<SidebarNotice | null>(null);
  const fail = (text: string, error?: unknown, action?: SidebarNotice["action"]) => {
    const reason = error === undefined ? "" : formatBackendError(error, t.locale).trim();
    setActionError({ text, reason: reason || undefined, action });
  };
  // 批量清理 worktree 的进行状态与结果（结果可能多行：失败项逐行列出）。S-5（2026-11-12）：原先 8s
  // 自动收起，逐行的失败原因读不完；改为留到点「好」（与失败提示同一条规则）。
  const [cleaning, setCleaning] = useState(false);
  const [cleanResult, setCleanResult] = useState<string | null>(null);

  // 键盘动作之后把焦点放回列表里的某一行（S-2/S-3）：被操作的行卸载或重排后焦点会掉到 body，
  // 键盘用户只能从窗口顶上重新 Tab。下一拍按 data-session-id 找（与 keyboardMove 同法）；
  // 行已不在就退到搜索框，总之不留在 body。
  const focusRow = (id: number | null) => {
    window.setTimeout(() => {
      if (!mountedRef.current) return;
      const row = id == null ? null : listRef.current?.querySelector<HTMLElement>(`.chat-sidebar-item[data-session-id="${id}"]`);
      if (row) row.focus();
      else if (document.activeElement === document.body || !document.activeElement) searchBoxRef.current?.focus();
    }, 0);
  };

  const startRename = (item: LiveSession) => {
    setActionError(null);
    setEditDraft(null);
    setEditingId(item.session.id);
  };
  const submitRename = (item: LiveSession, draft: string, byKey = false) => {
    const title = draft.trim();
    const id = item.session.id;
    if (title && title !== item.task_title) {
      renameSession(item.cwd, item.session.cc_session_id, title, item.provider)
        .catch((error) => {
          if (!mountedRef.current) return;
          // S-4：带着草稿重新打开编辑框（autoFocus 接住焦点），用户改一个字再回车即重试。
          setEditDraft(draft);
          setEditingId(id);
          fail(t.chat.sidebarRenameFailed, error);
        });
    }
    setEditingId(null);
    setEditDraft(null);
    if (byKey) focusRow(id);
  };
  const cancelRename = (id: number) => {
    setEditingId(null);
    setEditDraft(null);
    focusRow(id);
  };
  // 归档动作(单条/批量)与撤销条统一走 useSessionActions(C-13):与对话窗标题菜单同一条
  // set_archived、同一个「下一条」取序(按当前可见顺序取当前条之后的第一条)、同一个 8s 撤销窗。
  const { archive: archiveSessions } = useSessionActions({
    onNavigate: onSelect,
    onError: (reason) => setActionError({ text: t.chat.sidebarArchiveFailed, reason: reason || undefined }),
  });
  // 「标记完成」会不会结束在干活的 Agent（S-1）：口径同状态点（sessionTone），外加待授权徽标那批。
  const isWorking = (item: LiveSession) =>
    approvalAwaitingIds.has(item.session.id)
    || markDoneEndsWork(sessionTone(item.connected, item.session.status, item.pending_review, item.errored, item.busy_subagents, item.screen_state));
  // 用户眼前的行序（折叠组里没画出来的不算）：连选、标记完成后的焦点去向都按它。
  const visibleIds = () => (renderedGroups ? renderedGroups.flatMap((entry) => entry.shown) : (ordered ?? []))
    .map((item) => item.session.id);
  // 批量标记完成:Ctrl/Cmd+点击(或右键菜单)进入多选,多选态下点整行即增删,
  // 底部「取消」或 Esc 清空退出。选中集按 id,刷新/翻页不受影响。
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<number>>(() => new Set());
  useEffect(() => {
    if (selectedIds.size === 0) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      // 截停:多选态的 Esc 只清选择,不落到窗口级「拒绝审批」。
      event.preventDefault();
      setSelectedIds(new Set());
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedIds]);
  // Shift+点击连选的锚点:最近一次单点增删的那条(没有就从当前打开的会话起,同文件管理器)。
  const selectAnchorRef = useRef<number | null>(null);
  const toggleSelected = (id: number) => {
    selectAnchorRef.current = id;
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  };
  // 整组增删(组头勾选圈):清理旧会话是「按日期分组 → 勾上『更早』整组 → 归档」一次做完
  // (对照 Kimi 会话管理页的按更新时间筛选再批量),不必逐条点几十下。组里已全选则整组取消。
  const toggleGroupSelected = (items: LiveSession[]) =>
    setSelectedIds((prev) => {
      const next = new Set(prev);
      const all = items.every((item) => prev.has(item.session.id));
      for (const item of items) {
        if (all) next.delete(item.session.id);
        else next.add(item.session.id);
      }
      return next;
    });
  const archiveSelected = async () => {
    const ids = [...selectedIds];
    if (ids.length === 0) return;
    const targets = (sessions ?? []).filter((item) => selectedIds.has(item.session.id));
    if (!(await confirmMarkDone(targets.map((item) => ({ working: isWorking(item) })), t))) return;
    if (!mountedRef.current) return;
    setActionError(null);
    // S-2：操作条随选中集清空而卸载，焦点会掉到 body；落到被摘掉那批之后的第一条（没有就往前找）。
    const shown = visibleIds();
    const lastPicked = [...shown].reverse().find((id) => selectedIds.has(id)) ?? ids[ids.length - 1];
    const focusNext = ids.includes(activeId) ? null : pickNextAfterArchive(shown, ids, lastPicked);
    setSelectedIds(new Set());
    // 乐观整批摘除;当前打开的也在其中时跳「下一条」(取序与对话窗归档一致,见 useSessionActions)。
    setSessions((prev) => prev?.filter((s) => !ids.includes(s.session.id)) ?? prev);
    void archiveSessions({
      ids,
      visibleOrder: (ordered ?? []).map((s) => s.session.id),
      activeId,
      onFailure: () => void loadRef.current("refresh"),
      errorMessage: (reason) => formatBackendError(reason, t.locale),
    });
    // 归档的含当前打开的会话时，跳到下一条并由对话窗接住焦点（输入框），这里不抢。
    if (!ids.includes(activeId)) focusRow(focusNext);
  };
  const toggleArchived = async (item: LiveSession) => {
    const target = !item.archived;
    const id = item.session.id;
    if (target && !(await confirmMarkDone([{ working: isWorking(item) }], t))) return;
    if (!mountedRef.current) return;
    setActionError(null);
    // S-2：被摘掉的那一行原本握着焦点（菜单关闭时还给它），落到下一条。
    const focusNext = id === activeId ? null : pickNextAfterArchive(visibleIds(), [id], id);
    // 归档 = 从 "all" 里消失,乐观摘掉(不等 IPC 往返),失败重取拉回。当前打开的那条也照摘:
    // 已归档 = 已收纳,列表里就不该再有它,哪怕右边还开着它的对话。
    setSessions((prev) => prev?.filter((s) => s.session.id !== id) ?? prev);
    if (target) {
      // 归档的正是当前打开的这条:右边也跟着跳到「下一条」(取序同对话窗,见 useSessionActions)。
      void archiveSessions({
        ids: [id],
        visibleOrder: (ordered ?? []).map((s) => s.session.id),
        activeId,
        onFailure: () => void loadRef.current("refresh"),
        errorMessage: (reason) => formatBackendError(reason, t.locale),
      });
      if (id !== activeId) focusRow(focusNext);
      return;
    }
    // 「已完成」页里恢复进行中，同样从这一页消失。
    if (id !== activeId) focusRow(pickNextAfterArchive(visibleIds(), [id], id));
    setArchived(id, false).catch((error) => {
      if (!mountedRef.current) return;
      fail(t.chat.sidebarReopenFailed, error);
      void loadRef.current("refresh");
    });
  };
  const endSession = (item: LiveSession) => {
    setActionError(null);
    void confirmStopSession(item.session.id, { title: t.chat.endSession, message: t.chat.endSessionConfirm })
      .catch((error) => { if (mountedRef.current) fail(t.chat.sidebarEndFailed, error, { label: t.chat.sidebarRetry, run: () => endSession(item) }); });
  };
  const openMenuAt = (item: LiveSession, x: number, y: number) => setCtxMenu({ sid: item.session.id, x, y });

  const changeGroupMode = (mode: GroupMode) => {
    setGroupMode(mode);
    localStorage.setItem(GROUP_MODE_KEY, mode);
  };
  const changeSortMode = (mode: SortMode) => {
    setSortMode(mode);
    localStorage.setItem(SORT_MODE_KEY, mode);
  };
  // 只有**目录**分组的折叠值得记住：目录键是稳定的路径。日期桶（`date:today`）与状态桶
  // 的键固定、内容天天换，落盘等于「昨天折起来的今天照样折着」，新会话凭空消失（7C-6）。
  const toggleFolded = (key: string) => setFolded((prev) => {
    const next = new Set(prev);
    if (!next.delete(key)) next.add(key);
    localStorage.setItem(FOLDED_KEY, JSON.stringify([...next].filter(isDirGroupKey)));
    return next;
  });

  // 分组只在开关打开时算。**它只覆盖已加载的这一页**:滚动翻页会让组长大、后来的
  // 会话可能落进更靠上的组。选了目录时不存在这个问题(筛选在后端做,清单是完整的)。
  // 各分组里「未运行会话」的展开状态。刻意不持久化：它是一次浏览中的临时动作，
  // 下次打开侧栏该回到「只看在跑的」这个默认。
  const [revealed, setRevealed] = useState<ReadonlySet<string>>(() => new Set<string>());
  const toggleRevealed = (key: string) =>
    setRevealed((prev) => {
      const next = new Set(prev);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  // 置顶浮顶:与看板同一条规则(置顶的排最前),否则「置顶」名不副实——标记亮着、位置没动。
  // Array.sort 稳定,「最近活跃」档非置顶的相对次序原样保留(后端 connected-first +
  // 时间倒序);「创建时间/名称」档在置顶之后按所选键重排(用户明确选了排序,不再保
  // connected-first——正在跑的会话有状态点可辨识)。
  // 分组开着时排序发生在**分组之前**:组内置顶在前,而组序由「组内第一条的位置」派生,
  // 于是含置顶会话的目录也跟着上浮——两套顺序不打架。
  const ordered = useMemo(() => {
    if (!sessions || starred.size === 0) return sessions;
    const rank = (item: LiveSession) => (starred.has(item.session.cc_session_id) ? 0 : 1);
    return [...sessions].sort((a, b) => {
      const star = rank(a) - rank(b);
      if (star !== 0) return star;
      return 0;
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps -- sortMode 是重排信号，切排序方式时按置顶再排一次，保留原行为
  }, [sessions, starred, sortMode]);
  // 会话行的 PR / CI 与改动数（UI 优化 #54）：按已加载会话的工作目录批量取，同目录只算一次。
  const loadedCwds = useMemo(() => (ordered ?? []).map((item) => item.cwd), [ordered]);
  const gitBadges = useSidebarGitBadges(loadedCwds);

  const groups = useMemo(() => {
    if (!ordered || groupMode !== "dir") return null;
    return sidebarDirGroups(ordered, sortMode, workspaceOrder);
  }, [groupMode, ordered, sortMode, workspaceOrder]);
  // 拖分组头调整工作区顺序（手动排序、按工作区分组、非多选态；手机长按后拖）。落下即本地先排，
  // 再把整份顺序写进设置；写失败报错并回到设置里原来的顺序。
  const groupKeys = useMemo(() => (groups ?? []).map((group) => group.key), [groups]);
  const groupDrag = usePointerReorder<string>({
    enabled: sortMode === "manual" && groupMode === "dir" && selectedIds.size === 0,
    attr: "groupKey",
    parse: (raw) => raw || null,
    bucketOf: (key) => (groupKeys.includes(key) ? "groups" : null),
    listOf: () => groupKeys,
    onMove: (key, above, below) => {
      const next = moveNextTo(groupKeys, (entry) => entry, key, above, below).map(orderKey);
      setWorkspaceOrder(next);
      // 只写这一个字段（不走整份 setSettings）：读改写两步之间别处改的设置会被旧快照盖掉，
      // 且手机端调不了 setSettings。
      saveWorkspaceOrder(next)
        .catch((error) => {
          if (!mountedRef.current) return;
          fail(t.chat.sidebarReorderFailed, error);
          void getSettings().then((settings) => setWorkspaceOrder(settings.workspace_order ?? [])).catch(() => {});
        });
    },
  });
  // 「会话」小节的全部折叠 / 展开（对照 Kimi）：全都折着时展开全部，否则折起全部。
  const allGroupsFolded = !!groups && groups.length > 0 && groups.every((group) => folded.has(group.key));
  const toggleAllFolded = () => setFolded(() => {
    const next = new Set<string>(allGroupsFolded ? [] : (groups ?? []).map((group) => group.key));
    localStorage.setItem(FOLDED_KEY, JSON.stringify([...next].filter(isDirGroupKey)));
    return next;
  });
  // 拖拽的桶：同一分组 + 同为置顶 / 非置顶（理由见 sidebarDrag）。桶内顺序取组的全部条目
  // （含收起的未运行会话），落点邻居才和后端的全局顺序对得上。
  const dragBuckets = useMemo(() => {
    const bucketOf = new Map<number, string>();
    const lists = new Map<string, number[]>();
    const lanes = groups ? groups.map((group) => ({ key: group.key, items: group.items })) : [{ key: "flat", items: ordered ?? [] }];
    for (const lane of lanes) {
      for (const item of lane.items) {
        const bucket = `${lane.key}\u0000${starred.has(item.session.cc_session_id) ? 1 : 0}`;
        bucketOf.set(item.session.id, bucket);
        const list = lists.get(bucket) ?? [];
        list.push(item.session.id);
        lists.set(bucket, list);
      }
    }
    return { bucketOf, lists };
  }, [groups, ordered, starred]);
  // 拖拽（手动排序、非多选态；手机长按后拖）：落下即本地先挪，再写后端；写失败重取回真实顺序。
  const moveSession = (id: number, above: number | null, below: number | null) => {
    setSessions((prev) => (prev ? moveNextTo(prev, (item) => item.session.id, id, above, below) : prev));
    setActionError(null);
    moveSessionRank(id, above, below).catch((error) => {
      if (!mountedRef.current) return;
      fail(t.chat.sidebarReorderFailed, error);
      void loadRef.current("refresh");
    });
  };
  const reorderEnabled = sortMode === "manual" && selectedIds.size === 0;
  // 键盘挪位的播报（S-7）。借全侧栏唯一的 live region（G-16）出声，几秒后让回状态汇总。
  const [moveAnnounce, setMoveAnnounce] = useState("");
  useEffect(() => {
    if (!moveAnnounce) return;
    const timer = window.setTimeout(() => setMoveAnnounce(""), 3_000);
    return () => window.clearTimeout(timer);
  }, [moveAnnounce]);
  // 键盘排序（调研 s8：原先只能鼠标拖）：焦点在条目上按 Ctrl/⌘+Shift+↑/↓ 在同一个桶里挪一位，
  // 与拖拽同一条写回路径。不用 Alt：Alt+↑/↓ 切会话、Alt+Shift+↓ 跳到下一个待处理会话。
  // 挪完 React 会搬动条目 DOM，焦点随之掉回 body，下一拍按 id 找回来。
  const keyboardMove = (id: number, dir: -1 | 1): boolean => {
    if (!reorderEnabled) return false;
    const bucket = dragBuckets.bucketOf.get(id);
    const list = bucket == null ? [] : dragBuckets.lists.get(bucket) ?? [];
    const neighbor = list[list.indexOf(id) + dir];
    if (neighbor === undefined) return true;
    const neighbors = dropNeighbors(list, id, { id: neighbor, after: dir > 0 });
    if (!neighbors) return true;
    moveSession(id, neighbors.above, neighbors.below);
    // S-7：读屏只会把同一行再念一遍，听不出挪没挪成、挪到了哪。播报桶内新位置（第 N / 共 M）。
    setMoveAnnounce(t.chat.sidebarMoved(list.indexOf(id) + dir + 1, list.length));
    window.setTimeout(() => {
      listRef.current?.querySelector<HTMLElement>(`.chat-sidebar-item[data-session-id="${id}"]`)?.focus();
    }, 0);
    return true;
  };
  const drag = useSessionDrag({
    enabled: reorderEnabled,
    bucketOf: (id) => dragBuckets.bucketOf.get(id) ?? null,
    listOf: (bucket) => dragBuckets.lists.get(bucket) ?? [],
    onMove: moveSession,
    // 手机长按没挪就松手 = 开菜单（iOS 长按不派发 contextmenu，Android 的那次被拖拽吞掉了）。
    onHold: (id, x, y) => {
      const item = sessions?.find((entry) => entry.session.id === id);
      if (item) openMenuAt(item, x, y);
    },
  });

  // 显示顺序镜像:分组模式的组序由 ordered 派生、组内亦是 ordered 序,写 ordered 即可。
  // 卸载清空——收起的侧栏没有「用户看到的顺序」,父层归档跳转退回后端现查。
  useEffect(() => {
    if (!visibleOrderRef) return;
    visibleOrderRef.current = (ordered ?? []).map((item) => item.session.id);
  }, [ordered, visibleOrderRef]);
  useEffect(() => () => { if (visibleOrderRef) visibleOrderRef.current = []; }, [visibleOrderRef]);
  useEffect(() => {
    if (!attentionIdsRef) return;
    attentionIdsRef.current = new Set((ordered ?? [])
      .filter((item) => approvalAwaitingIds.has(item.session.id)
        || attentionTone(sessionTone(item.connected, item.session.status, item.pending_review, item.errored, item.busy_subagents, item.screen_state, item.screen_assumed)))
      .map((item) => item.session.id));
  }, [ordered, approvalAwaitingIds, attentionIdsRef]);
  useEffect(() => () => { if (attentionIdsRef) attentionIdsRef.current = new Set(); }, [attentionIdsRef]);

  // 滚到底前 120px 就预取下一页。
  const onScroll = (event: UIEvent<HTMLElement>) => {
    if (reachedEnd || sessions === null || growingRef.current) return;
    const el = event.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight > 120) return;
    growingRef.current = true;
    setGrowing(true);
    setLimit((n) => n + PAGE_LIMIT);
  };

  // 分组的**渲染形态**（哪些组折着、每组实际画出哪几条）。此前这段算在 JSX 里，
  // roving 锚点只能拿 ordered 猜（7C-6：锚点落在折叠组里没渲染的条目上时，整张列表
  // 一个 tabIndex=0 都没有，Tab 根本进不来）。抽成 memo，锚点与渲染读同一份。
  const renderedGroups = useMemo(() => {
    if (!groups) return null;
    return groups.map((group) => {
      // 「未运行」= 已断开，且不是当前打开的这条，也不是置顶的那些——正开着的会话
      // 无论死活都得留在视野里（否则用户一进来就找不到自己在哪），置顶的更是：
      // 用户刚亲手把它顶上来，转头被折进「显示 N 个未运行会话」里等于白顶。
      const live = (item: LiveSession) =>
        item.connected || item.session.id === activeId || starred.has(item.session.cc_session_id);
      const idle = group.items.filter((item) => !live(item));
      // 折叠只在组里真有在跑的会话时才启用：它的使命是保住在跑的那几条不被
      // 历史挤出视野。仅因当前打开/置顶而有「活」条目的组（典型是「已结束」组，
      // 用户刚把当前会话切进去）不收——否则选中一条,其余几十条凭空消失。
      // 有搜索词时一律不折：用户明确在找某条会话,命中了却被藏进「显示 N 个
      // 未运行」等于告诉他「搜不到」。
      const canCollapse = !search.trim() && group.items.some((item) => item.connected) && idle.length > 0;
      // 当前会话所在的组不再强制展开（用户反馈折不起来）。7C-6 要防的「打开的会话落进
      // 折起来的组、自己看不见」改由下面的 effect 在切换会话时展开一次。
      const isFolded = folded.has(group.key);
      const shown = canCollapse && !revealed.has(group.key) ? group.items.filter(live) : group.items;
      return { group, isFolded, canCollapse, idleCount: idle.length, shown: isFolded ? [] : shown };
    });
  }, [groups, folded, revealed, search, activeId, starred]);

  // 切到一个会话时，它所在的组若折着就展开一次（7C-6：日期分桶的键固定，折过一次后
  // 第二天的新会话全落进同一个折着的桶，打开的那条自己看不见）。每个会话只展开一次：
  // 之后用户主动折起就照办，不再跟他抢。分组随列表异步加载，找到所在组才算办完。
  const unfoldedForRef = useRef<number | null>(null);
  useEffect(() => {
    if (!groups || unfoldedForRef.current === activeId) return;
    const home = groups.find((group) => group.items.some((item) => item.session.id === activeId));
    if (!home) return;
    unfoldedForRef.current = activeId;
    if (folded.has(home.key)) toggleFolded(home.key);
    // folded 不进依赖：只在会话切换/分组就位时判一次，用户之后折起不该触发重新展开。
    // eslint-disable-next-line react-hooks/exhaustive-deps -- folded 不进依赖，用户之后折起不该触发重新展开
  }, [groups, activeId]);

  // C-17：侧栏列表 roving tabindex——上百条会话只占一个 Tab 停靠点，↑↓/Home/End 在
  // 条目间搬 DOM 焦点。可停靠条目取当前选中的那条（没被渲染出来时退回首条渲染项）。
  const tabbableId = useMemo(() => {
    const visible = renderedGroups
      ? renderedGroups.flatMap((entry) => entry.shown)
      : (ordered ?? []);
    if (visible.length === 0) return null;
    return visible.some((item) => item.session.id === activeId) ? activeId : visible[0].session.id;
  }, [renderedGroups, ordered, activeId]);

  const selectRange = (id: number) => {
    const visible = (renderedGroups ? renderedGroups.flatMap((entry) => entry.shown) : (ordered ?? []))
      .map((item) => item.session.id);
    const to = visible.indexOf(id);
    const from = visible.indexOf(selectAnchorRef.current ?? activeId ?? id);
    if (to < 0 || from < 0) {
      toggleSelected(id);
      return;
    }
    const range = visible.slice(Math.min(from, to), Math.max(from, to) + 1);
    setSelectedIds((prev) => new Set([...prev, ...range]));
  };
  // 组头勾选圈:只在多选态出现,全选实心勾、部分选中横杠(aria-checked=mixed)。
  const renderGroupCheck = (items: LiveSession[], label: string) => {
    if (selectedIds.size === 0 || items.length === 0) return null;
    const count = items.filter((item) => selectedIds.has(item.session.id)).length;
    const state = count === items.length ? "true" : count > 0 ? "mixed" : "false";
    return (
      <button
        type="button"
        className={"chat-sidebar-check-btn" + (state === "false" ? "" : " is-checked")}
        role="checkbox"
        aria-checked={state}
        aria-label={t.chat.selectGroup(label)}
        onClick={(event) => {
          event.stopPropagation();
          toggleGroupSelected(items);
        }}
      >
        <span className="chat-sidebar-check" aria-hidden="true">
          {state === "true" && (
            <Icon glyph={icCheck} size={12} />
          )}
          {state === "mixed" && (
            <Icon glyph={icMinus} size={12} />
          )}
        </span>
      </button>
    );
  };

  // G-16：状态点已全部降级为 role=img（每条会话一个 role=status 就是 N 个播报源，
  // 状态秒级跳变等于播报风暴）；全侧栏只留这一个视觉隐藏的汇总 live region。
  // 文本没变时 React 不动 DOM，读屏不会重复播报。
  const liveSummary = useMemo(() => {
    if (!ordered || ordered.length === 0) return "";
    let approval = 0, waiting = 0, error = 0;
    for (const item of ordered) {
      if (approvalAwaitingIds.has(item.session.id)) approval += 1;
      const tone = sessionTone(item.connected, item.session.status, item.pending_review, item.errored, item.busy_subagents, item.screen_state, item.screen_assumed);
      if (tone === "waiting") waiting += 1;
      else if (tone === "error") error += 1;
    }
    return t.chat.sidebarLiveSummary(approval, waiting, error);
  }, [ordered, approvalAwaitingIds, t]);

  // 平铺与分组共用同一份条目渲染:两套 JSX 会各自漂移(状态点口径、标题回退文案),
  // 那正是「贴纸报错、侧栏亮绿点」那类不一致的来源。
  const renderItem = (item: LiveSession) => {
    const dir = folderName(item.cwd);
    // 与对话窗标题栏同一套口径(sessionTone,含 errored——贴纸的错误优先级由此
    // 对齐,不再出现「贴纸报错、侧栏亮绿点」)。offline/ended 不加点——图标置灰
    // (is-off)已表达「不活跃」,再叠一个灰点是噪声。
    const tone = sessionTone(item.connected, item.session.status, item.pending_review, item.errored, item.busy_subagents, item.screen_state, item.screen_assumed);
    const showDot = tone === "running" || tone === "pending" || tone === "waiting" || tone === "error";
    // 待授权徽标优先于状态点：授权在等用户决策，比「在跑/待处理」都紧急；
    // 且不依附 connected——离线会话恢复的 agent 同样可能来要权限。
    const awaitingApproval = approvalAwaitingIds.has(item.session.id);
    const editing = editingId === item.session.id;
    // 目录已删的会话不挂（点进去也恢复不了，下面另有「目录已删除」）。
    const badgeCwd = item.cwd && !item.cwd_missing ? item.cwd : null;
    // 条目从 <button> 改成 role=button 的 <div>：里面要放「⋯」按钮，button 套 button 是
    // 非法 HTML（浏览器会把内层拆出去）。键盘可达性靠 tabIndex + Enter/Space 补齐；
    // tabIndex 是 roving 的——只有 tabbableId 那条进 Tab 序，其余靠列表的 ↑↓ 到达（C-17）。
    return (
      <div
        key={item.session.id}
        role="button"
        tabIndex={item.session.id === tabbableId ? 0 : -1}
        data-session-id={item.session.id}
        className={"chat-sidebar-item" + (item.session.id === activeId ? " is-active" : "") + (selectedIds.has(item.session.id) ? " is-checked" : "")
          + (drag.dragId === item.session.id ? " is-dragging" : "")
          + (drag.dropAt?.id === item.session.id && drag.dragId !== item.session.id ? (drag.dropAt.after ? " is-drop-after" : " is-drop-before") : "")}
        aria-current={item.session.id === activeId ? "true" : undefined}
        // 多选态是切换语义，用 aria-pressed：aria-selected 对 role=button 无效（读屏收不到）。
        aria-pressed={selectedIds.size > 0 ? selectedIds.has(item.session.id) : undefined}
        // 预取：触屏按下即读（到点击生效还有一百多毫秒），鼠标悬停 150ms 再读。多选态点行是勾选，不预取。
        onPointerDown={(event) => {
          if (selectedIds.size === 0 && !editing) onPrefetch?.(item.session.id);
          // 手动排序下按住拖动即调位置（行内按钮、编辑框不起拖）。
          if (!editing && !(event.target as HTMLElement).closest("button, input, textarea, .chat-sidebar-item-menu, .chat-sidebar-check-btn")) drag.onPointerDown(event, item.session.id);
        }}
        onPointerEnter={(event) => {
          if (event.pointerType !== "mouse" || !onPrefetch) return;
          window.clearTimeout(prefetchTimerRef.current);
          prefetchTimerRef.current = window.setTimeout(() => onPrefetch(item.session.id), 150);
        }}
        onPointerLeave={() => window.clearTimeout(prefetchTimerRef.current)}
        onClick={(event) => {
          // 刚拖完松手的那次 click 不算点击，不然拖完会顺手打开会话。
          if (drag.swallowClick()) return;
          // 编辑态下点条目只用于收起编辑器，不切会话（输入框自身已 stopPropagation）。
          if (editing) { setEditingId(null); return; }
          // Ctrl/Cmd+点击 = 进入多选/增删(批量标记完成)。多选态下**普通点击整行也是增删**,
          // 不切会话:此前普通点击会退出多选并打开会话,只有右侧小勾选圈才增删——用户点行首
          // 想勾选却被带进了会话(用户反馈「多选的时候为什么点前面会直接进入会话」)。整行即
          // 勾选是 Gmail/Finder/iOS 编辑态的通行做法;退出多选走底部「取消」或 Esc。
          // Shift+点击 = 从锚点连选到这条,按用户眼前的顺序(折叠组里没画出来的不算)。
          if (event.shiftKey) {
            selectRange(item.session.id);
            return;
          }
          if (event.ctrlKey || event.metaKey || selectedIds.size > 0) {
            toggleSelected(item.session.id);
            return;
          }
          onSelect(item.session.id);
        }}
        aria-keyshortcuts={reorderEnabled ? "Control+Shift+ArrowUp Control+Shift+ArrowDown" : undefined}
        onKeyDown={(event) => {
          if (event.target !== event.currentTarget) return; // 内部按钮/输入框自理
          if ((event.ctrlKey || event.metaKey) && event.shiftKey && !event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
            // 先拦住：列表上的 ↑/↓ roving 不看修饰键，冒泡上去会把焦点挪走。S-6（走查 2026-11-12）：
            // 原先排序不可调时 keyboardMove 返回 false 不拦，焦点像普通 ↑↓ 一样下移，看着像挪了一位、
            // 其实顺序没变。现在一律拦下；按最近活动排序时说明原因并给「改用手动排序」。
            event.preventDefault();
            event.stopPropagation();
            if (!keyboardMove(item.session.id, event.key === "ArrowUp" ? -1 : 1) && sortMode !== "manual") {
              setActionError({ text: t.chat.sidebarReorderNeedsManual, action: { label: t.chat.sidebarSwitchToManual, run: () => { changeSortMode("manual"); setActionError(null); focusRow(item.session.id); } } });
            }
            return;
          }
          // S-9：多选态下 Shift+↑/↓ 从锚点连选（与 Shift+点击同一个 selectRange），Shift+空格连选到本行。
          if (selectedIds.size > 0 && event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
            if (event.key === "ArrowUp" || event.key === "ArrowDown") {
              event.preventDefault();
              event.stopPropagation();
              const shown = visibleIds();
              const nextId = shown[shown.indexOf(item.session.id) + (event.key === "ArrowUp" ? -1 : 1)];
              if (nextId === undefined) return;
              selectRange(nextId);
              listRef.current?.querySelector<HTMLElement>(`.chat-sidebar-item[data-session-id="${nextId}"]`)?.focus();
              return;
            }
            if (event.key === " ") {
              event.preventDefault();
              selectRange(item.session.id);
              return;
            }
          }
          if (event.key !== "Enter" && event.key !== " ") return;
          event.preventDefault(); // 空格不要滚动列表
          if (editing) { setEditingId(null); return; }
          // 与点击同口径：多选态下 Enter/空格是增删，不切会话。
          if (selectedIds.size > 0) { toggleSelected(item.session.id); return; }
          onSelect(item.session.id);
        }}
        onContextMenu={(event) => {
          // 右键恒开菜单：条目上右键出上下文菜单是通用惯例，不随设置失效——
          // card_menu_mode 只决定「⋯」按钮是否常驻，不再关掉右键这条路。
          event.preventDefault();
          // 触屏长按归拖拽管：拖起来时 Android 补发的这次不开菜单，没挪就松手由 onHold 开。
          if (drag.swallowContextMenu()) return;
          openMenuAt(item, event.clientX, event.clientY);
        }}
        onMouseEnter={(event) => {
          // 标题溢出时 hover 自动滚动（用户要求）：纯 CSS 测不出溢出（短标题不能被
          // 平白推走），进条目时量一次，溢出才滚动。时长按「固定速度 × 实际距离」算：
          // 统一 4s 会让长标题飞快、短标题蠕行（用户实拍）。jsdom 无 WAAPI，跳过。
          const nameEl = event.currentTarget.querySelector<HTMLElement>(".chat-sidebar-name-text");
          const inner = nameEl?.querySelector<HTMLElement>(".chat-sidebar-name-inner");
          if (!nameEl || !inner || typeof inner.animate !== "function") return;
          // 减少动效：不做跑马灯位移，完整标题看悬停提示（UI 优化 fix1 排查内联动画时一并补上）。
          if (typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
          const pendingLeave = titleScrollTimers.get(inner);
          if (pendingLeave) {
            window.clearTimeout(pendingLeave);
            titleScrollTimers.delete(inner);
          }
          // 还在跑（含 forwards 停在末尾）就不重启：重启会把进度打回 0，观感即「卡住」。
          if (titleScrollAnims.has(inner)) return;
          // 先挂上滚动态再量：让位「⋯」的 26px 只在滚动态生效（见 styles.css），量出来的溢出
          // 才含这段让位；不溢出就当帧撤掉——同一帧内增删，不会画出来，短标题后的图标不动。
          nameEl.classList.add("is-clip-scroll");
          const overflow = nameEl.scrollWidth - nameEl.clientWidth;
          if (overflow <= 4) { nameEl.classList.remove("is-clip-scroll"); return; }
          const anim = inner.animate(
            [{ transform: "translateX(0)" }, { transform: `translateX(${-(overflow + 8)}px)` }],
            { duration: Math.max((overflow / 60) * 1000, 1000), easing: "linear", fill: "forwards" },
          );
          titleScrollAnims.set(inner, anim);
        }}
        onMouseLeave={(event) => {
          const nameEl = event.currentTarget.querySelector<HTMLElement>(".chat-sidebar-name-text");
          const inner = nameEl?.querySelector<HTMLElement>(".chat-sidebar-name-inner");
          if (!nameEl || !inner) return;
          const timer = window.setTimeout(() => {
            nameEl.classList.remove("is-clip-scroll");
            titleScrollAnims.get(inner)?.cancel();
            titleScrollAnims.delete(inner);
            titleScrollTimers.delete(inner);
          }, 160);
          titleScrollTimers.set(inner, timer);
        }}
      >
        {/* 左侧 16px 状态位（对照 Kimi Code 会话行的 lead）：待授权 > 运行/待处理/出错 > 未读，
            没有状态就空着，标题照样对齐。agent 徽标不再占位：一行只留标题和时间。 */}
        <span className="chat-sidebar-lead">
          {awaitingApproval
            ? <i className="chat-sidebar-dot is-approval" role="img" aria-label={t.chat.sidebarApproval} data-tip={t.chat.sidebarApproval} />
            : showDot
              ? <i className={`chat-sidebar-dot is-${tone}`} role="img" aria-label={t.chat.status[tone]} data-tip={t.chat.status[tone]} />
              // 未读点（对照 Kimi 的 unread-dot）：回合结束时你没在看；当前会话不显示（正在看就是已读）。
              : item.unread && item.session.id !== activeId && (
                <i className="chat-sidebar-unread" role="img" aria-label={t.chat.unread} data-tip={t.chat.unread} />
              )}
        </span>
        <span className="chat-sidebar-text">
          {editing ? (
            <EditorInput
              initial={editDraft ?? (item.task_title && item.task_title !== UNNAMED_SESSION_SENTINEL ? item.task_title : "")}
              placeholder={t.sticker.renamePlaceholder}
              onSubmit={(value, byKey) => submitRename(item, value, byKey)}
              onCancel={() => cancelRename(item.session.id)}
            />
          ) : (
            <span className="chat-sidebar-name">
              {starred.has(item.session.cc_session_id) && (
                <span className="chat-sidebar-star" role="img" aria-label={t.sticker.star}><TopIcon /></span>
              )}
              {/* 标题必须包成自己的 span：.chat-sidebar-name 是 flex 容器（要排星标），
                  裸文本节点是匿名 flex item，容器上的 text-overflow 对它不生效——
                  长标题会溢出而不出省略号（实拍反馈）。
                  data-tip 也收窄到这里：挂整行容器时，悬到 ⋯ 按钮/目录行也弹标题（与
                  看板 U1-1 同口径）。内层再包一层 span 作滚动体：hover 自动滚动时 transform
                  只动内层，外层留作固定裁剪框——transform 加在 overflow:hidden 元素自身上
                  会连裁剪框一起推走，文字与可视窗口相对静止，视觉上卡住（实测踩过）。 */}
              <span className="chat-sidebar-name-text" data-tip={item.task_title || undefined}>
                <span className="chat-sidebar-name-inner">{item.task_title || t.sticker.waitingFirstInput}</span>
              </span>
              {/* 按工作区分组、本行没有副行（不是 worktree）时，PR/CI 与改动数跟在标题末尾，不单开一行。 */}
              {groupMode === "dir" && !worktreeNameOf(item) && badgeCwd && <SidebarGitBadgeSlot store={gitBadges} cwd={badgeCwd} />}
            </span>
          )}
          {/* 目录行 = 目录名 + 附加目录标(+N,与贴纸卡同款) + 最后活跃时间同排——前两者曾是
              纵排容器里的块级兄弟,+N 会掉到下一行(用户实拍)。按目录分组时目录名让位组头,但
              +N 照显(组头只写主仓,跨仓信息组头没有);时间恒显——此前条目完全没有时间信息,
              「哪条是刚才动的」只能靠猜。waiting 态改「已等待 X」警示色(与看板 B-3 同款,
              fmtWaited 复用 sticker/helpers):等待时长是召唤信号,「X 前」读不出急迫。 */}
          {/* 副行只在不按工作区分组时出现（对照 Kimi：平铺列表才在标题下写工作区名）。 */}
          {/* PR / CI 与改动数跟在工作区名（或 worktree 标）后面；分组视图没有副行时跟在标题行末尾（见上）。 */}
          {/* 摘要位自己订阅结果（SidebarGitBadgeSlot），回来时不重渲染整张列表。 */}
          {(dir && groupMode !== "dir" ? (
            <span className="chat-sidebar-metaline">
              <span className="chat-sidebar-meta" data-tip={item.cwd ?? undefined}>{dir}</span>
              {badgeCwd && <SidebarGitBadgeSlot store={gitBadges} cwd={badgeCwd} />}
            </span>
          ) : groupMode === "dir" && worktreeNameOf(item) ? (
            /* 包 5：按工作区分组时 worktree 会话归在原项目的组里，组内标出它开在哪个 worktree。 */
            <span className="chat-sidebar-metaline">
              <span className="chat-sidebar-wt" data-testid="sidebar-worktree-tag" data-tip={t.wtnew.tagTip(worktreeNameOf(item)!)}>{worktreeNameOf(item)}</span>
              {badgeCwd && <SidebarGitBadgeSlot store={gitBadges} cwd={badgeCwd} />}
            </span>
          ) : null)}
          {/* 目录已删除（worktree 清理掉了）：不管按不按目录分组都要显示——点进去恢复不了，得先说。 */}
          {item.cwd_missing && (
            <span className="chat-sidebar-gone" data-tip={t.worktree.dirMissingTip} data-testid="sidebar-dir-missing">{t.worktree.dirMissing}</span>
          )}
        </span>
        {/* 右侧：时间（对照 Kimi 的紧凑「3d」），等待中换成「已等待 X」警示色；悬停时让位给「⋯」。
            旧库/合成数据可能缺 last_event_at：NaN 会让 Intl 直接抛错炸掉整棵侧栏，缺失时不显示。 */}
        <span className="chat-sidebar-act">
          {/* 手机列表卡把状态写成字（样张：标题右侧「待批准 / 运行中 / 出错了」）：触屏没有悬停提示，
              只靠 7px 圆点的颜色分不清「等待输入」和「待处理」。文案与圆点的读屏名同源；圆点已有
              读屏名，这里对读屏隐藏免得念两遍。桌面侧栏不渲染。 */}
          {remoteUi() && selectedIds.size === 0 && (awaitingApproval || showDot) && (
            <span className={"chat-sidebar-status is-" + (awaitingApproval ? "approval" : tone)} aria-hidden="true">
              {awaitingApproval ? t.chat.sidebarApproval : t.chat.status[tone]}
            </span>
          )}
          {selectedIds.size === 0 && Number.isFinite(item.session.last_event_at) && (
            <span className={"chat-sidebar-time" + (tone === "waiting" ? " is-waited" : "")} data-tip={tone === "waiting" ? fmtWaited(item.session.last_event_at, t) : undefined}>
              {/* 等待中也用紧凑写法（「6m」），只换警示色：「已等待 6 分钟」在这一列太长（用户实拍），
                  左侧黄点已经说明它在等你，这里只交代等了多久。完整说法进悬停提示。 */}
              {fmtShortAgo(item.session.last_event_at, t)}
            </span>
          )}
        {selectedIds.size > 0 ? (
          /* 多选态:右侧换成勾选圈(未选空心/选中实心勾),点它增删不必按住 Ctrl。
             「⋯」菜单此间让位——多选时的动作在底部操作条上。 */
          // 条目本身是 role=button，子元素对读屏是「纯展示」，嵌在里面的按钮本来就读不到；
          // 哪怕 tabIndex=-1 + aria-hidden，可聚焦子元素仍被 axe 判 nested-interactive（serious）。
          // 所以只做指针热区（span，不可聚焦）：选中状态由条目的 aria-pressed 播报，
          // 键盘在条目上按 Enter/空格增删。
          <span
            className="chat-sidebar-check-btn"
            aria-hidden="true"
            onClick={(event) => {
              event.stopPropagation();
              toggleSelected(item.session.id);
            }}
          >
            <span className="chat-sidebar-check" aria-hidden="true">
              {selectedIds.has(item.session.id) && (
                <Icon glyph={icCheck} size={12} />
              )}
            </span>
          </span>
        ) : menuMode === "button" && (
          // 同上，只做指针热区。不占 Tab 停靠点：上百条会话每条一个 Tab 站等于列表无法
          // traverse。键盘走菜单键/Shift+F10（onContextMenu，菜单挂载即聚焦首项），
          // 读屏触屏走长按（onHold 开同一个菜单）。
          <span
            className="chat-sidebar-item-menu"
            aria-hidden="true"
            aria-label={t.sticker.cardMenu}
            onClick={(event) => {
              event.stopPropagation(); // 点菜单不切会话
              const rect = event.currentTarget.getBoundingClientRect();
              openMenuAt(item, rect.right, rect.bottom + 4);
            }}
          ><MoreIcon /></span>
        )}
        </span>
      </div>
    );
  };

  return (
    <aside className="chat-sidebar" style={width != null ? { width } : undefined}>
      {/* macOS：红绿灯独占一行（52px 带内垂直居中），按钮行整行下移——挤在红绿灯
          右侧时「新建会话」CTA 会被压得只剩图标（实拍反馈）。条带本身也是拖拽区。
          折叠钮留在这一行（红绿灯右侧，Finder/Safari 的侧栏开关惯例位），图标与
          折叠后标题栏的展开钮同款（panel-left），一开一收视觉上是同一个钮。 */}
      {isMac() && (
        <div className="chat-sidebar-mac-strip" data-tauri-drag-region>
          <button
            type="button"
            className="chat-sidebar-toggle"
            aria-label={t.chat.sidebarCollapse}
            data-tip={tipWithKeys(t.chat.sidebarCollapse, "Ctrl+B")}
            onClick={onCollapse}
          >
            <Icon glyph={icSidebarLeft} size={14} />
          </button>
        </div>
      )}
      {/* 窗口无系统装饰，侧栏顶部也要能拖动窗口——与右列标题栏同为 drag region。
          头部即「新建会话」按钮（Kimi 式整宽 CTA），不再放「会话」标题 + 迷你加号。 */}
      {/* 顶部两行入口（对照 Kimi Code 侧栏）：「新建会话」「搜索」都是图标加文字的平铺行，
          不再是整宽按钮加输入框。视图与排序挪到下面的「会话」小节标题。 */}
      <div className="chat-sidebar-head" data-tauri-drag-region>
        <button
          type="button"
          className={"chat-sidebar-new" + (newSessionActive ? " is-active" : "")}
          aria-current={newSessionActive ? "page" : undefined}
          onClick={() => void openNewSessionWindow().catch(() => {})}
        >
          <Icon glyph={icPlusCircle} size={16} />
          <span>{t.sticker.newSession}</span>
        </button>
        {remoteSettings && <RemoteSettings />}
      </div>
      {/* 会话搜索（Ctrl/Cmd+F 聚焦）：标题/仓库名下沉后端 LIKE，与看板同一条通道。 */}
      <div className="chat-sidebar-search">
        <Icon glyph={icSearch} size={14} className="chat-sidebar-search-icon" />
        <input
          ref={(el) => { searchBoxRef.current = el; if (searchInputRef) searchInputRef.current = el; }}
          value={search}
          placeholder={t.chat.sidebarSearch}
          aria-label={t.chat.sidebarSearch}
          // IME 合成守卫：与看板搜索框同款，拼音中间态不下发。
          onChange={(e) => {
            if ((e.nativeEvent as InputEvent).isComposing) return;
            setSearch(e.target.value);
          }}
          onCompositionEnd={(e) => setSearch(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && search) {
              e.stopPropagation(); // 有词时 Esc 只清词，不落到窗口级「拒绝审批」
              setSearch("");
              return;
            }
            // S-8（走查 2026-11-12）：搜索框里按 ↓ 原先不动，要 Tab 过清空钮、页签、筛选钮才到结果。
            // ↓ 进第一条结果（之后列表自己的 roving 接手）；有词时 Enter 打开第一条命中，但只在结果
            // 已按当前词刷新过时——防抖 300ms 内回车会打开旧词的第一条。
            if (e.nativeEvent.isComposing || workspacesView) return;
            const first = listRef.current?.querySelector<HTMLElement>(".chat-sidebar-item");
            if (e.key === "ArrowDown" && first) {
              e.preventDefault();
              first.focus();
            } else if (e.key === "Enter" && search.trim() && first && loadedQueryRef.current === search.trim()) {
              e.preventDefault();
              const id = Number(first.dataset.sessionId);
              if (Number.isFinite(id)) onSelect(id);
            }
          }}
        />
        {search && (
          <button
            type="button"
            className="chat-sidebar-search-x"
            aria-label={t.chat.sidebarSearchClear}
            onClick={() => setSearch("")}
          >×</button>
        )}
      </div>
      {/* 「进行中 / 已完成」两个标签页（对照 Kimi 的 tabOpen / tabDone）：已完成就是 archived，
          此前藏在「筛选 → 已归档会话」里、进去后还得靠一条状态条说明「换了一批会话」（7C-5）。
          做成常驻标签页，当前看的是哪一批一眼可见，取回也只要一击。 */}
      <div className="chat-sidebar-tabs" {...sidebarTabs.listProps} aria-label={t.chat.sidebarTitle}>
        {SIDEBAR_TABS.map((key) => (
          <button
            key={key}
            type="button"
            {...sidebarTabs.tabProps(key)}
            className={sidebarTab === key ? "is-active" : ""}
            onClick={() => selectSidebarTab(key)}
          >{key === "done" ? t.chat.sidebarTabDone : key === "open" ? t.chat.sidebarTabOpen : t.chat.sidebarTabWorkspaces}</button>
        ))}
      </div>
      {/* 「会话」小节标题（对照 Kimi Code）：右侧两个小图标——全部折叠 / 展开（分组时才有）、
          视图与排序。 */}
      <div className="chat-sidebar-section">
        <span className="chat-sidebar-section-label">{workspacesView ? t.chat.sidebarWorkspaces : t.chat.sidebarTitle}</span>
        {!workspacesView && groups && groups.length > 0 && (
          <button
            type="button"
            className="chat-sidebar-section-btn"
            aria-label={allGroupsFolded ? t.chat.sidebarUnfoldAll : t.chat.sidebarFoldAll}
            data-tip={allGroupsFolded ? t.chat.sidebarUnfoldAll : t.chat.sidebarFoldAll}
            onClick={toggleAllFolded}
          >
            <Icon glyph={allGroupsFolded ? icChevronsUpDown : icChevronsDownUp} size={14} />
          </button>
        )}
        {!workspacesView && <SidebarFilterMenu
          groupMode={groupMode}
          groupLabels={{ none: t.chat.groupNone, dir: t.chat.groupByDir }}
          sortMode={sortMode}
          sortLabels={{ manual: t.chat.sortManual, recent: t.chat.sortRecent }}
          onGroup={changeGroupMode}
          onSort={changeSortMode}
          onAdmin={onOpenAdmin}
          t={t}
        />}
      </div>
      {/* 工作区筛选生效时的标签（筛选从「工作空间」页签进入）：列表少了很多会话，界面上得有
          痕迹，也得有一键退出的地方（7C-5 的老问题，原先靠菜单钮上的小点）。 */}
      {dirFilter && !workspacesView && (
        <div className="chat-sidebar-scope">
          <span className="chat-sidebar-scope-label" data-tip={dirFilter}>{t.chat.sidebarScopeOnly(folderName(dirFilter) || dirFilter)}</span>
          <button type="button" className="chat-sidebar-scope-x" aria-label={t.chat.sidebarScopeClear} data-tip={t.chat.sidebarScopeClear} onClick={() => setDirFilter(null)}>×</button>
        </div>
      )}
      {workspacesView ? (
        <nav id={sidebarTabs.panelId} className="chat-sidebar-list" aria-label={t.chat.sidebarTabWorkspaces}>
          <WorkspaceDirectory
            options={workspaceOptions}
            onFilter={(cwd) => { setDirFilter(cwd); setArchivedView(false); setWorkspacesView(false); }}
            onHidden={refreshDirs}
          />
        </nav>
      ) : (
      <nav
        ref={listRef}
        id={sidebarTabs.panelId}
        className="chat-sidebar-list"
        aria-label={t.chat.sidebarTitle}
        onScroll={onScroll}
        // C-17：↑↓/Home/End 在会话条目间搬 DOM 焦点（roving，与 menu.tsx 同款）。
        // 只认焦点落在条目本体上的情况——编辑输入框/行内按钮里的方向键各自自理。
        onKeyDown={(e) => {
          if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Home" && e.key !== "End") return;
          const target = e.target as HTMLElement;
          if (!target.classList?.contains("chat-sidebar-item")) return;
          const items = Array.from(listRef.current?.querySelectorAll<HTMLElement>(".chat-sidebar-item") ?? []);
          if (items.length === 0) return;
          e.preventDefault();
          const at = items.indexOf(target);
          const next = e.key === "Home" ? items[0]
            : e.key === "End" ? items[items.length - 1]
            : items[(at + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length];
          next?.focus();
        }}
      >
        {sessions !== null && syncLost && (
          <div className="chat-sync-warn is-lost chat-sidebar-sync-warn" role="alert">{t.chat.syncInterrupted}</div>
        )}
        {sessions === null && loadFailed && (
          /* 首载失败：不是「暂无会话」。给重试出口——load 的失败路径已把 limit 等状态
             留在首页口径，直接重发 refresh 即可。 */
          <div className="chat-sidebar-empty is-error">
            {t.chat.sidebarLoadFailed}
            <button
              type="button"
              className="chat-sidebar-retry"
              onClick={() => { setLoadFailed(false); void loadRef.current("refresh"); }}
            >{t.chat.sidebarRetry}</button>
          </div>
        )}
        {sessions === null && !loadFailed && <div className="chat-sidebar-empty">{t.chat.sidebarLoading}</div>}
        {sessions !== null && sessions.length === 0 && (
          <div className="chat-sidebar-empty">
            {search.trim() ? t.chat.sidebarEmptySearch : dirFilter ? t.chat.sidebarEmptyDir : archivedView ? t.chat.sidebarEmptyArchived : t.chat.sidebarEmpty}
            {/* 真正一条会话都没有时，空态本身给出下一步（对照 Kimi 空列表直接给新建入口），
                筛选/搜索/归档视图下的空是「条件不中」，不该引导新建。 */}
            {!search.trim() && !dirFilter && !archivedView && <SidebarEmptyNext newSessionActive={!!newSessionActive} t={t} />}
          </div>
        )}
        {/* 已完成页顶部：批量清理已合并 worktree。手机端只有侧栏没有会话管理页，批量入口放在这里
            两端都够得着（用户定：手机端同样提供）。 */}
        {archivedView && !workspacesView && sessions !== null && sessions.length > 0 && (
          <div className="chat-sidebar-cleanbar">
            <button
              type="button"
              {...busyProps(cleaning, () => {
                setCleaning(true);
                void runBatchCleanup(t).then((text) => {
                  setCleaning(false);
                  if (text) setCleanResult(text);
                  void loadRef.current("refresh");
                });
              })}
            >{cleaning ? t.worktree.batchScanning : t.worktree.batch}</button>
          </div>
        )}
        {renderedGroups
          ? renderedGroups.map(({ group, isFolded, canCollapse, idleCount, shown }) => {
              // 组头汇总点:折起来时它是这一组唯一的状态出口,取组内最强的召唤。
              const approval = group.items.some((item) => approvalAwaitingIds.has(item.session.id));
              const tone = group.items
                .map((item) => sessionTone(item.connected, item.session.status, item.pending_review, item.errored, item.busy_subagents, item.screen_state, item.screen_assumed))
                .reduce<SessionTone | null>((best, cur) => (best && TONE_RANK[best] >= TONE_RANK[cur] ? best : cur), null);
              const showDot = !!tone && TONE_RANK[tone] > 0;
              // 未读排在出错之后、等待/运行之前（对照 Kimi 分组汇总：审批 > 提问 > 失败 > 未读）：
              // 组折起来时行上的未读点看不见，组头只剩一个黄/绿点，「跑完了你还没看」就丢了。
              // 当前会话不算（正在看就是已读，与行上的未读点同口径）。
              const unread = (!tone || TONE_RANK[tone] < TONE_RANK.error)
                && group.items.some((item) => item.unread && item.session.id !== activeId);
              const groupDot = approval
                ? <i className="chat-sidebar-dot is-approval" role="img" aria-label={t.chat.sidebarApproval} data-tip={t.chat.sidebarApproval} />
                : unread
                  ? <i className="chat-sidebar-dot is-unread" role="img" aria-label={t.chat.unread} data-tip={t.chat.unread} />
                  : showDot && <i className={`chat-sidebar-dot is-${tone}`} role="img" aria-label={t.chat.status[tone]} data-tip={t.chat.status[tone]} />;
              // 目录分组的组头汇总「N 项待处理」（对照 Kimi 的 attentionTitle）：组折起来也知道里面有几件要你决定。
              const attention = group.items.filter((item) => approvalAwaitingIds.has(item.session.id)
                || sessionTone(item.connected, item.session.status, item.pending_review, item.errored, item.busy_subagents, item.screen_state, item.screen_assumed) === "pending").length;
              return (
                <div
                  key={group.key}
                  data-group-key={group.key}
                  className={"chat-sidebar-group"
                    + (groupDrag.dragId === group.key ? " is-dragging" : "")
                    + (groupDrag.dropAt?.id === group.key && groupDrag.dragId !== group.key ? (groupDrag.dropAt.after ? " is-drop-after" : " is-drop-before") : "")}
                  // 按住分组头拖动调整工作区顺序；组里的会话行、头上的按钮各管各的。
                  onPointerDown={(event) => {
                    const target = event.target as HTMLElement;
                    if (!target.closest(".chat-sidebar-group-head") || target.closest(".chat-sidebar-item, .chat-workspace-action, .chat-sidebar-check-btn")) return;
                    groupDrag.onPointerDown(event, group.key, target.closest<HTMLElement>(".chat-sidebar-group-head"));
                  }}
                >
                    <WorkspaceHeader
                      cwd={group.cwd}
                      count={group.items.length}
                      attention={attention}
                      trailing={renderGroupCheck(group.items, folderName(group.cwd) || t.chat.sidebarNoDir)}
                      folded={isFolded}
                      // 刚拖完分组头松手的那次 click 不算折叠。
                      onToggle={() => { if (!groupDrag.swallowClick()) toggleFolded(group.key); }}
                      leading={
                        // 对照 Kimi 的分组头：文件夹图标代替折叠箭头，折着是合上的文件夹。
                        <Icon glyph={isFolded ? icFolder : icFolderOpen} size={16} className="chat-workspace-folder" />
                      }
                      summary={groupDot}
                      onFilter={setDirFilter}
                    />
                  {shown.map(renderItem)}
                  {/* 未运行的会话默认收起：一个跑了一阵的目录里，历史会话能堆到几十条，
                      把当前真正在跑的那一两条挤出视野。折起来但不丢——点一下就回来。 */}
                  {!isFolded && canCollapse && (
                    <button
                      type="button"
                      className="chat-sidebar-more"
                      aria-expanded={revealed.has(group.key)}
                      onClick={() => toggleRevealed(group.key)}
                    >
                      {revealed.has(group.key)
                        ? t.chat.sidebarHideIdle
                        : t.chat.sidebarShowIdle(idleCount)}
                    </button>
                  )}
                </div>
              );
            })
          : (ordered ?? []).map(renderItem)}
        {/* 下一页在路上。挂在真实在途状态上：翻页失败会清掉它，不会留下一个永远
            转不完的 loading 行。 */}
        {sessions !== null && sessions.length > 0 && growing && (
          <div className="chat-sidebar-empty">{t.chat.sidebarLoading}</div>
        )}
        {/* 对话内容全文搜索:LIKE 结果之外的第二段。显式按钮触发(扫 transcript 文件,
            不随击键);结果每会话一条(标题 + 命中摘录),点击直达。 */}
        {search.trim() && sessions !== null && (
          <div className="chat-sidebar-deep">
            {deepHits === null && !deepSearching && (
              <button type="button" className="chat-sidebar-more" onClick={runDeepSearch}>
                {t.chat.searchTranscripts}
              </button>
            )}
            {deepSearching && <div className="chat-sidebar-empty">{t.chat.searchTranscriptsRunning}</div>}
            {deepHits !== null && !deepSearching && (deepHits.length === 0
              ? <div className="chat-sidebar-empty">{t.chat.searchTranscriptsEmpty}</div>
              : <>
                  <div className="chat-sidebar-deep-head">{t.chat.searchTranscriptsHits}</div>
                  {deepHits.map((hit) => (
                    <button
                      type="button"
                      key={hit.sessionId}
                      className="chat-sidebar-deep-hit"
                      onClick={() => onSelect(hit.sessionId)}
                    >
                      <span className="chat-sidebar-deep-title">
                        <span className="chat-sidebar-name-text">{hit.title || t.sticker.waitingFirstInput}</span>
                        <span className="chat-sidebar-meta">{hit.projectName}</span>
                      </span>
                      <span className="chat-sidebar-deep-excerpt">{hit.excerpt}</span>
                    </button>
                  ))}
                </>)}
          </div>
        )}
      </nav>
      )}
      {/* G-16：全侧栏唯一的读屏汇总播报源（视觉隐藏），取代每条会话一个 role=status。 */}
      <div className="chat-sidebar-sronly" role="status">{moveAnnounce || liveSummary}</div>
      {cleanResult && !actionError && (
        <div className="chat-sidebar-error is-undo is-multiline" role="status">
          <span>{cleanResult}</span>
          <button type="button" onClick={() => setCleanResult(null)}>{t.dialog.ok}</button>
        </div>
      )}
      {/* 归档后所在 worktree 能清理就提示（二期包 5，不自动删）。 */}
      {!actionError && <ArchivedWorktreePrompt t={t} className="chat-sidebar-error is-undo" />}
      {/* 动作失败/提示的唯一出口（S-5）：不计时消失，× 关闭或下一个动作时让位。 */}
      {actionError && (
        <div className="chat-sidebar-error is-undo is-multiline is-notice" role="alert">
          <span>
            {actionError.text}
            {actionError.reason && <span className="chat-sidebar-error-reason">{actionError.reason}</span>}
          </span>
          {actionError.action && (
            <button type="button" onClick={() => { const run = actionError.action!.run; setActionError(null); run(); }}>{actionError.action.label}</button>
          )}
          <button type="button" className="chat-sidebar-error-x" aria-label={t.chat.close} data-tip={t.chat.close} onClick={() => setActionError(null)}>×</button>
        </div>
      )}
      {/* 批量归档操作条:多选态常驻底部(Ctrl+点击增删,Esc 退出)。 */}
      {selectedIds.size > 0 && (
        <div className="chat-sidebar-batch" role="toolbar" aria-label={t.chat.selectedCount(selectedIds.size)}>
          <span data-tip={t.chat.selectRangeHint}>{t.chat.selectedCount(selectedIds.size)}</span>
          <button type="button" className="btn-primary" onClick={() => void archiveSelected()}>{t.chat.archiveSelected}</button>
          <button type="button" onClick={() => setSelectedIds(new Set())}>{t.chat.cancelSelect}</button>
        </div>
      )}
      {ctxMenu && ctxItem && (
        <CardContextMenu
          x={ctxMenu.x}
          y={ctxMenu.y}
          starred={starred.has(ctxItem.session.cc_session_id)}
          archived={ctxItem.archived}
          onStar={() => toggleStar(ctxItem.session.cc_session_id)}
          onRename={() => startRename(ctxItem)}
          onArchive={() => void toggleArchived(ctxItem)}
          // 多选入口:Ctrl+点击对鼠标用户不可发现,菜单里给一个显式的起点。
          onSelectMode={() => {
            selectAnchorRef.current = ctxItem.session.id;
            setSelectedIds(new Set([ctxItem.session.id]));
          }}
          onOpenDir={!remoteUi() && ctxItem.cwd ? () => void openProjectDir(ctxItem.cwd!).catch(() => {}) : null}
          // 结束会话按 endable 口径（与对话窗/贴纸同源）：本 GUI 托管的 PTY，或进程仍活着
          // 的孤儿会话（后端按 pid 杀树兜底）；外部终端里跑的会话杀不了，后台会话杀了也会
          // 被 supervisor 拉回来（endable 已内含排除）。远程同样放行:stop_managed_terminal
          // 已进 /rpc 白名单(remote.rs 头注释记录了放行理由)。
          onEndSession={ctxItem.endable ? () => endSession(ctxItem) : null}
          worktreeSessionId={ctxItem.archived && !ctxItem.foreign && !ctxItem.cwd_missing ? ctxItem.session.id : null}
          onWorktreeDone={(outcome: RemovalOutcome) => {
            if (outcome.kind === "failed") setActionError({ text: outcome.message });
            else if (outcome.kind === "removed") {
              if (!outcome.branchDeleted) setActionError({ text: t.worktree.removedKeepBranch });
              void loadRef.current("refresh");
            }
          }}
          onClose={() => setCtxMenu(null)}
        />
      )}
      {/* 常驻底部：会话列表可能很长并滚动，设置入口不能跟着滚走。
          远程 v1 不含设置页(宿主执行类,default-deny)——整条底栏隐藏。 */}
      {!remoteUi() && (
      <div className="chat-sidebar-footer">
        <button
          type="button"
          className="chat-sidebar-settings"
          onClick={() => void invoke("open_settings").catch(() => {})}
        >
          <Icon glyph={icSettings} size={14} />
          <span>{t.sticker.openSettings}</span>
        </button>
      </div>
      )}
    </aside>
  );
}
