import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  getLiveSessionsCounts,
  getLiveSessionsPage,
  getSettings,
  setSettings,
  openNewSessionWindow,
  LiveSession,
  LiveSessionCounts,
  PageCursor,

} from "./api";
import { useBoardRefresh } from "./hooks/useBoardRefresh";
import { useSettingsEffect } from "./hooks/useSettings";
import type { IslandCompactStyle } from "./api";
import { type Item, type Tab } from "./views/sticker/types";
import { Island, IslandStrip, ISLAND_ALERT_SIZE, ISLAND_EAR, ISLAND_PILL_HEIGHT, ISLAND_STRIP_SIZE } from "./views/Island";
import { IslandPanel } from "./views/island/IslandPanel";
import { liveIslandUsage, orderIslandUsage, useIslandUsage } from "./views/island/useIslandUsage";
import { openIslandSession, type IslandOpenError } from "./views/island/openSession";
import { byUrgency, islandCounts, islandTone, menubarSegments } from "./views/Island";
import { EYES_BUSY_AT, useEyeCue, useEyeMode } from "./views/island/islandParts";
import type { IslandAlertEvent } from "./generated/contracts/IslandAlertEvent";
import { useUpdate } from "./useUpdate";
import { useShowWhenReady } from "./useShowWhenReady";
import { useT } from "./i18n";
import { FLOATING_HOST, isMac, type IslandHost } from "./platform";
import { reconcileRows, sameRefs, type RowCache as SharedRowCache } from "./rowCache";
import { reconcilePendings, type PendingRegistry } from "./views/sticker/pending";
import { pushEscLayer } from "./escLayers";
import {
  initStickerWindowState,
  updateStickerWindowState,
} from "./windowState";

type IslandMode = "compact" | "alert" | "expanded";
/// 展开面板的窗口宽度（含两侧耳朵）。高度按面板实测。
const ISLAND_PANEL_WIDTH = 600 + ISLAND_EAR * 2;
/// 面板没量过之前先按这个高度摆（三行会话 + 页码点的常见高度）。
const ISLAND_PANEL_HEIGHT_GUESS = 250;
/// 胶囊最窄宽度（逻辑像素）：空态也得是一颗好点的胶囊，不是一条缝。
const ISLAND_PILL_MIN_WIDTH = 120;
/// 提醒停留时长的缺省值（设置里可改 4 / 6 / 10 秒）。
const ISLAND_ALERT_MS_DEFAULT = 6_000;
/// 提醒种类 → 设置里的开关。计划确认、屏幕上挂着交互都算「等待审批」，用户分不出也不必分。
function islandAlertSwitch(kind: string): string {
  return kind === "plan" || kind === "blocked" ? "approval" : kind;
}
/// 要人做决定的提醒（审批、提问、计划、屏幕阻塞、出错）插到队头。跑完只是告知，可以晚点看。
function isUrgentAlert(kind: string): boolean {
  return kind !== "waiting";
}
/// 急事插队时，正在显示的「跑完」至少露这么久再让位：刚弹出就被换掉像闪了一下。
/// 再加两条提醒之间 300ms 的间隔，审批在 1.5s 内弹出（二期定：原来排在三条跑完后面要等约 19s）。
const ISLAND_ALERT_YIELD_MS = 1_200;
/// 鼠标从提醒上移开后，提醒再留多久才缩回。
const ISLAND_ALERT_LINGER_MS = 3_000;
/// 鼠标停在提醒上最多留多久（i5）：提醒压在最大化窗口的标题栏上，人想点底下的东西，光标一移上去
/// 倒计时就停，提醒反而不走。到点照常缩回；要细看的有「打开」和对话窗。
const ISLAND_ALERT_HOVER_MAX_MS = 15_000;
/// 收回动画时长，与 styles.css 的 --dur-island（.island-shell.is-leaving 在用）同源。
const ISLAND_LEAVE_MS = 180;

const TAB_KEY = "meowo-tab";
const PAGE_SIZE = 100; // 贴纸会话每页条数，与首屏一致

// board-changed 常是「空转」：命令写库后 db-watcher 又为同一次写入报一次、liveness 轮询重发、
// 甚至 app 自身读库触碰 board.db-wal/-shm 的 mtime 也会被 watcher 当成变更而回声。这些刷新拉回的
// 数据与当前完全一致，若照旧整表替换成新对象引用，会让整个虚拟列表无谓重渲染（视觉上「一直在更新」）。
//
// 行级 JSON 缓存 + 结构共享：每行 stringify 一次、与上次的缓存比对，没变的行**复用旧对象引用**。
// 于是（1）空转刷新整表引用不变（sameRefs 命中，跳过 setState）；（2）只有一行变化时，其余行
// 引用稳定，配合卡片层的 memo 只重渲染那一张。全字段 JSON 比较是刻意的**排除法**——按字段挑
// 白名单的签名方案会在漏字段时让 UI 静默不更新（sameHistoryMeta 的注释记过三回同类事故）。
// 缓存只增不清：键是 session.id，量级为窗口生命周期内见过的会话数，几百条小对象，不值得管理。
// 实现移到 ../rowCache 与对话窗侧栏共用（那边此前缺这层，几百条时每次空转刷新都全表重建）。
type RowCache = SharedRowCache<Item>;

/** 当前 tab 对应的总会话数（用于 hasMore 与加载守卫，必须与后端 filter 语义一致）。 */
function totalFor(filter: Tab, counts: LiveSessionCounts): number {
  switch (filter) {
    case "running":
      return counts.running;
    case "waiting":
      return counts.waiting;
    case "all":
      // "all" tab 后端过滤为 archived=0
      return counts.total - counts.archived;
  }
}



export function App() {
  // 启动显示闸门（W-4/W-17）：贴纸几何（尺寸/吸附边/置顶）在 settings.json，需一次 IPC 读取；
  // 读完并完成「沿用折叠 / 还原正常尺寸」之前不渲染内容、不显示窗口——否则闪一帧
  // 「默认尺寸大框里的错误内容」。后端 show_after_grace 兜底不变。
  const [bootReady, setBootReady] = useState(false);
  // 贴纸窗口以 visible:false 创建（tauri.conf.json），首帧渲染后再显示，消除启动瞬间的白框闪烁。
  // 不抢焦点（窗口配置 focus:false，开机自启同理）。macOS 的岛是非激活面板，显隐走
  // island_set_visible（普通 show 会让它成为 key、吃掉前台应用的键盘），这里不管。
  useShowWhenReady({ focus: false, enabled: !isMac(), ready: bootReady });
  const [items, setItems] = useState<Item[]>([]);
  const [counts, setCounts] = useState<LiveSessionCounts>({
    total: 0,
    running: 0,
    waiting: 0,
    archived: 0,
  });
  const [loadingMore, setLoadingMore] = useState<boolean>(false);
  const [reachedEnd, setReachedEnd] = useState<boolean>(false);
  // 冷启动首页加载：未落地前 initialLoading=true（Sticker 显示加载占位而非假空态）；
  // 首页/刷新失败置 loadError（Sticker 显示「加载失败 + 重试」），任一首页型加载成功后清除。
  const [initialLoading, setInitialLoading] = useState<boolean>(true);
  const [loadError, setLoadError] = useState<boolean>(false);
  // 重试：递增 nonce 重新触发下方的 filter/search 首页加载 effect。
  const [retryNonce, setRetryNonce] = useState(0);
  const retryLoad = useCallback(() => setRetryNonce((n) => n + 1), []);
  // 「已归档」不再是看板 tab（管理入口在设置 → 会话）；持久化里存过它的自动退回「全部」。
  const [filter, setFilter] = useState<Tab>(() => {
    const s = localStorage.getItem(TAB_KEY);
    return s === "waiting" || s === "running" ? s : "all";
  });
  const [search, setSearch] = useState("");
  // 搜索结果是临时视图，不能覆盖用户搜索前已经加载好的普通列表（包括它的服务端顺序）。
  // 按 tab 分开缓存，避免搜索中切 tab/清空时拿另一个 tab 的列表来恢复。
  const unsearchedItemsRef = useRef<Partial<Record<Tab, Item[]>>>({});
  // 切 tab/搜索的首页请求在途（B-4）：Sticker 据此保留旧列表灰化直到新数据就绪，
  // 不再先渲染「旧数据按新条件过滤出的子集」（waiting 的 ASC 到达后整列表翻转的来源）。
  const [switching, setSwitching] = useState(false);
  const pickFilter = useCallback((f: Tab) => {
    if (f !== filter) setSwitching(true);
    setFilter(f);
    localStorage.setItem(TAB_KEY, f);
  }, [filter]);
  // 窗口置顶偏好（W-17）：存 settings.json，启动时读入；切换经 togglePin 原子落盘。
  const [pinned, setPinned] = useState(false);
  const pinnedRef = useRef(false); // 同步镜像：togglePin 等回调里取最新值不依赖闭包
  const applyPinned = useCallback((p: boolean) => {
    pinnedRef.current = p;
    setPinned(p);
  }, []);
  const togglePin = useCallback(() => {
    applyPinned(!pinnedRef.current);
    updateStickerWindowState({ pinned: pinnedRef.current });
  }, [applyPinned]);
  // 只读检查：仅驱动贴纸设置钮上的更新红点；下载/安装由更新窗口（views/Updater）全权负责。
  const { status: upStatus } = useUpdate({ automatic: true });

  // 折叠条恒显示全部「连接中」会话（running + waiting），与当前选中 tab 无关——
  // 故独立于分页 items 单独加载（按状态查，覆盖旧但仍连接的会话，不受 tab/分页窗口影响）。
  const [stripSessions, setStripSessions] = useState<Item[]>([]);
  // 灵动岛「打开」在轻量模式下按 pid 找外部终端（I-2），回调里同步读最新一份。
  const stripSessionsRef = useRef<Item[]>([]);
  stripSessionsRef.current = stripSessions;
  const stripRowCacheRef = useRef<RowCache>(new Map());
  const loadStrip = useCallback(() => {
    Promise.all([
      // includeForeign:折叠条与主列表同视野——dev 构建下安装版的连接中会话也计入。
      getLiveSessionsPage("running", null, null, 200, null, true),
      getLiveSessionsPage("waiting", null, null, 200, null, true),
    ])
      .then(([r, w]) => {
        const map = new Map<number, Item>();
        [...r.items, ...w.items].forEach((s) => map.set(s.session.id, s as Item));
        const next = reconcileRows(stripRowCacheRef.current, [...map.values()]);
        setStripSessions((prev) => (sameRefs(prev, next) ? prev : next));
      })
      .catch(() => {});
  }, []);
  useEffect(() => {
    loadStrip();
  }, [loadStrip]);

  // 请求序号守卫：并发刷新时旧响应可能晚于新响应返回，仅当自己仍是最新一次请求才写入。
  const refreshSeqRef = useRef(0);
  // 列表行的结构共享缓存（与折叠条的各自独立，两边行对象不混用）。
  const itemsRowCacheRef = useRef<RowCache>(new Map());
  // 下一页游标：**只认后端响应里带回的扫描位置**。列表做过 connected-first 排序，
  // 末项不是本页时间上最旧的一条，从 items 里自己推游标会重复/漏页。
  const nextCursorRef = useRef<PageCursor | null>(null);
  // 「正在启动」占位卡（pending PTY，负 id）的登记簿：后端在首页查询里合成占位项、
  // claim 认领后摘除；这里按 provider+cwd 对账（见 sticker/pending.ts），保证
  // 「真卡出现即撤占位、启动失败不留僵尸卡」，并兜住 mergeTail 把旧占位留在尾部的缝隙。
  const pendingsRef = useRef<PendingRegistry>(new Map());
  const loadPage = useCallback(
    async (
      filter: Tab,
      search: string,
      cursor: PageCursor | null,
      limit: number = PAGE_SIZE,
      // board-changed 增量刷新（B-6）：首页权威（刚有事件变化的会话按 last_event_at
      // 必然排在前列，状态/排序迁移都在首页里反映），已滚出首页的旧行按 id 保留，
      // 不再整窗重查（500 条时每事件重查 500 行）。仅 cursor === null 时有效。
      mergeTail: boolean = false
    ): Promise<{ page: Item[]; cursor: PageCursor | null; applied: boolean }> => {
      const seq = ++refreshSeqRef.current;
      // counts 只在首页/刷新（cursor === null）时才需要；loadMore 复用已有 counts，
      // 避免频繁 loadMore 时对 counts 做纯重复的后端查询（审查发现）。
      const needCounts = cursor === null;
      try {
        const [countsRes, res] = await Promise.all([
          needCounts ? getLiveSessionsCounts() : Promise.resolve(null),
          // includeForeign:贴纸看板是监控视野的主入口,dev 构建下聚合安装版的会话
          // (外库卡只读展示,后端仅在首页附加,翻页游标不受影响)。
          // includePending:同入口附带「正在启动」占位卡(pending PTY,负 id,同样只进首页)。
          getLiveSessionsPage(filter, search, cursor, limit, null, true, true),
        ]);
        const page = res.items;
        const applied = seq === refreshSeqRef.current;
        if (!applied) return { page: page as Item[], cursor: res.next_cursor, applied };
        nextCursorRef.current = res.next_cursor;
        // 占位卡对账（仅首页响应；loadMore 的响应里没有占位项，既有登记原样保留）。
        // 对账后占位从 page 剥离、改由登记簿统一前置——否则 mergeTail 会把上一轮
        // 的占位行当「未进首页的旧行」永远留在尾部。
        const pendings = cursor === null
          // 占位行同样过行级结构共享：后端每次刷新都重新合成占位项，内容没变时
          // 复用旧引用，否则占位在场期间每次 board-changed 都整表重渲染（见 reconcileRows）。
          ? reconcileRows(
              itemsRowCacheRef.current,
              reconcilePendings(pendingsRef.current, page as Item[], Date.now())
            )
          : [];
        const pageReals = cursor === null ? page.filter((l) => l.session.id > 0) : page;
        if (countsRes) {
          setCounts((prev) =>
            prev.total === countsRes.total &&
            prev.running === countsRes.running &&
            prev.waiting === countsRes.waiting &&
            prev.archived === countsRes.archived
              ? prev
              : countsRes
          );
        }

        setItems((prev) => {
          if (cursor === null) {
            // 首页请求（切 tab / 首次加载 / board-changed 刷新）：直接按服务端顺序整体替换。
            // 服务端已按 last_event_at DESC 排序，天然反映既有会话的最新排序位置——
            // 若只按 prev 数组旧位置合并、仅将全新会话插到最前，已存在会话（如恢复的旧会话）
            // 排序键变化后不会移动，得等用户手动切 tab 才会跳到正确位置（回归 bug）。
            // 不在 page 里的会话（状态迁移出当前 filter/归档/删除）也随整体替换自然被移除。
            // 占位卡（负 id）不走 page——它由登记簿对账后统一前置（见上面 pendings）。
            const next = reconcileRows(itemsRowCacheRef.current, pageReals as Item[]);
            if (mergeTail && prev.length > next.length) {
              // 增量修补（B-6）：首页权威置顶，未进首页的旧行原样保留在尾部（这些会话
              // 近期无事件，展示状态不会变）；合并后按 connected 稳定分区恢复全局不变量
              // （与 loadMore 合并同规则）。上一轮的占位行（负 id）不属于「旧行」，
              // 必须排除——它的去留只由登记簿决定。
              const fresh = new Set(next.map((l) => l.session.id));
              const merged = [...next, ...prev.filter((l) => l.session.id > 0 && !fresh.has(l.session.id))];
              const kept = [...pendings, ...merged.filter((l) => l.connected), ...merged.filter((l) => !l.connected)];
              if (!search.trim()) unsearchedItemsRef.current[filter] = kept;
              return sameRefs(prev, kept) ? prev : kept;
            }
            const withPendings = [...pendings, ...next];
            if (!search.trim()) unsearchedItemsRef.current[filter] = withPendings;
            // 空转刷新（数据未变）保持原引用，跳过整表重渲染，消除视觉抖动（见 reconcileRows）。
            return sameRefs(prev, withPendings) ? prev : withPendings;
          }
          // loadMore（cursor 非空）：按 id 合并，保留已加载会话原有顺序，新条目追加到末尾。
          // 先过结构共享：新页里与已加载行内容相同的，合并后保持旧引用。
          const reconciled = reconcileRows(itemsRowCacheRef.current, page as Item[]);
          const map = new Map(prev.map((l) => [l.session.id, l]));
          const append: Item[] = [];
          for (const l of reconciled) {
            if (!map.has(l.session.id)) append.push(l);
            map.set(l.session.id, l);
          }
          const merged = [...prev.map((l) => map.get(l.session.id)!), ...append];
          // connected-first 是**每页内**排序（session_query 对单页做），直接追加会把第 2 页
          // 的已连接会话排到第 1 页断开会话之下——活跃会话散落在历史会话之间，越滚越乱。
          // 合并后按 connected 稳定分区恢复全局不变量（两组内部保持原有相对顺序）。
          const next = [...merged.filter((l) => l.connected), ...merged.filter((l) => !l.connected)];
          if (!search.trim()) unsearchedItemsRef.current[filter] = next;
          return next;
        });
        return { page: page as Item[], cursor: res.next_cursor, applied };
      } catch (err) {
        console.error("[loadPage] 加载失败：", err);
        throw err;
      }
    },
    []
  );

  // 一次刷新：已加载窗口超过首页时只重查首页（PAGE_SIZE）+ 增量修补尾部（B-6，不再
  // 每次事件整窗重查 500 行）。刚有事件的会话按 last_event_at 必排在首页，状态/排序
  // 变化都被首页权威覆盖；无事件的尾部旧行原样保留，用户已滚动加载的窗口不被打回
  // 第一页（P0 的诉求改由 mergeTail 保住）。窗口未超首页时首页就是整窗，整体替换
  // （filter 迁出的会话随之移除，与 loadMore 前行为一致）。
  // 例外：归档失败回滚必须整窗重查——被乐观移除的卡可能深在尾部，首页 + 增量修补
  // 捞不回来（fullRefreshOnceRef 由 onArchiveFailed 置位）。
  const itemsLenRef = useRef(0);
  itemsLenRef.current = items.length;
  const fullRefreshOnceRef = useRef(false);
  // 首页/搜索视图正在切换时，旧 items 仍会短暂留在 DOM。此时虚拟列表若触发 loadMore，
  // 会拿旧列表游标发起一个更新请求，取消首页请求并把结果按旧顺序合并（清空搜索排序错乱的根因）。
  const resettingPageRef = useRef(false);
  const pageResetSeqRef = useRef(0);
  const doRefresh = useCallback(() => {
    const resetSeq = ++pageResetSeqRef.current;
    resettingPageRef.current = true;
    const full = fullRefreshOnceRef.current;
    fullRefreshOnceRef.current = false;
    const incremental = !full && itemsLenRef.current > PAGE_SIZE;
    // 增量刷新不动 reachedEnd：尾部原样保留，此前「已到底」依然成立。
    if (!incremental) setReachedEnd(false);
    loadPage(filter, search, null, incremental ? PAGE_SIZE : Math.max(PAGE_SIZE, itemsLenRef.current), incremental)
      .then(({ cursor, applied }) => {
        if (applied) {
          setInitialLoading(false);
          setLoadError(false);
          if (resetSeq === pageResetSeqRef.current) {
            resettingPageRef.current = false;
            // 切换期的首页请求可能被这次更新的 board-changed 刷新取代（applied=false 永不
            // 落地）——切换灰化（B-4）在这里也要解除，否则列表永远灰着。
            setSwitching(false);
          }
          if (cursor === null) setReachedEnd(true);
        }
      })
      .catch(() => {
        if (resetSeq === pageResetSeqRef.current) {
          setInitialLoading(false);
          setLoadError(true);
          resettingPageRef.current = false;
          setSwitching(false);
        }
      });
    // 折叠条数据独立刷新（不随 tab）——但只在非 normal 态：正常态下缩略条不渲染，
    // 每次 board-changed 都跟着拉 2×200 条只为一个折叠时才用的计数，不值。挂载时和
    // 折叠动作发起时（doCollapse）各拉一次，保证真折叠的那一刻数据是新的。
    loadStrip();
  }, [filter, search, loadPage, loadStrip]);

  // board-changed 订阅 + leading/trailing 节流统一在 useBoardRefresh：每次刷新是
  // counts + 一整页 + 折叠条两查询，页大小随滚动增长，值得省。E2E 观测点也在 hook 里。
  const refresh = useBoardRefresh(doRefresh);

  // loadingMore 是 state：setLoadingMore(true) 到下次渲染落地之间，同一 tick 内 loadMore 仍可按
  // 旧闭包重入（Sticker 触底 effect 在一个渲染批内可能连发），以相同游标重复请求下一页。
  // ref 镜像同步置位，重入当场被拒（与 useLoginOperations 的 pendingRef 同一套路）。
  const loadingMoreRef = useRef(false);
  const loadMore = useCallback(async () => {
    if (resettingPageRef.current || loadingMoreRef.current || reachedEnd) return;
    // 游标必须用后端上次带回的扫描位置：从可见列表末项推会撞上 connected-first 排序
    // （末项不是时间上最旧的一条），每页都会重复返回被顶到前排的会话。
    const cursor = nextCursorRef.current;
    if (cursor === null) {
      setReachedEnd(true);
      return;
    }
    loadingMoreRef.current = true;
    setLoadingMore(true);
    try {
      const { cursor: next, applied } = await loadPage(filter, search, cursor);
      // 请求过程中若已被更新的请求（如切 tab/刷新）取代，本次结果不再代表当前 tab 的状态，
      // reachedEnd 不应据此更新，否则可能把旧 tab 的「已到底」误写到新 tab 上（审查发现的竞态）。
      if (applied && next === null) {
        setReachedEnd(true);
      }
    } catch (err) {
      console.error("[loadMore] 加载失败：", err);
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
  }, [filter, search, reachedEnd, loadPage]);

  // filter / search 变化：重置到首页（search 变化去抖 300ms，避免每次按键都打一次后端；
  // filter 切换无需去抖，0ms 立即加载，含首次挂载）。取代原先仅 [filter, loadPage] 的切 tab effect。
  useEffect(() => {
    const t = window.setTimeout(() => {
      const resetSeq = ++pageResetSeqRef.current;
      resettingPageRef.current = true;
      setReachedEnd(false);
      // 清空搜索后覆盖搜索前已经加载的窗口，而不是退回固定首屏；否则列表尾部会丢失，
      // 用户看到的原列表顺序/滚动窗口也会被搜索操作改变。
      const limit = search.trim()
        ? PAGE_SIZE
        : Math.max(PAGE_SIZE, unsearchedItemsRef.current[filter]?.length ?? 0);
      loadPage(filter, search, null, limit)
        .then(({ cursor, applied }) => {
          if (applied) {
            setInitialLoading(false);
            setLoadError(false);
            if (resetSeq === pageResetSeqRef.current) {
              resettingPageRef.current = false;
              setSwitching(false);
            }
            if (cursor === null) setReachedEnd(true);
          }
        })
        .catch(() => {
          if (resetSeq === pageResetSeqRef.current) {
            setInitialLoading(false);
            setLoadError(true);
            resettingPageRef.current = false;
            setSwitching(false);
          }
        });
    }, search ? 300 : 0);
    return () => window.clearTimeout(t);
  }, [filter, search, loadPage, retryNonce]);

  const changeSearch = useCallback((next: string) => {
    if (search.trim() !== next.trim()) {
      resettingPageRef.current = true;
      setSwitching(true);
    }
    // 后端的无搜索请求回来前就恢复原数组，既不让搜索结果继续占位，也完整保留原顺序。
    // 同时使仍在途的搜索请求失效，防止它在清空后短暂覆盖恢复出的列表。
    if (search.trim() && !next.trim()) {
      refreshSeqRef.current += 1;
      const cached = unsearchedItemsRef.current[filter];
      if (cached) {
        setItems(cached);
        // 缓存恢复即目标列表就位，无需切换灰化（B-4）：否则在途的后端请求不返回时，
        // 列表会一直冻在搜索结果上。
        setSwitching(false);
      }
    }
    setSearch(next);
  }, [filter, search]);

  // 归档/取消归档会改变当前 tab 的可见性：乐观从列表移除该卡片并调整 counts，卡片即刻消失。
  // 这里不能顺手 refresh()——refresh 是前沿触发，会与尚未落库的 set_archived 赛跑，抢先拉回旧数据
  // 把乐观更新冲掉，卡片闪一下又回来。成功路径无需自己刷：后端 set_archived 写库后会发 board-changed，
  // 届时 counts/列表被真实数据校正。失败路径由 onArchiveFailed 显式拉回。
  const onArchiveOptimistic = useCallback(
    (sessionId: number) => {
      setItems((prev) => prev.filter((l) => l.session.id !== sessionId));
      // 看板上只可能发生「归档」（已归档的不再上板），archived 计数恒 +1。
      setCounts((prev) => ({ ...prev, archived: prev.archived + 1 }));
    },
    []
  );

  // 归档失败：乐观移除的卡片必须回来，否则用户以为归档成功了。此刻后端未改动，refresh 拉到的就是真实态。
  // 置位整窗重查：常规刷新是首页 + 增量修补，捞不回被移除后深在尾部的卡（见 doRefresh）。
  const onArchiveFailed = useCallback(() => {
    fullRefreshOnceRef.current = true;
    refresh();
  }, [refresh]);

  // 撤销归档同样要整窗重查（P1-1）：被撤回的会话 last_event_at 没变、排不进首页，
  // 归档时又被乐观摘掉、不在 prev 尾部——「首页权威 + 按 id 保留」的增量刷新两条路都
  // 捞不回它，等 board-changed 的常规刷新只会让它永远缺席。
  const onArchiveUndo = useCallback(() => {
    fullRefreshOnceRef.current = true;
    refresh();
  }, [refresh]);

  // ── 灵动岛（Windows）────────────────────────────────────────────────────────
  // 2026-11-04 用户决定：Windows 贴纸整体换成顶部居中的灵动岛（替换贴边缩略条），只能左右拖；
  // 视觉照用户给的参考图（纯黑刘海、环形进度、分页面板）。三种形态：
  //   compact  收起的胶囊（额度环 · 眼睛 · 时间 · 会话图标）
  //   alert    有事时弹几秒的提醒
  //   expanded 悬停/点击展开的黑色面板（最要紧的会话 + 额度）
  // 原来还有一种「完整列表」（原贴纸），2026-11-05 用户定案删掉，会话列表看对话窗口侧栏。
  // 窗口几何由 island.rs 管，按当前中心点对称改尺寸；刘海在窗口里水平居中，窗口长大时它在
  // 屏幕上不挪位，展开动画从它的轮廓长出来。macOS 2026-11-05 起也是灵动岛，落点见 IslandHost。
  const [islandMode, setIslandMode] = useState<IslandMode>("compact");
  const islandModeRef = useRef<IslandMode>("compact");
  islandModeRef.current = islandMode;
  const [closing, setClosing] = useState(false);
  const [alert, setAlert] = useState<IslandAlertEvent | null>(null);
  const alertQueueRef = useRef<IslandAlertEvent[]>([]);
  // 队列里还排着几条（不含正在弹的那条），提醒卡片的「忽略 N 条」钮上显示（N = 这个数 + 正在弹的那条）。
  const [alertQueued, setAlertQueued] = useState(0);
  const alertTimerRef = useRef<number | null>(null);
  // 正在显示的提醒与它弹出的时刻、倒计时到点的时刻：急事插队要据此算「跑完」还该留多久，
  // 点正文要据此知道打开哪个会话（state 在事件回调里读到的是旧值，故另存 ref）。
  const shownAlertRef = useRef<{ event: IslandAlertEvent; at: number } | null>(null);
  const alertDueRef = useRef(0);
  // 胶囊（刘海本体，不含耳朵）的实测宽度。展开/收起动画以它为起止轮廓，窗口收回时按它定尺寸。
  const [pillWidth, setPillWidth] = useState(ISLAND_PILL_MIN_WIDTH);
  const pillWidthRef = useRef(ISLAND_PILL_MIN_WIDTH);
  // 面板实测高度（宽度固定）。首次展开前没量过，先按一个常见高度摆，量到后再贴合。
  const panelHeightRef = useRef(ISLAND_PANEL_HEIGHT_GUESS);
  const draggingRef = useRef(false);
  // 形态切换进行中（窗口正在改尺寸）：期间的测量不得去改窗口尺寸。展开时窗口先长大，
  // 此刻任何「按胶囊宽度摆窗口」的调用都会把它缩回去，列表被挤进胶囊大小（DEV 实拍）。
  const transitioningRef = useRef(false);
  // 收起动画的定时器。收起途中又展开（托盘找回、再点一下胶囊）要把它作废：此前展开因形态
  // 还是 expanded 直接返回，旧定时器到点照样把面板缩回胶囊（审查复现）。
  const collapseTimerRef = useRef<number | null>(null);
  useEffect(() => () => {
    if (collapseTimerRef.current != null) window.clearTimeout(collapseTimerRef.current);
  }, []);
  // 提醒倒计时的挂法定义在 grow 之后，经 ref 取（与 pumpAlertRef 同理）。
  const armAlertRef = useRef<(ms: number) => void>(() => {});
  // 设置：收起样式、哪些事弹提醒、提醒停留多久、额度环取哪家（实时跟随设置页改动）。
  const [islandCompact, setIslandCompact] = useState<IslandCompactStyle>("counts");
  const [quotaProviders, setQuotaProviders] = useState<string[]>(["claude"]);
  const islandAlertsRef = useRef<string[] | null>(null);
  const islandAlertMsRef = useRef(ISLAND_ALERT_MS_DEFAULT);
  // 轻量模式（对话功能关闭）时面板不放「打开对话窗口」，与贴纸底栏、托盘菜单同一规则。
  const [chatEnabled, setChatEnabled] = useState(true);
  // 回调里同步读（openSession 是稳定回调，闭包里的 state 会过期）。
  const chatEnabledRef = useRef(true);
  chatEnabledRef.current = chatEnabled;
  // 「打开」失败（I-1/I-2，走查 2026-11-12）：留住提醒或面板，就地显示原因和下一步。
  // 不自动消失：提醒的倒计时在点「打开」时就撤了，直到用户 ×、重试或收起。
  const [openError, setOpenError] = useState<IslandOpenError | null>(null);
  const openErrorRef = useRef<IslandOpenError | null>(null);
  openErrorRef.current = openError;
  const openingRef = useRef(false);
  // 极简模式：收起时只留顶边一条细条（用户 2026-11-05 要求）。ref 给布局回调同步读。
  const [islandStrip, setIslandStrip] = useState(false);
  const islandStripRef = useRef(false);
  islandStripRef.current = islandStrip;
  // 极简模式下鼠标停在细条上：临时变回正常胶囊，移开再缩回细条（用户 2026-11-05 要求）。
  const [stripPeek, setStripPeekState] = useState(false);
  const stripPeekRef = useRef(false);
  const setStripPeek = useCallback((peek: boolean) => {
    stripPeekRef.current = peek;
    setStripPeekState(peek);
  }, []);
  // 岛落在哪（见 platform.ts 的 IslandHost）。Windows 恒为 floating；macOS 启动时探一次，之后
  // 每几秒再探，插拔外接屏、合盖开盖后跟着换。ref 给布局回调同步读。
  const [host, setHostState] = useState<IslandHost>(FLOATING_HOST);
  const hostRef = useRef<IslandHost>(FLOATING_HOST);
  const setHost = useCallback((next: IslandHost) => {
    hostRef.current = next;
    setHostState(next);
  }, []);
  // 岛压在前台最大化窗口的标题栏上（island.rs 每 1.5s 判一次，变了才发 island-titlebar）：自动收成
  // 细条，光标靠近照样露出胶囊（关卡 A 定案，i6）。设置里可关（island_auto_strip，缺省开）。
  // 不改用户自己的极简模式开关：最大化窗口一走就恢复胶囊。
  const [titlebarCovered, setTitlebarCovered] = useState(false);
  const [autoStripEnabled, setAutoStripEnabled] = useState(true);
  const autoStrip = autoStripEnabled && titlebarCovered;
  const autoStripRef = useRef(false);
  autoStripRef.current = autoStrip;
  // 极简模式只在 Windows 有：刘海本身就是那条「细条」，菜单栏落点平时根本不显示。
  const stripOn = (islandStrip || autoStrip) && host.kind === "floating";
  // 刘海挡住的那一截（摄像头）：提醒和面板的内容要从它下面开始画。
  const safeTopOf = (h: IslandHost) => (h.kind === "notch" ? h.notchHeight : 0);
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("--isl-safe-top", `${safeTopOf(host)}px`);
    // 没刘海时面板按 macOS 菜单栏弹出面板画（styles.css 的 :root.island-menubar）。
    root.classList.toggle("island-menubar", host.kind === "menubar");
  }, [host]);
  useSettingsEffect((settings) => {
    setChatEnabled(settings.chat_enabled ?? true);
    setIslandStrip(settings.island_strip ?? false);
    setAutoStripEnabled(settings.island_auto_strip ?? true);
    if (settings.island_compact) setIslandCompact(settings.island_compact);
    if (Array.isArray(settings.sticker_quota_providers)) setQuotaProviders(settings.sticker_quota_providers);
    if (Array.isArray(settings.island_alerts)) islandAlertsRef.current = settings.island_alerts;
    if (typeof settings.island_alert_seconds === "number") {
      islandAlertMsRef.current = Math.min(30, Math.max(2, settings.island_alert_seconds)) * 1000;
    }
  });
  const { usage, meta: usageMeta } = useIslandUsage();
  // 额度按「正在用的」排：在跑/要处理的会话的 agent 在前，再按设置顺序。
  const activeProviders = useMemo(
    () => byUrgency(stripSessions).map((item) => item.provider).filter((p, i, all) => all.indexOf(p) === i),
    [stripSessions],
  );
  const islandUsages = useMemo(
    () => liveIslandUsage(usage, quotaProviders, activeProviders, usageMeta),
    [usage, quotaProviders, activeProviders, usageMeta],
  );
  // macOS 菜单栏图标：眼睛的神态 + 右边的富文本（menubar.rs / tray_anim.rs / status_title.rs）。
  // 由这里推而不是后端自己数：这里看得到所有连着的会话（含另一个 Meowo 实例开的）和额度。
  // 额度不限「正在用的」那几家：没会话在跑时也要显示（用户要空闲时也有信息）。
  const t = useT();
  const menubarRef = useRef("");
  // 菜单栏的眼睛和灵动岛上的同一个表情（useEyeMode 同一套规则，输入也和 Island.tsx 一致）。
  const menubarCounts = useMemo(() => islandCounts(stripSessions), [stripSessions]);
  const { cue: menubarCue } = useEyeCue(stripSessions);
  const menubarMood = useEyeMode({
    tone: islandTone(menubarCounts),
    cue: menubarCue,
    quotaPct: islandUsages[0]?.pct ?? null,
    busy: menubarCounts.running >= EYES_BUSY_AT,
    errors: menubarCounts.error,
  });
  useEffect(() => {
    if (!isMac()) return;
    const tone = islandTone(islandCounts(stripSessions));
    const quota = orderIslandUsage(usage, quotaProviders, activeProviders)[0];
    const segments = menubarSegments(stripSessions, tone, quota?.pct ?? null, {
      toneLabel: t.island.toneLabel,
      untitled: t.sticker.waitingFirstInput,
    });
    const key = JSON.stringify([tone, segments, menubarMood]);
    if (key === menubarRef.current) return;
    menubarRef.current = key;
    invoke("set_menubar_status", { tone, segments, mood: menubarMood }).catch(() => {});
  }, [stripSessions, usage, quotaProviders, activeProviders, t, menubarMood]);

  // monitor 只在启动还原时有；平时不带，后端缺参即按窗口当前所在屏。
  const layout = useCallback((width: number, height: number, centerRatio: number | null = null, monitor: string | null = null) =>
    invoke("island_layout", monitor ? { width, height, centerRatio, monitor } : { width, height, centerRatio }), []);
  // 菜单栏落点平时藏着，提醒、展开时摆到图标下方再显示。要等新内容画上去再显示：窗口底下的系统
  // 毛玻璃是一显示就整块出现的，先显示的话会先闪一块空玻璃（2026-11-05 Mac 录屏）。
  // 藏着的窗口里 WebView 可能停了 requestAnimationFrame，所以再挂一个 80ms 的定时器兜底，谁先到用谁。
  // 菜单栏落点收起时面板内容本来就留在窗口里（见渲染处），这里多半只是等尺寸变化画完。
  const showWhenPainted = useCallback((focus: boolean) => {
    let done = false;
    const show = () => {
      if (done) return;
      done = true;
      invoke("island_set_visible", { visible: true, focus }).catch(() => {});
    };
    window.setTimeout(show, 80);
    if (typeof window.requestAnimationFrame === "function") {
      window.requestAnimationFrame(() => window.requestAnimationFrame(show));
    }
  }, []);
  // 最近一次收起面板的时刻。菜单栏落点下点图标关面板：按下时「点了外面」先把面板收了，松手时
  // 图标的点击才到，不挡一下就又展开了（录屏里关了又弹开）。
  const lastCollapseRef = useRef(0);
  const alertHeight = () => ISLAND_ALERT_SIZE.height + safeTopOf(hostRef.current);
  const layoutCompact = useCallback(async (centerRatio: number | null = null, monitor: string | null = null) => {
    const h = hostRef.current;
    // 菜单栏落点：收起 = 藏起来，状态看菜单栏上的迷你岛。
    if (h.kind === "menubar") {
      await invoke("island_set_visible", { visible: false, focus: false });
      return;
    }
    if (h.kind === "floating" && (islandStripRef.current || autoStripRef.current) && !stripPeekRef.current) {
      await layout(ISLAND_STRIP_SIZE.width, ISLAND_STRIP_SIZE.height, centerRatio, monitor);
      return;
    }
    // 有刘海：高度就是刘海高，和菜单栏齐平。
    const height = h.kind === "notch" ? h.notchHeight : ISLAND_PILL_HEIGHT;
    await layout(pillWidthRef.current + ISLAND_EAR * 2, height, centerRatio, monitor);
  }, [layout]);

  // 极简模式开关一变（面板上点、设置页改、启动时读到设置），收起着的岛立刻换形。
  // 启动摆放（bootReady 之前）自己按设置摆，这里不抢。
  useEffect(() => {
    if (!bootReady || islandModeRef.current !== "compact" || transitioningRef.current) return;
    layoutCompact().catch(() => {});
  }, [stripOn, stripPeek, bootReady, layoutCompact]);
  // 细条撤掉（最大化窗口走了、关了极简模式）时把「临时露出」一起清掉，下次收细条从细条开始。
  useEffect(() => {
    if (!stripOn && stripPeekRef.current) setStripPeek(false);
  }, [stripOn, setStripPeek]);
  useEffect(() => {
    const un = listen<boolean>("island-titlebar", (event) => setTitlebarCovered(event.payload === true));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // 胶囊临时露出期间轮询真实光标，连续两次在窗口外才缩回细条。不用 DOM 的 pointerleave：
  // 细条长成胶囊时窗口在改尺寸，会误报一串假 leave/enter（展开态收回用轮询同一个理由）。
  useEffect(() => {
    if (!stripOn || !stripPeek || islandMode !== "compact") return;
    let outCount = 0;
    const id = window.setInterval(() => {
      if (draggingRef.current) {
        outCount = 0;
        return;
      }
      invoke<boolean>("cursor_over_window")
        .then((over) => {
          if (over) {
            outCount = 0;
            return;
          }
          outCount += 1;
          if (outCount >= 2) setStripPeek(false);
        })
        .catch(() => {});
    }, 180);
    return () => window.clearInterval(id);
  }, [stripOn, stripPeek, islandMode, setStripPeek]);

  // 面板上的极简模式按钮：先本地切（按钮立刻反映），再写回设置；写失败退回原值。
  const toggleStrip = useCallback(() => {
    const next = !islandStripRef.current;
    setIslandStrip(next);
    getSettings()
      .then((s) => setSettings({ ...s, island_strip: next }))
      .catch((err) => {
        console.error("[island] 保存极简模式失败：", err);
        setIslandStrip(!next);
      });
  }, []);

  /// 切到一个更大的形态：先把窗口放大（刘海居中，屏幕位置不动），再换内容、播动画。
  /// 顺序反过来会让内容先在小窗口里挤一帧（W-4 同一课）。
  const grow = useCallback(async (mode: "expanded", width: number, height: number) => {
    if (alertTimerRef.current != null) {
      window.clearTimeout(alertTimerRef.current);
      alertTimerRef.current = null;
    }
    transitioningRef.current = true;
    try {
      await layout(width, height);
    } catch (err) {
      transitioningRef.current = false;
      console.error("[island] 展开失败：", err);
      // 从提醒展开失败：开头清掉了它的倒计时，重新挂上，不然提醒一直挂着。
      // 从胶囊展开失败：展开途中排进来的提醒补弹（pumpAlert 在展开途中只排队不弹）。
      if (islandModeRef.current === "alert") armAlertRef.current(ISLAND_ALERT_LINGER_MS);
      else pumpAlertRef.current();
      return;
    }
    // 展开着不弹提醒：排队的那些在面板/列表里都看得到，作废。
    alertQueueRef.current = [];
    setAlertQueued(0);
    setAlert(null);
    setOpenError(null);
    // ref 先同步改：重渲染落地前到达的测量回调读到的就已经是新形态，不会去缩窗口。
    islandModeRef.current = mode;
    setIslandMode(mode);
    // macOS 的岛是非激活面板：展开时让它成为 key，Esc 和「点外面收起」（失焦）才接得到。
    if (isMac()) showWhenPainted(true);
    // 展开过就不算「临时露出」了：收起时按极简模式回到细条。
    setStripPeek(false);
    transitioningRef.current = false;
    loadStrip();
  }, [layout, loadStrip, setStripPeek, showWhenPainted]);

  const expand = useCallback(() => {
    const mode = islandModeRef.current;
    if (mode === "expanded" && collapseTimerRef.current != null) {
      // 收起动画还没播完：窗口还是面板尺寸，作废收起、停在面板上即可。
      window.clearTimeout(collapseTimerRef.current);
      collapseTimerRef.current = null;
      setClosing(false);
      return;
    }
    if (mode === "expanded" || draggingRef.current) return;
    void grow("expanded", ISLAND_PANEL_WIDTH, panelHeightRef.current);
  }, [grow]);

  // 提醒排队：一次只弹一条，弹完缩回再弹下一条。
  // 提醒到点缩回胶囊，再弹队列里的下一条。pumpAlert 经 ref 取，两者互相引用。
  const pumpAlertRef = useRef<() => void>(() => {});
  // 鼠标是否停在提醒上：停着时倒计时换成 15s 上限，急事也不插队（人在看）。
  const alertHoverRef = useRef(false);
  const finishAlert = useCallback(() => {
    alertTimerRef.current = null;
    alertHoverRef.current = false;
    if (islandModeRef.current !== "alert") return;
    setAlert(null);
    setOpenError(null);
    islandModeRef.current = "compact";
    setIslandMode("compact");
    layoutCompact()
      .catch(() => {})
      // 两条提醒之间留一拍，别让人以为是同一条换了字。
      .finally(() => window.setTimeout(() => pumpAlertRef.current(), 300));
  }, [layoutCompact]);
  const armAlertTimer = useCallback((ms: number) => {
    if (alertTimerRef.current != null) window.clearTimeout(alertTimerRef.current);
    alertDueRef.current = Date.now() + ms;
    alertTimerRef.current = window.setTimeout(finishAlert, ms);
  }, [finishAlert]);
  armAlertRef.current = armAlertTimer;
  const pumpAlert = useCallback(() => {
    // transitioningRef：正在展开（grow 在等窗口长大），形态还是 compact。这时弹提醒会把窗口
    // 改成提醒尺寸、盖掉展开的尺寸，面板被截在提醒高度里（审查复现）。只排队，展开失败再补弹。
    if (islandModeRef.current !== "compact" || alertTimerRef.current != null || transitioningRef.current) return;
    const next = alertQueueRef.current.shift();
    setAlertQueued(alertQueueRef.current.length);
    if (!next) return;
    shownAlertRef.current = { event: next, at: Date.now() };
    alertHoverRef.current = false;
    setAlert(next);
    islandModeRef.current = "alert";
    setIslandMode("alert");
    layout(ISLAND_ALERT_SIZE.width, alertHeight())
      .then(() => { if (hostRef.current.kind === "menubar") showWhenPainted(false); })
      .catch((err) => console.error("[island] 弹出提醒失败：", err));
    armAlertTimer(islandAlertMsRef.current);
  }, [layout, armAlertTimer, showWhenPainted]);
  pumpAlertRef.current = pumpAlert;
  // 鼠标停在提醒上：倒计时换成 ISLAND_ALERT_HOVER_MAX_MS 上限，移开后再留 ISLAND_ALERT_LINGER_MS 给人去点。
  // 此前停上去会触发「悬停展开」，提醒被换成面板，还没看完就没了（用户反馈）；之后改成无限暂停，
  // 又成了「挡在标题栏上赶不走」（i5），所以设上限。
  const onAlertHover = useCallback((hovering: boolean) => {
    if (islandModeRef.current !== "alert") return;
    alertHoverRef.current = hovering;
    // 打开失败的提醒不再有倒计时（含原因和按钮的提示不在读完前消失，impl 准则 2026-11-12）。
    if (openErrorRef.current) return;
    armAlertTimer(hovering ? ISLAND_ALERT_HOVER_MAX_MS : ISLAND_ALERT_LINGER_MS);
  }, [armAlertTimer]);
  // 提醒上的 ×：只关这一条，排队的照常接着弹（i5）。先撤倒计时：不撤的话旧定时器到点会把下一条提前收掉。
  const dismissAlert = useCallback(() => {
    if (islandModeRef.current !== "alert") return;
    if (alertTimerRef.current != null) window.clearTimeout(alertTimerRef.current);
    finishAlert();
  }, [finishAlert]);
  // 「全部忽略」：清空队列再关这一条。
  const dismissAllAlerts = useCallback(() => {
    alertQueueRef.current = [];
    setAlertQueued(0);
    dismissAlert();
  }, [dismissAlert]);

  useEffect(() => {
    const un = listen<IslandAlertEvent>("island-alert", (event) => {
      const mode = islandModeRef.current;
      if (mode === "expanded") return;
      // 设置还没读到时（启动头一两秒）按全开处理，宁可多弹一次也别漏掉要人处理的事。
      const enabled = islandAlertsRef.current;
      if (enabled && !enabled.includes(islandAlertSwitch(event.payload.kind))) return;
      // 同一会话排队中的旧提醒换成新的：一个会话接连变状态时只弹最新的那条，不连弹好几次。
      const rest = alertQueueRef.current.filter((queued) => queued.sessionId !== event.payload.sessionId);
      const urgent = isUrgentAlert(event.payload.kind);
      // 急事排在已有急事之后、所有「跑完」之前：急事之间仍按先来后到。
      const at = urgent ? rest.findIndex((queued) => !isUrgentAlert(queued.kind)) : -1;
      alertQueueRef.current = at < 0 ? [...rest, event.payload] : [...rest.slice(0, at), event.payload, ...rest.slice(at)];
      setAlertQueued(alertQueueRef.current.length);
      // 正在显示「跑完」时让它提前让位。鼠标停在上面（倒计时已暂停）说明人在看，不打断。
      const shown = shownAlertRef.current;
      if (urgent && islandModeRef.current === "alert" && alertTimerRef.current != null && !alertHoverRef.current && shown && !isUrgentAlert(shown.event.kind)) {
        const wait = Math.max(0, ISLAND_ALERT_YIELD_MS - (Date.now() - shown.at));
        if (Date.now() + wait < alertDueRef.current) armAlertTimer(wait);
      }
      pumpAlert();
    });
    return () => {
      un.then((f) => f());
      if (alertTimerRef.current != null) window.clearTimeout(alertTimerRef.current);
    };
  }, [pumpAlert, armAlertTimer]);

  const collapse = useCallback(() => {
    const mode = islandModeRef.current;
    if (mode !== "expanded") return;
    lastCollapseRef.current = Date.now();
    setOpenError(null);
    // 菜单栏落点：直接藏起窗口，不播收回动画。系统毛玻璃不跟着内容淡出，播动画就会剩一块空玻璃。
    if (hostRef.current.kind === "menubar") {
      islandModeRef.current = "compact";
      setIslandMode("compact");
      layoutCompact()
        .catch((err) => console.error("[island] 收起失败：", err))
        .finally(() => pumpAlert());
      return;
    }
    if (collapseTimerRef.current != null) return;
    setClosing(true);
    // 先播收回动画（缩回胶囊轮廓），播完再把窗口缩小。
    collapseTimerRef.current = window.setTimeout(() => {
      collapseTimerRef.current = null;
      if (islandModeRef.current !== "expanded") return;
      setClosing(false);
      islandModeRef.current = "compact";
      setIslandMode("compact");
      layoutCompact()
        .catch((err) => console.error("[island] 收起失败：", err))
        .finally(() => pumpAlert());
    }, ISLAND_LEAVE_MS);
  }, [layoutCompact, pumpAlert]);

  // 面板标题行的动作（新建会话 / 打开对话窗 / 设置）：执行后收起面板。收起动画进行中（或菜单栏落点已经
  // 收起）再点直接忽略：collapse 自己防住了重复收起，但动作本身会再执行一次，双击「新建会话」就开两次窗口，
  // 后端两个线程还会抢着建同名窗口（审查 2026-11-09，从 chore/remove-edge-snap 移植）。
  const panelAction = useCallback((run: () => void) => {
    if (islandModeRef.current !== "expanded" || collapseTimerRef.current != null) return;
    run();
    collapse();
  }, [collapse]);

  const onMeasure = useCallback((width: number) => {
    const next = Math.max(ISLAND_PILL_MIN_WIDTH, width);
    // 宽度没变就不动窗口：测量回调在窗口改尺寸、字体加载等时候都会触发，多数时候宽度根本没变。
    if (next === pillWidthRef.current) return;
    pillWidthRef.current = next;
    setPillWidth(next);
    if (islandModeRef.current === "compact" && !transitioningRef.current) layoutCompact().catch(() => {});
  }, [layoutCompact]);

  const onPanelMeasure = useCallback((height: number) => {
    if (height <= 0 || height === panelHeightRef.current) return;
    panelHeightRef.current = height;
    if (islandModeRef.current === "expanded" && !transitioningRef.current) {
      layout(ISLAND_PANEL_WIDTH, height).catch(() => {});
    }
  }, [layout]);

  // 左右拖：按下后横向挪过 3px 才算拖动，否则松手当一次点击。位置由后端按真实光标算
  // （island.rs），只动横向、始终贴顶；松手把中心比例记进 settings。
  const startDrag = useCallback((target: HTMLElement, pointerId: number, startX: number, onClick: () => void) => {
    try { target.setPointerCapture(pointerId); } catch { /* 测试环境无 pointer capture */ }
    let dragging = false;
    let frame = 0;
    const move = (event: PointerEvent) => {
      // macOS 的岛钉在刘海上或菜单栏图标下，不能拖：按下松开一律当点击。
      if (isMac()) return;
      if (!dragging && Math.abs(event.screenX - startX) > 3) {
        dragging = true;
        draggingRef.current = true;
        invoke("island_drag_begin").catch(() => {});
      }
      if (dragging && !frame) {
        frame = window.requestAnimationFrame(() => {
          frame = 0;
          invoke("island_drag_update").catch(() => {});
        });
      }
    };
    // pointercancel（失去捕获、触控手势被系统接管）不是用户松手：没拖起来就什么都不做，
    // 此前同走 finish 被当成一次点击，岛自己展开或打开了对话窗（审查复现）。
    const finish = (event: PointerEvent) => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", finish);
      target.removeEventListener("pointercancel", finish);
      if (frame) window.cancelAnimationFrame(frame);
      if (!dragging) {
        if (event.type !== "pointercancel") onClick();
        return;
      }
      // 记下中心比例和所在显示器：只记比例的话，拖到副屏的岛重启后按同一比例落回主屏（审查发现）。
      invoke<{ ratio: number; monitor: string | null } | null>("island_drag_end")
        .then((spot) => {
          if (spot && typeof spot.ratio === "number") updateStickerWindowState({ island_center: spot.ratio, island_monitor: spot.monitor ?? null });
        })
        .catch(() => {})
        .finally(() => { draggingRef.current = false; });
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", finish);
    target.addEventListener("pointercancel", finish);
  }, []);

  // 点胶囊上的会话图标（没拖动）直接打开那个会话；点别处展开面板。openSession 定义在后面，经 ref 取。
  const openSessionRef = useRef<(sessionId: number) => void>(() => {});
  const onPillPointerDown = useCallback((event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    const hit = (event.target as HTMLElement | null)?.closest?.("[data-session-id]");
    // 提醒在场时点正文 = 打开那条会话（与系统通知横幅一致，二期定）；只有空闲胶囊点了才展开面板。
    const alerting = islandModeRef.current === "alert" ? shownAlertRef.current?.event.sessionId : undefined;
    const sessionId = hit ? Number(hit.getAttribute("data-session-id")) : alerting ?? NaN;
    const onClick = Number.isFinite(sessionId) ? () => openSessionRef.current(sessionId) : expand;
    startDrag(event.currentTarget, event.pointerId, event.screenX, onClick);
  }, [startDrag, expand]);

  const onPanelHeadPointerDown = useCallback((event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    startDrag(event.currentTarget, event.pointerId, event.screenX, () => {});
  }, [startDrag]);

  // 托盘找回（轻量模式下单击托盘、通知定位失败的兜底）：Windows 贴纸已删（2026-11-05 用户
  // 定案），岛本来就在顶上，找回就是展开面板。
  useEffect(() => {
    const un = listen("recall-sticker", () => expand());
    return () => {
      un.then((f) => f());
    };
  }, [expand]);

  // macOS 没刘海时，菜单栏图标就是迷你岛：点它展开 / 收起面板（menubar.rs 的 on_left_click）。
  useEffect(() => {
    const un = listen("island-tray-toggle", () => {
      if (islandModeRef.current === "expanded") collapse();
      // 刚因为按下图标（算「点了外面」）收起的，这次点击是来关面板的，不再展开。
      else if (Date.now() - lastCollapseRef.current > 500) expand();
    });
    return () => {
      un.then((f) => f());
    };
  }, [expand, collapse]);

  // macOS：几秒问一次落点。插上外接屏、合盖、开盖后，有没有刘海会变；变了就收起回胶囊、
  // 按新落点重摆（菜单栏落点下是藏起来）。
  useEffect(() => {
    if (!isMac()) return;
    const id = window.setInterval(() => {
      invoke<IslandHost>("island_host")
        .then((next) => {
          const cur = hostRef.current;
          if (next.kind === cur.kind && next.notchWidth === cur.notchWidth && next.notchHeight === cur.notchHeight) return;
          setHost(next);
          if (alertTimerRef.current != null) {
            window.clearTimeout(alertTimerRef.current);
            alertTimerRef.current = null;
          }
          setAlert(null);
          islandModeRef.current = "compact";
          setIslandMode("compact");
          layoutCompact()
            .then(() => (next.kind === "notch" ? invoke("island_set_visible", { visible: true, focus: false }) : undefined))
            .catch(() => {});
        })
        .catch(() => {});
    }, 3000);
    return () => window.clearInterval(id);
  }, [setHost, layoutCompact]);

  // 换显示器缩放（拖到另一块屏、系统改缩放）：按当前形态的逻辑尺寸重摆一次。
  useEffect(() => {
    let un: (() => void) | undefined;
    try {
      getCurrentWindow()
        .onScaleChanged(() => {
          const mode = islandModeRef.current;
          if (mode === "expanded") layout(ISLAND_PANEL_WIDTH, panelHeightRef.current).catch(() => {});
          else if (mode === "alert") layout(ISLAND_ALERT_SIZE.width, alertHeight()).catch(() => {});
          else layoutCompact().catch(() => {});
        })
        .then((f) => { un = f; })
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（测试/浏览器）忽略 */
    }
    return () => un?.();
  }, [layout, layoutCompact]);

  // 启动：读 settings 的位置与置顶偏好，按记住的横向位置把胶囊贴顶摆好再显示（bootReady 闸门，
  // 几何落地前不渲染、不显示，W-4/W-17）。macOS 先探落点（有没有刘海）再摆。
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const s = await initStickerWindowState();
        // 先读极简模式再摆：否则开机先摆成胶囊、设置到了再缩成细条，闪一下。
        const settings = await getSettings().catch(() => null);
        if (cancelled) return;
        if (settings?.island_strip) {
          islandStripRef.current = true;
          setIslandStrip(true);
        }
        if (isMac()) {
          const found = await invoke<IslandHost>("island_host").catch(() => null);
          if (found) setHost(found);
        }
        if (cancelled) return;
        applyPinned(s.pinned);
        await layoutCompact(s.island_center ?? 0.5, s.island_monitor);
        // 刘海落点摆好就显示（不抢键盘）；菜单栏落点平时藏着，等点图标。
        if (hostRef.current.kind === "notch") await invoke("island_set_visible", { visible: true, focus: false });
      } catch (err) {
        console.error("[island] 启动摆放失败：", err);
      } finally {
        // 成功失败都放行显示：失败时窗口留在默认几何，藏着不显示只会更糟。
        if (!cancelled) setBootReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
    // 仅启动跑一次
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 启动流程只跑一次
  }, []);

  // 展开态收回：点岛外面才收，光标移开不收（用户 2026-11-05 要求；原来光标出窗口就收，
  // 想挪去点别处的东西面板先没了）。点到别的窗口 → 岛失焦，靠 blur；点桌面空白、任务栏
  // 这类岛可能从没拿过焦点的地方，blur 不来，再轮询全局「鼠标按着且在窗口外」兜底。
  // 60ms 一拍：一次普通点击按下约 100ms，间隔再长会漏。
  useEffect(() => {
    if (islandMode !== "expanded" || closing) return;
    const onBlur = () => {
      if (!draggingRef.current) collapse();
    };
    window.addEventListener("blur", onBlur);
    let busy = false;
    const id = window.setInterval(() => {
      if (draggingRef.current || busy) return;
      busy = true;
      invoke<boolean>("mouse_pressed_outside_window")
        .then((outside) => {
          if (outside) collapse();
        })
        .catch(() => {})
        .finally(() => {
          busy = false;
        });
    }, 60);
    return () => {
      window.removeEventListener("blur", onBlur);
      window.clearInterval(id);
    };
  }, [islandMode, closing, collapse]);

  // 展开态的键盘出口：Esc 收回（P2-12 同一课）。注册 Esc 层，展开的面板本身就是一层浮层。
  useEffect(() => {
    if (islandMode !== "expanded") return;
    const popLayer = pushEscLayer();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const active = document.activeElement;
      if (active instanceof HTMLElement && (active.tagName === "INPUT" || active.tagName === "TEXTAREA")) return;
      e.preventDefault();
      collapse();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey);
    };
  }, [islandMode, collapse]);

  // 打开某个会话的对话窗：提醒里点「打开」后缩回胶囊；面板里点会话后收起面板。
  // 等开窗结果再收（I-1，走查 2026-11-12）：此前先收再说，失败时提醒已经没了，用户点了毫无反应。
  const openSession = useCallback(async (sessionId: number) => {
    if (openingRef.current) return;
    openingRef.current = true;
    // 点了「打开」是明确意图：先撤提醒倒计时，免得等结果时提醒自己收走，错误无处可报。
    if (islandModeRef.current === "alert" && alertTimerRef.current != null) {
      window.clearTimeout(alertTimerRef.current);
      alertTimerRef.current = null;
    }
    setOpenError(null);
    const failure = await openIslandSession({
      sessionId,
      chatEnabled: chatEnabledRef.current,
      item: stripSessionsRef.current.find((entry) => entry.session.id === sessionId),
      locale: t.locale,
    }).finally(() => { openingRef.current = false; });
    const mode = islandModeRef.current;
    if (failure) {
      if (mode === "expanded" || mode === "alert") setOpenError(failure);
      return;
    }
    if (mode === "expanded") {
      collapse();
      return;
    }
    if (alertTimerRef.current != null) {
      window.clearTimeout(alertTimerRef.current);
      alertTimerRef.current = null;
    }
    setAlert(null);
    islandModeRef.current = "compact";
    setIslandMode("compact");
    layoutCompact().catch(() => {}).finally(() => pumpAlert());
  }, [collapse, layoutCompact, pumpAlert, t.locale]);
  // 轻量模式失败时的「打开设置」：设置里有「对话窗口功能」开关。
  const openSettingsFromError = useCallback(() => {
    setOpenError(null);
    invoke("open_settings").catch((err) => console.error("[island] 打开设置失败：", err));
    if (islandModeRef.current === "expanded") collapse();
    else if (islandModeRef.current === "alert") finishAlert();
  }, [collapse, finishAlert]);
  openSessionRef.current = openSession;

  if (!bootReady) {
    return null;
  }
  // 菜单栏落点收起时窗口是藏着的，但里面照样留着面板：下次点图标直接显示，不用等重新渲染
  // （藏着的 WebView 可能不画，先显示再画就会闪一块空玻璃）。
  if (islandMode === "expanded" || (host.kind === "menubar" && islandMode === "compact")) {
    return (
      <div
        className={`island-shell${closing ? " is-leaving" : " is-entering"}`}
        // 收回动画缩向收起后的形状：极简模式缩成细条，否则缩回胶囊。
        style={(stripOn
          ? { "--island-pill-w": `${ISLAND_STRIP_SIZE.width}px`, "--island-pill-h": `${ISLAND_STRIP_SIZE.height}px` }
          : {
            "--island-pill-w": `${pillWidth}px`,
            "--island-pill-h": `${host.kind === "notch" ? host.notchHeight : ISLAND_PILL_HEIGHT}px`,
          }) as React.CSSProperties}
      >
        <IslandPanel
          sessions={stripSessions}
          usage={usage}
          usageMeta={usageMeta}
          quotaProviders={quotaProviders}
          onOpenSession={openSession}
          openError={openError}
          focusOnOpen={islandMode === "expanded"}
          onRetryOpen={(id) => void openSession(id)}
          onOpenSettingsFromError={openSettingsFromError}
          onDismissOpenError={() => setOpenError(null)}
          onMeasure={onPanelMeasure}
          onPointerDownDrag={onPanelHeadPointerDown}
          onNewSession={() => panelAction(() => {
            openNewSessionWindow().catch((err) => console.error("[island] 打开新建会话失败：", err));
          })}
          onOpenChat={chatEnabled ? () => panelAction(() => {
            invoke("open_latest_chat").catch((err) => console.error("[island] 打开对话窗口失败：", err));
          }) : undefined}
          onOpenSettings={() => panelAction(() => {
            invoke("open_settings").catch((err) => console.error("[island] 打开设置失败：", err));
          })}
          // 按钮只反映用户自己的极简模式开关；最大化窗口触发的自动细条不算（不然点一下「退出」反而打开了它）。
          strip={islandStrip && host.kind === "floating"}
          onToggleStrip={host.kind === "floating" ? toggleStrip : undefined}
        />
      </div>
    );
  }
  if (islandMode === "compact" && stripOn && !stripPeek) {
    return (
      <IslandStrip
        sessions={stripSessions}
        onPointerDownDrag={onPillPointerDown}
        onActivate={expand}
        onHover={() => setStripPeek(true)}
      />
    );
  }
  return (
    <Island
      sessions={stripSessions}
      compactStyle={islandCompact}
      alert={islandMode === "alert" ? alert : null}
      alertQueued={alertQueued}
      usages={islandUsages}
      onMeasure={onMeasure}
      onActivate={expand}
      onOpenSession={openSession}
      openError={openError}
      onOpenSettingsFromError={openSettingsFromError}
      onPointerDownDrag={onPillPointerDown}
      onAlertHover={onAlertHover}
      onDismissAlert={dismissAlert}
      onDismissAllAlerts={dismissAllAlerts}
      notch={host.kind === "notch" ? { width: host.notchWidth, height: host.notchHeight } : undefined}
    />
  );
}
