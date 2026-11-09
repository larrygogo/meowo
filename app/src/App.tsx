import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  getLiveSessionsCounts,
  getLiveSessionsPage,
  openNewSessionWindow,
  LiveSession,
  LiveSessionCounts,
  PageCursor,

} from "./api";
import { Sticker } from "./views/Sticker";
import { useBoardRefresh } from "./hooks/useBoardRefresh";
import { useSettingsEffect } from "./hooks/useSettings";
import type { IslandCompactStyle } from "./api";
import { type Item, type Tab } from "./views/sticker/types";
import { Island, ISLAND_ALERT_SIZE, ISLAND_EAR, ISLAND_PILL_HEIGHT } from "./views/Island";
import { IslandPanel } from "./views/island/IslandPanel";
import { liveIslandUsage, useIslandUsage } from "./views/island/useIslandUsage";
import { byUrgency } from "./views/Island";
import type { IslandAlertEvent } from "./generated/contracts/IslandAlertEvent";
import { useUpdate } from "./useUpdate";
import { useShowWhenReady } from "./useShowWhenReady";
import { isMacPanel } from "./platform";
import { reconcileRows, sameRefs, type RowCache as SharedRowCache } from "./rowCache";
import { reconcilePendings, type PendingRegistry } from "./views/sticker/pending";
import { pushEscLayer } from "./escLayers";
import {
  initStickerWindowState,
  normalSize,
  updateStickerWindowState,
} from "./windowState";

type IslandMode = "compact" | "alert" | "expanded" | "full";
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
/// 鼠标从提醒上移开后，提醒再留多久才缩回。
const ISLAND_ALERT_LINGER_MS = 3_000;
/// 收回动画时长，与 styles.css 的 .island-shell.is-leaving 同源。
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
  const [bootReady, setBootReady] = useState(isMacPanel());
  // 贴纸窗口以 visible:false 创建（tauri.conf.json），首帧渲染后再显示，消除启动瞬间的白框闪烁。
  // 不抢焦点（窗口配置 focus:false，开机自启同理）；macOS 面板模式显隐归 menubar 管，这里不越权。
  useShowWhenReady({ focus: false, enabled: !isMacPanel(), ready: bootReady });
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
    // macOS 面板模式没有吸边/折叠，缩略条永不渲染，这 2×200 条的查询是纯浪费。
    if (isMacPanel()) return;
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
    if (!isMacPanel()) loadStrip();
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
  // 视觉照用户给的参考图（纯黑刘海、环形进度、分页面板）。四种形态：
  //   compact  收起的胶囊（额度环 · 眼睛 · 时间 · 会话图标）
  //   alert    有事时弹几秒的提醒
  //   expanded 悬停/点击展开的黑色面板（最要紧的会话 + 额度）
  //   full     「全部」/ 托盘找回打开的完整列表（原贴纸）
  // 窗口几何由 island.rs 管，按当前中心点对称改尺寸；刘海在窗口里水平居中，窗口长大时它在
  // 屏幕上不挪位，展开动画从它的轮廓长出来。macOS 仍是菜单栏面板，下面每段都先判 isMacPanel()。
  const [islandMode, setIslandMode] = useState<IslandMode>("compact");
  const islandModeRef = useRef<IslandMode>("compact");
  islandModeRef.current = islandMode;
  const [closing, setClosing] = useState(false);
  const [alert, setAlert] = useState<IslandAlertEvent | null>(null);
  const alertQueueRef = useRef<IslandAlertEvent[]>([]);
  // 队列里还排着几条（不含正在弹的那条），提醒卡片上显示「还有 N 条」。
  const [alertQueued, setAlertQueued] = useState(0);
  const alertTimerRef = useRef<number | null>(null);
  // 胶囊（刘海本体，不含耳朵）的实测宽度。展开/收起动画以它为起止轮廓，窗口收回时按它定尺寸。
  const [pillWidth, setPillWidth] = useState(ISLAND_PILL_MIN_WIDTH);
  const pillWidthRef = useRef(ISLAND_PILL_MIN_WIDTH);
  // 面板实测高度（宽度固定）。首次展开前没量过，先按一个常见高度摆，量到后再贴合。
  const panelHeightRef = useRef(ISLAND_PANEL_HEIGHT_GUESS);
  const draggingRef = useRef(false);
  // 形态切换进行中（窗口正在改尺寸）：期间的测量不得去改窗口尺寸。展开时窗口先长大，
  // 此刻任何「按胶囊宽度摆窗口」的调用都会把它缩回去，列表被挤进胶囊大小（DEV 实拍）。
  const transitioningRef = useRef(false);
  // 收起动画的定时器。展开/找回（grow）要把它作废：收起途中点托盘「找回」，旧定时器
  // 到点会把刚展开的完整列表又缩回胶囊（审查复现）。同时防 Esc 与光标离开各排一个。
  const collapseTimerRef = useRef<number | null>(null);
  // 提醒倒计时的挂法定义在 grow 之后，经 ref 取（与 pumpAlertRef 同理）。
  const armAlertRef = useRef<(ms: number) => void>(() => {});
  useEffect(() => () => {
    if (collapseTimerRef.current != null) window.clearTimeout(collapseTimerRef.current);
  }, []);
  // 设置：收起样式、哪些事弹提醒、提醒停留多久、额度环取哪家（实时跟随设置页改动）。
  const [islandCompact, setIslandCompact] = useState<IslandCompactStyle>("counts");
  const [quotaProviders, setQuotaProviders] = useState<string[]>(["claude"]);
  const islandAlertsRef = useRef<string[] | null>(null);
  const islandAlertMsRef = useRef(ISLAND_ALERT_MS_DEFAULT);
  useSettingsEffect((settings) => {
    if (settings.island_compact) setIslandCompact(settings.island_compact);
    if (Array.isArray(settings.sticker_quota_providers)) setQuotaProviders(settings.sticker_quota_providers);
    if (Array.isArray(settings.island_alerts)) islandAlertsRef.current = settings.island_alerts;
    if (typeof settings.island_alert_seconds === "number") {
      islandAlertMsRef.current = Math.min(30, Math.max(2, settings.island_alert_seconds)) * 1000;
    }
  });
  // 完整列表里贴纸底栏自己取额度，岛这份先停：两边同时每 5 分钟各刷一遍（审查发现）。
  const usage = useIslandUsage(isMacPanel() || islandMode !== "full");
  // 额度按「正在用的」排：在跑/要处理的会话的 agent 在前，再按设置顺序。
  const activeProviders = useMemo(
    () => byUrgency(stripSessions).map((item) => item.provider).filter((p, i, all) => all.indexOf(p) === i),
    [stripSessions],
  );
  const islandUsages = useMemo(
    () => liveIslandUsage(usage, quotaProviders, activeProviders),
    [usage, quotaProviders, activeProviders],
  );

  // 收起即卸载 Sticker，searchOpen（Sticker 局部态）随之丢失；若搜索词留在这里，
  // 再打开完整列表时仍被后端过滤、底栏却没有搜索框，空结果会被误读成「会话全丢了」。离开
  // 完整列表时清空，走 changeSearch 的完整清空路径（恢复搜索前列表 + 作废在途搜索请求）。
  useEffect(() => {
    if (!isMacPanel() && islandMode !== "full" && search) changeSearch("");
  }, [islandMode, search, changeSearch]);

  const layout = useCallback((width: number, height: number, centerRatio: number | null = null, monitor: string | null = null) =>
    // monitor 只在启动还原时有；平时不带，后端缺参即按窗口当前所在屏。
    invoke("island_layout", monitor ? { width, height, centerRatio, monitor } : { width, height, centerRatio }), []);
  const layoutCompact = useCallback((centerRatio: number | null = null, monitor: string | null = null) =>
    layout(pillWidthRef.current + ISLAND_EAR * 2, ISLAND_PILL_HEIGHT, centerRatio, monitor), [layout]);

  /// 切到一个更大的形态：先把窗口放大（刘海居中，屏幕位置不动），再换内容、播动画。
  /// 顺序反过来会让内容先在小窗口里挤一帧（W-4 同一课）。
  const grow = useCallback(async (mode: "expanded" | "full", width: number, height: number) => {
    if (collapseTimerRef.current != null) {
      window.clearTimeout(collapseTimerRef.current);
      collapseTimerRef.current = null;
      setClosing(false);
    }
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
      // 从提醒展开失败：grow 开头清掉了它的倒计时，重新挂上，不然提醒一直挂着（做空推演）。
      // 从胶囊展开失败：展开途中排进来的提醒补弹。
      if (islandModeRef.current === "alert") armAlertRef.current(ISLAND_ALERT_LINGER_MS);
      else pumpAlertRef.current();
      return;
    }
    // 展开着不弹提醒：排队的那些在面板/列表里都看得到，作废。
    alertQueueRef.current = [];
    setAlertQueued(0);
    setAlert(null);
    // ref 先同步改：重渲染落地前到达的测量回调读到的就已经是新形态，不会去缩窗口。
    islandModeRef.current = mode;
    setIslandMode(mode);
    transitioningRef.current = false;
    loadStrip();
  }, [layout, loadStrip]);

  const expand = useCallback(() => {
    const mode = islandModeRef.current;
    if (isMacPanel() || mode === "expanded" || mode === "full" || draggingRef.current) return;
    void grow("expanded", ISLAND_PANEL_WIDTH, panelHeightRef.current);
  }, [grow]);

  const showAll = useCallback(() => {
    if (isMacPanel() || islandModeRef.current === "full") return;
    const { w, h } = normalSize();
    void grow("full", w, h);
  }, [grow]);

  // 提醒排队：一次只弹一条，弹完缩回再弹下一条。
  // 提醒到点缩回胶囊，再弹队列里的下一条。pumpAlert 经 ref 取，两者互相引用。
  const pumpAlertRef = useRef<() => void>(() => {});
  const finishAlert = useCallback(() => {
    alertTimerRef.current = null;
    if (islandModeRef.current !== "alert") return;
    setAlert(null);
    islandModeRef.current = "compact";
    setIslandMode("compact");
    layoutCompact()
      .catch(() => {})
      // 两条提醒之间留一拍，别让人以为是同一条换了字。
      .finally(() => window.setTimeout(() => pumpAlertRef.current(), 300));
  }, [layoutCompact]);
  const armAlertTimer = useCallback((ms: number) => {
    if (alertTimerRef.current != null) window.clearTimeout(alertTimerRef.current);
    alertTimerRef.current = window.setTimeout(finishAlert, ms);
  }, [finishAlert]);
  armAlertRef.current = armAlertTimer;
  const pumpAlert = useCallback(() => {
    // transitioningRef：正在展开（grow 在等窗口长大），形态还是 compact。这时弹提醒会把
    // 窗口改成提醒尺寸、盖掉展开的尺寸，面板被截在 68px 高的窗口里（审查复现）。
    if (islandModeRef.current !== "compact" || alertTimerRef.current != null || transitioningRef.current) return;
    const next = alertQueueRef.current.shift();
    setAlertQueued(alertQueueRef.current.length);
    if (!next) return;
    setAlert(next);
    islandModeRef.current = "alert";
    setIslandMode("alert");
    layout(ISLAND_ALERT_SIZE.width, ISLAND_ALERT_SIZE.height).catch((err) => console.error("[island] 弹出提醒失败：", err));
    armAlertTimer(islandAlertMsRef.current);
  }, [layout, armAlertTimer]);
  pumpAlertRef.current = pumpAlert;
  // 鼠标停在提醒上：倒计时暂停，提醒一直留着；移开后再留 ISLAND_ALERT_LINGER_MS 给人去点。
  // 此前停上去会触发「悬停展开」，提醒被换成面板，还没看完就没了（用户反馈）。
  const onAlertHover = useCallback((hovering: boolean) => {
    if (islandModeRef.current !== "alert") return;
    if (hovering) {
      if (alertTimerRef.current != null) window.clearTimeout(alertTimerRef.current);
      alertTimerRef.current = null;
    } else {
      armAlertTimer(ISLAND_ALERT_LINGER_MS);
    }
  }, [armAlertTimer]);

  useEffect(() => {
    if (isMacPanel()) return;
    const un = listen<IslandAlertEvent>("island-alert", (event) => {
      const mode = islandModeRef.current;
      // 正在展开（transitioning）时照样排进队列，只是先不弹（pumpAlert 会拦）：展开成功会清队，
      // 展开失败回到胶囊后补弹，不能就此丢掉（做空复现）。
      if (mode === "expanded" || mode === "full") return;
      // 设置还没读到时（启动头一两秒）按全开处理，宁可多弹一次也别漏掉要人处理的事。
      const enabled = islandAlertsRef.current;
      if (enabled && !enabled.includes(islandAlertSwitch(event.payload.kind))) return;
      // 同一会话排队中的旧提醒换成新的：一个会话接连变状态时只弹最新的那条，不连弹好几次。
      alertQueueRef.current = [
        ...alertQueueRef.current.filter((queued) => queued.sessionId !== event.payload.sessionId),
        event.payload,
      ];
      setAlertQueued(alertQueueRef.current.length);
      pumpAlert();
    });
    return () => {
      un.then((f) => f());
      if (alertTimerRef.current != null) window.clearTimeout(alertTimerRef.current);
    };
  }, [pumpAlert]);

  const collapse = useCallback(() => {
    const mode = islandModeRef.current;
    if (isMacPanel() || (mode !== "expanded" && mode !== "full") || collapseTimerRef.current != null) return;
    setClosing(true);
    // 先播收回动画（缩回胶囊轮廓），播完再把窗口缩小。
    collapseTimerRef.current = window.setTimeout(() => {
      collapseTimerRef.current = null;
      if (islandModeRef.current !== mode) return;
      setClosing(false);
      islandModeRef.current = "compact";
      setIslandMode("compact");
      layoutCompact()
        .catch((err) => console.error("[island] 收起失败：", err))
        .finally(() => pumpAlert());
    }, ISLAND_LEAVE_MS);
  }, [layoutCompact, pumpAlert]);

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
    const sessionId = hit ? Number(hit.getAttribute("data-session-id")) : NaN;
    const onClick = Number.isFinite(sessionId) ? () => openSessionRef.current(sessionId) : expand;
    startDrag(event.currentTarget, event.pointerId, event.screenX, onClick);
  }, [startDrag, expand]);

  const onPanelHeadPointerDown = useCallback((event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    startDrag(event.currentTarget, event.pointerId, event.screenX, () => {});
  }, [startDrag]);

  // 完整列表顶部原本是系统拖拽区（data-tauri-drag-region），那会连上下一起拖、还会
  // 双击最大化。岛模式在 capture 阶段截下它，改走同一套左右拖。
  useEffect(() => {
    if (isMacPanel()) return;
    const isDragRegion = (target: EventTarget | null) =>
      target instanceof HTMLElement && target.closest("[data-tauri-drag-region]") != null;
    const onMouseDown = (event: MouseEvent) => {
      if (!isDragRegion(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (event.button !== 0 || !isDragRegion(event.target)) return;
      startDrag(event.target as HTMLElement, event.pointerId, event.screenX, () => {});
    };
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [startDrag]);

  // 托盘「找回贴纸」：岛本来就在顶上，找回就是打开完整列表。
  useEffect(() => {
    if (isMacPanel()) return;
    // 已经是完整列表时 showAll 直接返回：仍要按当前尺寸重摆一次（贴顶 + 置顶），
    // 托盘点了要有反应（审查复现：只剩后端那一步时窗口落在主屏正中、还沉到别的窗口下面）。
    const un = listen("recall-sticker", () => {
      if (!isMacPanel() && islandModeRef.current === "full") {
        // 完整列表正在收起：作废收起定时器，不然它到点照样缩回胶囊（做空复现）。
        if (collapseTimerRef.current != null) {
          window.clearTimeout(collapseTimerRef.current);
          collapseTimerRef.current = null;
          setClosing(false);
        }
        const { w, h } = normalSize();
        layout(w, h).catch(() => {});
        return;
      }
      showAll();
    });
    return () => {
      un.then((f) => f());
    };
  }, [showAll, layout]);

  // 换显示器缩放（拖到另一块屏、系统改缩放）：按当前形态的逻辑尺寸重摆一次。
  useEffect(() => {
    if (isMacPanel()) return;
    let un: (() => void) | undefined;
    try {
      getCurrentWindow()
        .onScaleChanged(() => {
          const mode = islandModeRef.current;
          const { w, h } = normalSize();
          if (mode === "full") layout(w, h).catch(() => {});
          else if (mode === "expanded") layout(ISLAND_PANEL_WIDTH, panelHeightRef.current).catch(() => {});
          else if (mode === "alert") layout(ISLAND_ALERT_SIZE.width, ISLAND_ALERT_SIZE.height).catch(() => {});
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
  // 几何落地前不渲染、不显示，W-4/W-17）。macOS 面板只取 pin 偏好，异步套用不挡首帧。
  useEffect(() => {
    if (isMacPanel()) {
      initStickerWindowState()
        .then((s) => applyPinned(s.pinned))
        .catch(() => {});
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const s = await initStickerWindowState();
        if (cancelled) return;
        applyPinned(s.pinned);
        await layoutCompact(s.island_center ?? 0.5, s.island_monitor);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 展开态收回：不用 DOM 的 mouseleave（窗口缩放时会误报一串假 leave/enter），轮询真实光标，
  // 连续两次确认在窗口外才收回。完整列表里焦点还在（键盘操作中）或开着右键菜单时不收；
  // 面板不算这一条：点一下会话行焦点就留在行上，算进去的话面板永远收不回去。
  useEffect(() => {
    if (isMacPanel() || (islandMode !== "expanded" && islandMode !== "full") || closing) return;
    let outCount = 0;
    const id = window.setInterval(() => {
      if (draggingRef.current) {
        outCount = 0;
        return;
      }
      const active = document.activeElement;
      const interacting =
        (active instanceof HTMLElement && active.closest(".sticker") != null)
        || document.querySelector(".ctx-menu") != null;
      if (interacting) {
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
          if (outCount >= 2) collapse();
        })
        .catch(() => {});
    }, 180);
    return () => window.clearInterval(id);
  }, [islandMode, closing, collapse]);

  // 展开态的键盘出口：Esc 收回（P2-12 同一课）。注册 Esc 层，展开的面板/列表本身就是一层浮层。
  useEffect(() => {
    if (isMacPanel() || (islandMode !== "expanded" && islandMode !== "full")) return;
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
  const openSession = useCallback((sessionId: number) => {
    invoke("open_chat_window", { sessionId }).catch((err) => console.error("[island] 打开会话失败：", err));
    const mode = islandModeRef.current;
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
  }, [collapse, layoutCompact, pumpAlert]);
  openSessionRef.current = openSession;

  if (!isMacPanel() && !bootReady) {
    return null;
  }
  // 有新版本：贴纸不再弹浮动条，改为齿轮按钮上的红点提示(安装入口在设置→关于)。
  const hasUpdate = upStatus === "available" || upStatus === "downloading" || upStatus === "ready";
  const sticker = (
    <Sticker
      filter={filter}
      onFilterChange={pickFilter}
      data={items}
      counts={counts}
      total={totalFor(filter, counts)}
      hasMore={!reachedEnd}
      loadMore={loadMore}
      loadingMore={loadingMore}
      hasUpdate={hasUpdate}
      updateStatus={upStatus}
      search={search}
      onSearchChange={changeSearch}
      onArchiveOptimistic={onArchiveOptimistic}
      onArchiveFailed={onArchiveFailed}
      onArchiveUndo={onArchiveUndo}
      initialLoading={initialLoading}
      loadError={loadError}
      onRetry={retryLoad}
      switching={switching}
      // 岛恒置顶：贴纸自己的置顶开关不再起作用（Sticker 在 snapped 时跳过 setAlwaysOnTop）。
      snapped={!isMacPanel()}
      pinned={pinned}
      onTogglePin={togglePin}
    />
  );
  if (isMacPanel()) {
    return <div style={{ height: "100%" }}>{sticker}</div>;
  }
  if (islandMode === "expanded" || islandMode === "full") {
    return (
      <div
        className={`island-shell${islandMode === "full" ? " is-full" : ""}${closing ? " is-leaving" : " is-entering"}`}
        style={{ "--island-pill-w": `${pillWidth}px`, "--island-pill-h": `${ISLAND_PILL_HEIGHT}px` } as React.CSSProperties}
      >
        {islandMode === "full" ? sticker : (
          <IslandPanel
            sessions={stripSessions}
            usage={usage}
            quotaProviders={quotaProviders}
            onOpenSession={openSession}
            onMeasure={onPanelMeasure}
            onPointerDownDrag={onPanelHeadPointerDown}
            onNewSession={() => {
              openNewSessionWindow().catch((err) => console.error("[island] 打开新建会话失败：", err));
              collapse();
            }}
          />
        )}
      </div>
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
      onPointerDownDrag={onPillPointerDown}
      onAlertHover={onAlertHover}
    />
  );
}
