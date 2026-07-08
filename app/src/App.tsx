import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  getLiveSessionsCounts,
  getLiveSessionsPage,
  LiveSession,
  LiveSessionCounts,
  StickerFilter,
} from "./api";
import { Sticker } from "./views/Sticker";
import { CollapsedStrip } from "./views/CollapsedStrip";
import { useUpdate } from "./useUpdate";
import { isMacPanel } from "./platform";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right" | "top";
type Mode = "normal" | "collapsed" | "expanded";

const SNAP_KEY = "cc-kanban-snap-edge";
const SIZE_KEY = "cc-kanban-normal-size";
const PIN_KEY = "cc-kanban-pinned"; // 与 Sticker 的置顶偏好共用
const TAB_KEY = "cc-kanban-tab";
const RELEASE_POLL_MS = 90; // 拖拽中轮询鼠标左键的间隔（检测真正松手）
const PAGE_SIZE = 100; // 贴纸会话每页条数，与首屏一致

// 缩略条主轴逻辑长度：按 connected 点数贴合内容（点 10px + 间距 7px = 17，两端留白 26），最小 48。
// 仅作折叠初值，CollapsedStrip 挂载后会按真实 DOM 尺寸精确校正。
function stripExtent(count: number): number {
  return Math.max(48, count * 17 + 26);
}

// 正常窗口最小尺寸：与 tauri.conf.json 的 minWidth/minHeight 对齐。SIZE_KEY 是「正常尺寸」基准，
// 不应低于此（低于即被细条尺寸毒化）。
const SIZE_MIN_W = 360;
const SIZE_MIN_H = 240;
const SIZE_MAX = 20000; // 上界：与后端 snap_* 命令的 clamp 上限一致（防 f64→i32 回绕/异常大值）
const SIZE_DEFAULT = { w: 360, h: 440 }; // 与 tauri.conf.json 默认 width/height 一致

const sizeOk = (v: unknown): v is number =>
  typeof v === "number" && Number.isFinite(v);

function loadSize(): { w: number; h: number } {
  try {
    const s = JSON.parse(localStorage.getItem(SIZE_KEY) || "");
    // 校验有限数 + 落在 [最小, 最大] 内才采用；否则回落默认。低于最小=被「吸附态拖角缩成细条」的尺寸
    // 毒化(实测 {80,240}/{136,20})；非有限数/超大值=localStorage 被改坏，直接 set_size 会设出极端窗口。
    if (sizeOk(s?.w) && sizeOk(s?.h)
      && s.w >= SIZE_MIN_W && s.h >= SIZE_MIN_H
      && s.w <= SIZE_MAX && s.h <= SIZE_MAX) {
      return s;
    }
  } catch {
    /* ignore */
  }
  return { ...SIZE_DEFAULT };
}

// 写入「正常尺寸」基准：钳到 [最小, 最大]、非有限数回落默认。吸附态下 min_size 被放开（snap_collapse），
// 拖角可把窗缩成细条；若把细条/异常尺寸当正常尺寸存入会毒化 loadSize 基准，令启动还原异常。
function saveSize(w: number, h: number) {
  const clamp = (v: number, min: number, def: number) =>
    Number.isFinite(v) ? Math.min(SIZE_MAX, Math.max(min, v)) : def;
  localStorage.setItem(
    SIZE_KEY,
    JSON.stringify({ w: clamp(w, SIZE_MIN_W, SIZE_DEFAULT.w), h: clamp(h, SIZE_MIN_H, SIZE_DEFAULT.h) })
  );
}

function isPinned(): boolean {
  return localStorage.getItem(PIN_KEY) === "1";
}

/** 当前 tab 对应的总会话数（用于 hasMore 与加载守卫，必须与后端 filter 语义一致）。 */
function totalFor(filter: StickerFilter, counts: LiveSessionCounts): number {
  switch (filter) {
    case "archived":
      return counts.archived;
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
  const [items, setItems] = useState<Item[]>([]);
  const [counts, setCounts] = useState<LiveSessionCounts>({
    total: 0,
    running: 0,
    waiting: 0,
    archived: 0,
  });
  const [loadingMore, setLoadingMore] = useState<boolean>(false);
  const [reachedEnd, setReachedEnd] = useState<boolean>(false);
  const [filter, setFilter] = useState<StickerFilter>(() => {
    const s = localStorage.getItem(TAB_KEY);
    return s === "waiting" || s === "running" || s === "archived" ? s : "all";
  });
  const pickFilter = useCallback((f: StickerFilter) => {
    setFilter(f);
    localStorage.setItem(TAB_KEY, f);
  }, []);
  const [edge, setEdge] = useState<Edge | null>(() => {
    // macOS 面板模式：无吸边，edge 固定为 null。
    if (isMacPanel()) return null;
    const s = localStorage.getItem(SNAP_KEY);
    return s === "left" || s === "right" || s === "top" ? s : null;
  });
  // mode 初值与 edge 同源：上次在吸附态关闭（window-state 会把窗口恢复成缩略条尺寸），
  // 首屏就渲染缩略条，避免在细条窗口里显示完整贴纸的脱节。
  const [mode, setMode] = useState<Mode>(() => {
    // macOS 面板模式：始终以普通态启动。
    if (isMacPanel()) return "normal";
    const s = localStorage.getItem(SNAP_KEY);
    return s === "left" || s === "right" || s === "top" ? "collapsed" : "normal";
  });
  const [glow, setGlow] = useState<Edge | null>(null); // 拖拽中靠近边缘的发光提示
  // 展开过渡中：缩放期间强制不渲染缩略条（三个圆点），落地后再恢复按 mode 判定。
  const [expanding, setExpanding] = useState(false);
  // 只读检查：仅驱动贴纸设置钮上的更新红点；下载/安装由更新窗口（views/Updater）全权负责。
  const { status: upStatus } = useUpdate();

  const connectedCount = useMemo(
    () => items.filter((l) => !l.archived && l.connected).length,
    [items]
  );

  const modeRef = useRef(mode);
  modeRef.current = mode;
  const edgeRef = useRef(edge);
  edgeRef.current = edge;
  const countRef = useRef(connectedCount);
  countRef.current = connectedCount;
  const draggingRef = useRef(false); // 是否正在拖拽窗口（mousedown 命中拖拽区）
  const lastEdgeRef = useRef<Edge | null>(null); // 最近一次 Moved 检测到的边
  const settleRef = useRef<number | null>(null);
  const preResizeEdgeRef = useRef<Edge | null>(null); // 拖角缩放前的吸附边，缩放结束后据此恢复

  // 请求序号守卫：并发刷新时旧响应可能晚于新响应返回，仅当自己仍是最新一次请求才写入。
  const refreshSeqRef = useRef(0);
  const loadPage = useCallback(
    async (
      filter: StickerFilter,
      cursor: { last_event_at: number; id: number } | null,
      isRefresh: boolean
    ): Promise<{ page: Item[]; applied: boolean }> => {
      const seq = ++refreshSeqRef.current;
      // counts 只在首页/刷新（cursor === null）时才需要；loadMore 复用已有 counts，
      // 避免频繁 loadMore 时对 counts 做纯重复的后端查询（审查发现）。
      const needCounts = cursor === null;
      try {
        const [countsRes, page] = await Promise.all([
          needCounts ? getLiveSessionsCounts() : Promise.resolve(null),
          getLiveSessionsPage(filter, cursor, PAGE_SIZE),
        ]);
        const applied = seq === refreshSeqRef.current;
        if (!applied) return { page: page as Item[], applied };
        if (countsRes) setCounts(countsRes);

        setItems((prev) => {
          if (cursor === null && !isRefresh) {
            // 切换 tab / 首次加载：直接按服务端顺序
            return (page as Item[]).slice();
          }
          // 按 id 合并，保留已加载会话；刷新时新会话插到最前，加载更多时追加到末尾。
          // 刷新/首页请求（cursor === null）时，后端 page 是当前 filter 的权威快照：
          // 已加载列表中不在 page 里的会话（状态迁移、归档、删除）应被移除，
          // 否则它们会继续停留在错误的 tab 下。
          const map = new Map(prev.map((l) => [l.session.id, l]));
          const pageIds = new Set((page as Item[]).map((l) => l.session.id));
          const append: Item[] = [];
          const prepend: Item[] = [];
          for (const l of page as Item[]) {
            if (!map.has(l.session.id)) {
              if (cursor != null) {
                append.push(l);
              } else {
                prepend.push(l);
              }
            }
            map.set(l.session.id, l);
          }
          const merged: Item[] = [];
          for (const l of prev) {
            const updated = map.get(l.session.id);
            if (!updated) continue;
            if (cursor === null && !pageIds.has(l.session.id)) continue;
            merged.push(updated);
          }
          return [...prepend, ...merged, ...append];
        });
        return { page: page as Item[], applied };
      } catch (err) {
        console.error("[loadPage] 加载失败：", err);
        throw err;
      }
    },
    []
  );

  const refresh = useCallback(() => {
    setReachedEnd(false);
    return loadPage(filter, null, true);
  }, [filter, loadPage]);

  const loadMore = useCallback(async () => {
    if (loadingMore || reachedEnd || items.length >= totalFor(filter, counts)) return;
    const last = items[items.length - 1];
    if (!last) return;
    setLoadingMore(true);
    try {
      const { page, applied } = await loadPage(
        filter,
        { last_event_at: last.session.last_event_at, id: last.session.id },
        false
      );
      // 请求过程中若已被更新的请求（如切 tab/刷新）取代，本次结果不再代表当前 tab 的状态，
      // reachedEnd 不应据此更新，否则可能把旧 tab 的「已到底」误写到新 tab 上（审查发现的竞态）。
      if (applied && page.length < PAGE_SIZE) {
        setReachedEnd(true);
      }
    } catch (err) {
      console.error("[loadMore] 加载失败：", err);
    } finally {
      setLoadingMore(false);
    }
  }, [filter, loadingMore, reachedEnd, items, counts, loadPage]);

  // tab 切换/首次挂载时重置并加载该分类首页
  useEffect(() => {
    setReachedEnd(false);
    loadPage(filter, null, false)
      .then(({ page, applied }) => {
        if (applied && page.length < PAGE_SIZE) setReachedEnd(true);
      })
      .catch(() => {});
  }, [filter, loadPage]);

  useEffect(() => {
    const un = listen("board-changed", () => refresh());
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  // 托盘「找回贴纸」：把贴纸拉回主屏中央并置顶。折叠/吸附态先展开还原成正常窗口，再居中置顶。
  // macOS 面板模式无吸边/托盘菜单项，跳过。
  useEffect(() => {
    if (isMacPanel()) return;
    const recall = async () => {
      // 置顶偏好（Sticker 的 pin 按钮也读此 key）；折叠态下 Sticker 未挂载，展开后据此初始化为置顶。
      localStorage.setItem(PIN_KEY, "1");
      if (modeRef.current !== "normal") {
        const { w, h } = loadSize();
        localStorage.removeItem(SNAP_KEY);
        setEdge(null);
        setMode("normal");
        try {
          await invoke("snap_restore", { width: w, height: h, pinned: true });
        } catch (err) {
          console.error("[recall] snap_restore 失败：", err);
        }
      }
      try {
        await invoke("recall_center");
      } catch (err) {
        console.error("[recall] recall_center 失败：", err);
      }
    };
    const un = listen("recall-sticker", () => void recall());
    return () => {
      un.then((f) => f());
    };
  }, []);

  // 折叠成缩略条：厚度固定，主轴长度贴合当前点数。
  const doCollapse = useCallback(
    (d: Edge) => invoke("snap_collapse", { edge: d, extent: stripExtent(countRef.current) }),
    []
  );

  // 拖拽松手处理：靠边→折叠（从 normal 先存正常尺寸）；离边→若在吸附态则还原普通窗口。
  // macOS 面板模式：直接返回，不处理吸边逻辑。
  const handleDragRelease = useCallback(async () => {
    if (isMacPanel()) return;
    if (!draggingRef.current) return;
    draggingRef.current = false;
    document.documentElement.classList.remove("win-dragging");
    if (settleRef.current) {
      window.clearInterval(settleRef.current);
      settleRef.current = null;
    }
    setGlow(null);
    const d = lastEdgeRef.current;
    const m = modeRef.current;
    if (d) {
      // 靠边松手 → 折叠
      if (m === "normal") {
        try {
          const sz = await getCurrentWindow().outerSize();
          const sf = await getCurrentWindow().scaleFactor();
          saveSize(sz.width / sf, sz.height / sf);
        } catch {
          /* ignore */
        }
      }
      if (m !== "collapsed") {
        try {
          await doCollapse(d);
          localStorage.setItem(SNAP_KEY, d);
          setEdge(d);
          setMode("collapsed");
        } catch (err) {
          console.error("[snap] snap_collapse 失败：", err);
        }
      }
    } else if (m !== "normal") {
      // 离边松手 → 还原普通窗口
      const { w, h } = loadSize();
      try {
        await invoke("snap_restore", { width: w, height: h, pinned: isPinned() });
        localStorage.removeItem(SNAP_KEY);
        setEdge(null);
        setMode("normal");
      } catch (err) {
        console.error("[snap] snap_restore 失败：", err);
      }
    }
  }, [doCollapse]);

  // 用户拖边框缩放窗口（后端 WM_SIZING/WM_EXITSIZEMOVE 检测）：
  // - 缩放开始(user-resized)：暂解除吸附变普通窗口，避免缩放中被吸附逻辑误折叠抖动；记住原吸附边。
  // - 缩放结束(user-resize-end)：若缩放前是吸附的，按新尺寸重新吸回原来那条边（保留新尺寸）。
  useEffect(() => {
    if (isMacPanel()) return;
    const unStart = listen("user-resized", () => {
      if (modeRef.current === "normal") return; // 本就普通态，无吸附可解
      preResizeEdgeRef.current = edgeRef.current; // 记住缩放前的吸附边
      invoke("unsnap", { pinned: isPinned() }).catch(() => {});
      localStorage.removeItem(SNAP_KEY);
      setEdge(null);
      setMode("normal");
    });
    const unEnd = listen("user-resize-end", async () => {
      const e = preResizeEdgeRef.current;
      preResizeEdgeRef.current = null;
      if (!e) {
        // 缩放前非吸附态 → 保持普通窗口；把新尺寸记为常用尺寸，供启动对账/还原沿用，
        // 避免用户自定义的小窗口被误判为「吸附遗留细条」而被强行放大。
        try {
          const sz = await getCurrentWindow().outerSize();
          const sf = await getCurrentWindow().scaleFactor();
          saveSize(sz.width / sf, sz.height / sf);
        } catch {
          /* ignore */
        }
        return;
      }
      // 把缩放后的尺寸记为新的常用尺寸，再按它重新吸回原边（snap_expand 会贴边定位）。
      // saveSize 会钳到最小尺寸：吸附态下 min 被放开，拖角可把窗缩成细条，不能让细条尺寸污染基准。
      try {
        const sz = await getCurrentWindow().outerSize();
        const sf = await getCurrentWindow().scaleFactor();
        saveSize(sz.width / sf, sz.height / sf);
      } catch {
        /* ignore */
      }
      const { w, h } = loadSize();
      localStorage.setItem(SNAP_KEY, e);
      setEdge(e);
      setMode("expanded");
      invoke("snap_expand", { edge: e, width: w, height: h }).catch((err) =>
        console.error("[snap] 缩放后重新吸附失败：", err)
      );
    });
    return () => {
      unStart.then((f) => f());
      unEnd.then((f) => f());
    };
  }, []);

  // 监听窗口移动：拖拽中靠近边缘则发光，并轮询鼠标左键——真正松手才吸附。
  // data-tauri-drag-region 的 OS 拖动循环里 webview 收不到 mouseup，故问后端键状态；
  // 用 setInterval(而非定时兜底)，即使停在边缘不动也会持续轮询、松手即触发，不会因停顿误吸。
  // macOS 面板模式无吸边，不注册此监听器。
  useEffect(() => {
    if (isMacPanel()) return;
    const un = listen<{ edge: Edge | null }>("snap-changed", (e) => {
      const d = e.payload.edge;
      lastEdgeRef.current = d;
      if (!draggingRef.current) {
        setGlow(null);
        return;
      }
      setGlow(d);
      if (settleRef.current) return; // 轮询已在跑，无需重复启动
      settleRef.current = window.setInterval(() => {
        if (!draggingRef.current) {
          if (settleRef.current) window.clearInterval(settleRef.current);
          settleRef.current = null;
          return;
        }
        invoke<boolean>("pointer_left_down")
          .then((down) => {
            if (down) return; // 仍按着，继续等
            if (settleRef.current) window.clearInterval(settleRef.current);
            settleRef.current = null;
            void handleDragRelease();
          })
          .catch(() => {});
      }, RELEASE_POLL_MS);
    });
    return () => {
      un.then((f) => f());
    };
  }, [handleDragRelease]);

  // 拖拽开始/结束检测：mousedown 命中拖拽区 → 标记拖拽；mouseup → 处理松手。
  // macOS 面板模式：无拖拽/吸边，不注册此监听器。
  useEffect(() => {
    if (isMacPanel()) return;
    const onDown = (ev: MouseEvent) => {
      const t = ev.target as HTMLElement | null;
      if (t && t.closest("[data-tauri-drag-region]")) {
        // 双击拖拽区会触发 Tauri 默认的窗口最大化，贴纸不该被最大化 → 在 capture 阶段拦掉。
        if (ev.detail >= 2) {
          ev.preventDefault();
          ev.stopPropagation();
          return;
        }
        draggingRef.current = true;
        // 拖拽全程给 <html> 挂 class，驱动拖拽条放大——:active 在 OS 拖动接管后会丢失，不可靠。
        document.documentElement.classList.add("win-dragging");
      }
    };
    const onUp = () => {
      void handleDragRelease();
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("mouseup", onUp, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("mouseup", onUp, true);
    };
  }, [handleDragRelease]);

  // CollapsedStrip 测量到的真实内容尺寸 → 精确调整缩略条主轴长度（贴合、无滚动条）。
  const onMeasure = useCallback((ext: number) => {
    if (modeRef.current === "collapsed" && edgeRef.current) {
      invoke("snap_collapse", { edge: edgeRef.current, extent: ext }).catch((err) =>
        console.error("[snap] 调整缩略条尺寸失败：", err)
      );
    }
  }, []);

  // 重启沿用：上次吸附态→折叠回竖条；否则按 SIZE_KEY 还原正常尺寸。macOS 面板模式跳过。
  useEffect(() => {
    if (isMacPanel()) return;
    const e = edgeRef.current;
    if (e) {
      doCollapse(e)
        .then(() => setMode("collapsed"))
        .catch((err) => console.error("[snap] 启动沿用折叠失败：", err));
      return;
    }
    // 非吸附态启动：main 窗口尺寸由 localStorage(SIZE_KEY) 单一持有——window-state 已配置为不恢复尺寸
    // (见 lib.rs)，窗口此刻是 tauri.conf 默认尺寸、位置则由 window-state 恢复。无条件按 SIZE_KEY 还原
    // 成用户上次的正常尺寸(snap_restore 保留位置并拉回工作区内)。这样 window-state 永不持有「细条几何」，
    // 从根上消除「window-state 几何 ↔ SNAP_KEY 吸附态」不同步导致的「细条大小+完整内容+没真正吸附」。
    (async () => {
      try {
        const { w, h } = loadSize();
        await invoke("snap_restore", { width: w, height: h, pinned: isPinned() });
      } catch {
        /* 非 Tauri 环境（测试/浏览器）忽略 */
      }
    })();
    // 仅启动跑一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 偷看：悬停缩略条 → 就地放大成全尺寸（仍贴边）。先切看板视图再放大，避免放大瞬间渲染缩略条。
  // macOS 面板模式：无此操作。收回由下方光标轮询负责（不用 DOM mouseleave，缩放时会误报）。
  const onExpand = useCallback(() => {
    if (isMacPanel()) return;
    if (modeRef.current !== "collapsed" || !edgeRef.current) return;
    const { w, h } = loadSize();
    const e = edgeRef.current;
    setExpanding(true);
    setMode("expanded");
    invoke("snap_expand", { edge: e, width: w, height: h })
      .catch((err) => console.error("[snap] snap_expand 失败：", err))
      .finally(() => setExpanding(false));
  }, []);

  // 展开态收回：不用 DOM 的 mouseleave（窗口缩放时会误报一串假 leave/enter → 抖动死循环），
  // 改为轮询真实光标坐标，连续两次确认在窗口外才收回（给短暂掠出一点容差）。
  useEffect(() => {
    if (isMacPanel() || mode !== "expanded") return;
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
          if (outCount >= 2 && modeRef.current === "expanded" && edgeRef.current) {
            doCollapse(edgeRef.current)
              .then(() => setMode("collapsed"))
              .catch((err) => console.error("[snap] 收回竖条失败：", err));
          }
        })
        .catch(() => {});
    }, 180);
    return () => window.clearInterval(id);
  }, [mode, doCollapse]);

  if (!isMacPanel() && mode === "collapsed" && edge && !expanding) {
    return <CollapsedStrip data={items} edge={edge} onExpand={onExpand} onMeasure={onMeasure} />;
  }
  // 有新版本：贴纸不再弹浮动条，改为齿轮按钮上的红点提示(安装入口在设置→关于)。
  const hasUpdate = upStatus === "available";
  return (
    <div style={{ height: "100%" }}>
      {!isMacPanel() && glow && <div className={"snap-glow snap-glow-" + glow} />}
      <Sticker
        filter={filter}
        onFilterChange={pickFilter}
        data={items}
        counts={counts}
        total={totalFor(filter, counts)}
        hasMore={!reachedEnd && items.length < totalFor(filter, counts)}
        loadMore={loadMore}
        loadingMore={loadingMore}
        hasUpdate={hasUpdate}
      />
    </div>
  );
}
