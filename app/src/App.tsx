import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getLiveSessions, LiveSession } from "./api";
import { Sticker } from "./views/Sticker";
import { CollapsedStrip } from "./views/CollapsedStrip";
import { useUpdate } from "./useUpdate";
import { isMacPanel } from "./platform";
import { useT } from "./i18n";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right" | "top";
type Mode = "normal" | "collapsed" | "expanded";

const SNAP_KEY = "cc-kanban-snap-edge";
const SIZE_KEY = "cc-kanban-normal-size";
const PIN_KEY = "cc-kanban-pinned"; // 与 Sticker 的置顶偏好共用
const SETTLE_MS = 600; // 拖拽松手兜底（mouseup 未触发时）

// 缩略条主轴逻辑长度：按 connected 点数贴合内容（点 10px + 间距 7px = 17，两端留白 26），最小 48。
// 仅作折叠初值，CollapsedStrip 挂载后会按真实 DOM 尺寸精确校正。
function stripExtent(count: number): number {
  return Math.max(48, count * 17 + 26);
}

function loadSize(): { w: number; h: number } {
  try {
    const s = JSON.parse(localStorage.getItem(SIZE_KEY) || "");
    if (typeof s?.w === "number" && typeof s?.h === "number") return s;
  } catch {
    /* ignore */
  }
  return { w: 340, h: 440 }; // 与 tauri.conf.json 默认一致
}

function isPinned(): boolean {
  return localStorage.getItem(PIN_KEY) === "1";
}

export function App() {
  const t = useT();
  const [live, setLive] = useState<Item[]>([]);
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
  const {
    status: upStatus,
    version: updateVersion,
    progress: upProgress,
    apply: applyUpdate,
    recheck,
  } = useUpdate();
  const upStatusRef = useRef(upStatus);
  upStatusRef.current = upStatus;

  const connectedCount = live.filter((l) => !l.archived && l.connected).length;

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

  const refresh = useCallback(async () => {
    setLive((await getLiveSessions()) as Item[]);
  }, []);

  // 平台探测已提前到 main.tsx 渲染前完成，这里只负责首次拉取。
  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const un = listen("board-changed", () => refresh());
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  // 安装的单一来源：关于窗口的更新按钮发 trigger-update，统一由主窗处理。
  // 有新版则安装，否则重新检查；先把主窗显示出来好看进度。
  useEffect(() => {
    const handle = () => {
      getCurrentWindow().show().catch(() => {});
      if (upStatusRef.current === "available") void applyUpdate();
      else if (upStatusRef.current !== "downloading") void recheck();
    };
    const un = listen("trigger-update", handle);
    return () => {
      un.then((f) => f());
    };
  }, [applyUpdate, recheck]);

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
    if (settleRef.current) {
      window.clearTimeout(settleRef.current);
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
          localStorage.setItem(SIZE_KEY, JSON.stringify({ w: sz.width / sf, h: sz.height / sf }));
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
      if (!e) return; // 缩放前非吸附态 → 保持普通窗口
      // 把缩放后的尺寸记为新的常用尺寸，再按它重新吸回原边（snap_expand 会贴边定位）。
      try {
        const sz = await getCurrentWindow().outerSize();
        const sf = await getCurrentWindow().scaleFactor();
        localStorage.setItem(SIZE_KEY, JSON.stringify({ w: sz.width / sf, h: sz.height / sf }));
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

  // 监听窗口移动：拖拽中靠近边缘则发光；停止移动 SETTLE_MS 作为松手兜底。
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
      if (settleRef.current) window.clearTimeout(settleRef.current);
      settleRef.current = window.setTimeout(() => void handleDragRelease(), SETTLE_MS);
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

  // 重启沿用：若上次是吸附态，启动后折叠回竖条。macOS 面板模式跳过。
  useEffect(() => {
    if (isMacPanel()) return;
    const e = edgeRef.current;
    if (e) {
      doCollapse(e)
        .then(() => setMode("collapsed"))
        .catch((err) => console.error("[snap] 启动沿用折叠失败：", err));
    }
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
    return <CollapsedStrip data={live} edge={edge} onExpand={onExpand} onMeasure={onMeasure} />;
  }
  return (
    <div
      style={{ height: "100vh" }}
    >
      {!isMacPanel() && glow && <div className={"snap-glow snap-glow-" + glow} />}
      {(upStatus === "available" || upStatus === "downloading") && (
        <div
          className="update-bar"
          title={t.update.clickToInstall}
          onClick={upStatus === "downloading" ? undefined : applyUpdate}
        >
          {upStatus === "downloading"
            ? upProgress == null
              ? t.update.downloadingNoPct
              : t.update.downloading(upProgress)
            : t.update.newVersion(updateVersion ?? "")}
        </div>
      )}
      <Sticker data={live} />
    </div>
  );
}
