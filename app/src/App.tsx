import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getLiveSessions, LiveSession } from "./api";
import { Sticker } from "./views/Sticker";
import { CollapsedStrip } from "./views/CollapsedStrip";
import { useUpdate } from "./useUpdate";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right" | "top";
type Mode = "normal" | "collapsed" | "expanded";

const SNAP_KEY = "cc-kanban-snap-edge";
const SIZE_KEY = "cc-kanban-normal-size";
const PIN_KEY = "cc-kanban-pinned"; // 与 Sticker 的置顶偏好共用
const SETTLE_MS = 600; // 拖拽松手兜底（mouseup 未触发时）
const LEAVE_MS = 350; // 偷看展开后鼠标离开收回的防抖

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
  const [live, setLive] = useState<Item[]>([]);
  const [mode, setMode] = useState<Mode>("normal");
  const [edge, setEdge] = useState<Edge | null>(() => {
    const s = localStorage.getItem(SNAP_KEY);
    return s === "left" || s === "right" || s === "top" ? s : null;
  });
  const [glow, setGlow] = useState<Edge | null>(null); // 拖拽中靠近边缘的发光提示
  const { status: upStatus, version: updateVersion, progress: upProgress, apply: applyUpdate } = useUpdate();

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
  const leaveRef = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    setLive((await getLiveSessions()) as Item[]);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const un = listen("board-changed", () => refresh());
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  // 托盘「更新」点击 → 执行更新（安装逻辑的单一来源）。
  useEffect(() => {
    const un = listen("trigger-update", () => void applyUpdate());
    return () => {
      un.then((f) => f());
    };
  }, [applyUpdate]);

  // 折叠成缩略条：厚度固定，主轴长度贴合当前点数。
  const doCollapse = useCallback(
    (d: Edge) => invoke("snap_collapse", { edge: d, extent: stripExtent(countRef.current) }),
    []
  );

  // 拖拽松手处理：靠边→折叠（从 normal 先存正常尺寸）；离边→若在吸附态则还原普通窗口。
  const handleDragRelease = useCallback(async () => {
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

  // 监听窗口移动：拖拽中靠近边缘则发光；停止移动 SETTLE_MS 作为松手兜底。
  useEffect(() => {
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
  useEffect(() => {
    const onDown = (ev: MouseEvent) => {
      const t = ev.target as HTMLElement | null;
      if (t && t.closest("[data-tauri-drag-region]")) {
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

  // 重启沿用：若上次是吸附态，启动后折叠回竖条。
  useEffect(() => {
    const e = edgeRef.current;
    if (e) {
      doCollapse(e)
        .then(() => setMode("collapsed"))
        .catch((err) => console.error("[snap] 启动沿用折叠失败：", err));
    }
    // 仅启动跑一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 偷看：悬停竖条 → 展开成全尺寸（仍贴边）。
  const onExpand = useCallback(() => {
    if (leaveRef.current) {
      window.clearTimeout(leaveRef.current);
      leaveRef.current = null;
    }
    if (modeRef.current !== "collapsed" || !edgeRef.current) return;
    const { w, h } = loadSize();
    invoke("snap_expand", { edge: edgeRef.current, width: w, height: h })
      .then(() => setMode("expanded"))
      .catch((err) => console.error("[snap] snap_expand 失败：", err));
  }, []);

  // 离开展开态 → 防抖后收回竖条；正在拖窗时不收（拖离边缘由松手处理）。
  const onExpandedLeave = useCallback(() => {
    if (leaveRef.current) window.clearTimeout(leaveRef.current);
    leaveRef.current = window.setTimeout(() => {
      if (modeRef.current === "expanded" && edgeRef.current && !draggingRef.current) {
        doCollapse(edgeRef.current)
          .then(() => setMode("collapsed"))
          .catch((err) => console.error("[snap] 收回竖条失败：", err));
      }
    }, LEAVE_MS);
  }, [doCollapse]);

  const onExpandedEnter = useCallback(() => {
    if (leaveRef.current) {
      window.clearTimeout(leaveRef.current);
      leaveRef.current = null;
    }
  }, []);

  if (mode === "collapsed" && edge) {
    return <CollapsedStrip data={live} edge={edge} onExpand={onExpand} onMeasure={onMeasure} />;
  }
  return (
    <div
      style={{ height: "100vh" }}
      onMouseEnter={mode === "expanded" ? onExpandedEnter : undefined}
      onMouseLeave={mode === "expanded" ? onExpandedLeave : undefined}
    >
      {glow && <div className={"snap-glow snap-glow-" + glow} />}
      {(upStatus === "available" || upStatus === "downloading") && (
        <div
          className="update-bar"
          title="点击下载并安装新版本"
          onClick={upStatus === "downloading" ? undefined : applyUpdate}
        >
          {upStatus === "downloading"
            ? `下载更新中 ${upProgress}%`
            : `有新版本 v${updateVersion} · 点击更新`}
        </div>
      )}
      <Sticker data={live} />
    </div>
  );
}
