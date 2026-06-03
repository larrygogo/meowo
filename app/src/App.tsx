import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getLiveSessions, LiveSession } from "./api";
import { Sticker } from "./views/Sticker";

export function App() {
  const [live, setLive] = useState<LiveSession[]>([]);

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

  return <Sticker data={live} />;
}
