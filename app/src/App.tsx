import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getLiveSessions, LiveSession } from "./api";
import { Sticker } from "./views/Sticker";

const STICKER_WIDTH = 280;
const STICKER_MAX_HEIGHT = 600;

export function App() {
  const [live, setLive] = useState<LiveSession[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);

  const refresh = useCallback(async () => {
    setLive(await getLiveSessions());
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

  // 窗口高度自适应内容——避免内容比固定窗口矮/高时在透明区出现外层滚动条。
  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    let cancelled = false;
    const apply = async () => {
      try {
        const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
        const h = Math.min(Math.ceil(el.getBoundingClientRect().height), STICKER_MAX_HEIGHT);
        if (!cancelled) {
          await getCurrentWindow().setSize(new LogicalSize(STICKER_WIDTH, Math.max(h, 32)));
        }
      } catch {
        // 非 Tauri 环境（如测试）忽略
      }
    };
    apply();
    if (typeof ResizeObserver === "undefined") {
      return () => {
        cancelled = true;
      };
    }
    const ro = new ResizeObserver(apply);
    ro.observe(el);
    return () => {
      cancelled = true;
      ro.disconnect();
    };
  }, [live]);

  return (
    <div ref={rootRef}>
      <Sticker data={live} />
    </div>
  );
}
