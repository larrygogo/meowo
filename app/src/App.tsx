import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getLiveSessions, LiveSession } from "./api";
import { Sticker } from "./views/Sticker";
import { CollapsedStrip } from "./views/CollapsedStrip";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right";
type Mode = "normal" | "collapsed" | "expanded";

const SNAP_KEY = "cc-kanban-snap-edge";
const SIZE_KEY = "cc-kanban-normal-size";
const SETTLE_MS = 250; // 移动停止判定
const LEAVE_MS = 300; // 离开收回防抖

function loadSize(): { w: number; h: number } {
  try {
    const s = JSON.parse(localStorage.getItem(SIZE_KEY) || "");
    if (typeof s?.w === "number" && typeof s?.h === "number") return s;
  } catch {
    /* ignore */
  }
  return { w: 340, h: 440 }; // 与 tauri.conf.json 默认一致
}

export function App() {
  const [live, setLive] = useState<Item[]>([]);
  const [mode, setMode] = useState<Mode>("normal");
  const [edge, setEdge] = useState<Edge | null>(() => {
    const s = localStorage.getItem(SNAP_KEY);
    return s === "left" || s === "right" ? s : null;
  });

  const modeRef = useRef(mode);
  const edgeRef = useRef(edge);
  modeRef.current = mode;
  edgeRef.current = edge;
  const settleTimer = useRef<number | null>(null);
  const leaveTimer = useRef<number | null>(null);

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

  // 监听窗口移动检测到的边缘，防抖判定移动停止后再吸附/恢复。
  useEffect(() => {
    const un = listen<{ edge: Edge | null }>("snap-changed", (e) => {
      const detected = e.payload.edge;
      if (settleTimer.current) window.clearTimeout(settleTimer.current);
      settleTimer.current = window.setTimeout(async () => {
        const m = modeRef.current;
        if (detected && m === "normal") {
          try {
            const sz = await getCurrentWindow().outerSize();
            const sf = await getCurrentWindow().scaleFactor();
            localStorage.setItem(SIZE_KEY, JSON.stringify({ w: sz.width / sf, h: sz.height / sf }));
          } catch {
            /* ignore */
          }
          localStorage.setItem(SNAP_KEY, detected);
          setEdge(detected);
          await invoke("snap_collapse", { edge: detected }).catch(() => {});
          setMode("collapsed");
        } else if (!detected && m !== "normal") {
          const { w, h } = loadSize();
          localStorage.removeItem(SNAP_KEY);
          setEdge(null);
          await invoke("snap_restore", { width: w, height: h }).catch(() => {});
          setMode("normal");
        }
      }, SETTLE_MS);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // 重启沿用：若上次是吸附态，启动后折叠回竖条。
  useEffect(() => {
    if (edgeRef.current) {
      invoke("snap_collapse", { edge: edgeRef.current })
        .then(() => setMode("collapsed"))
        .catch(() => {});
    }
    // 仅启动跑一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onExpand = useCallback(() => {
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current);
    if (modeRef.current !== "collapsed" || !edgeRef.current) return;
    const { w } = loadSize();
    invoke("snap_expand", { edge: edgeRef.current, width: w })
      .then(() => setMode("expanded"))
      .catch(() => {});
  }, []);

  const onLeave = useCallback(() => {
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current);
    leaveTimer.current = window.setTimeout(() => {
      if (modeRef.current === "expanded" && edgeRef.current) {
        invoke("snap_collapse", { edge: edgeRef.current })
          .then(() => setMode("collapsed"))
          .catch(() => {});
      }
    }, LEAVE_MS);
  }, []);

  if (mode === "collapsed" && edge) {
    return <CollapsedStrip data={live} edge={edge} onExpand={onExpand} onLeave={onLeave} />;
  }
  return (
    <div
      style={{ height: "100vh" }}
      onMouseLeave={mode === "expanded" ? onLeave : undefined}
      onMouseEnter={mode === "expanded" ? onExpand : undefined}
    >
      <Sticker data={live} />
    </div>
  );
}
