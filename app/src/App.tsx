import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getOverview,
  getProjectTasks,
  getLiveSessions,
  ProjectOverview,
  TaskCard,
  LiveSession,
} from "./api";
import { Overview } from "./views/Overview";
import { ProjectBoard } from "./views/ProjectBoard";
import { LiveView } from "./views/LiveView";

type View =
  | { kind: "overview" }
  | { kind: "live" }
  | { kind: "board"; projectId: number; name: string };

export function App() {
  const [view, setView] = useState<View>({ kind: "overview" });
  const [overview, setOverview] = useState<ProjectOverview[]>([]);
  const [live, setLive] = useState<LiveSession[]>([]);
  const [cards, setCards] = useState<TaskCard[]>([]);

  const refresh = useCallback(async (v: View) => {
    if (v.kind === "overview") setOverview(await getOverview());
    else if (v.kind === "live") setLive(await getLiveSessions());
    else setCards(await getProjectTasks(v.projectId));
  }, []);

  useEffect(() => {
    refresh(view);
  }, [view, refresh]);

  useEffect(() => {
    const un = listen("board-changed", () => refresh(view));
    return () => {
      un.then((f) => f());
    };
  }, [view, refresh]);

  const tab = (kind: "overview" | "live", label: string) => (
    <span
      className={"tab " + (view.kind === kind ? "tab-active" : "")}
      onClick={() => setView({ kind })}
    >
      {label}
    </span>
  );

  return (
    <div className="app">
      <div className="nav">
        {tab("overview", "项目总览")}
        {tab("live", "当前活跃")}
      </div>
      {view.kind === "overview" && (
        <Overview
          data={overview}
          onOpen={(projectId, name) => setView({ kind: "board", projectId, name })}
        />
      )}
      {view.kind === "live" && <LiveView data={live} />}
      {view.kind === "board" && (
        <>
          <span className="back" onClick={() => setView({ kind: "overview" })}>
            ← 返回总览
          </span>
          <div className="h1">{view.name}</div>
          <ProjectBoard cards={cards} />
        </>
      )}
    </div>
  );
}
