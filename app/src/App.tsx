import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getOverview, getProjectTasks, ProjectOverview, TaskCard } from "./api";
import { Overview } from "./views/Overview";
import { ProjectBoard } from "./views/ProjectBoard";

type View = { kind: "overview" } | { kind: "board"; projectId: number; name: string };

export function App() {
  const [view, setView] = useState<View>({ kind: "overview" });
  const [overview, setOverview] = useState<ProjectOverview[]>([]);
  const [cards, setCards] = useState<TaskCard[]>([]);

  const refresh = useCallback(async (v: View) => {
    if (v.kind === "overview") {
      setOverview(await getOverview());
    } else {
      setCards(await getProjectTasks(v.projectId));
    }
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

  if (view.kind === "overview") {
    return (
      <div className="app">
        <div className="h1">项目总览</div>
        <Overview data={overview} onOpen={(projectId, name) => setView({ kind: "board", projectId, name })} />
      </div>
    );
  }
  return (
    <div className="app">
      <span className="back" onClick={() => setView({ kind: "overview" })}>
        ← 返回总览
      </span>
      <div className="h1">{view.name}</div>
      <ProjectBoard cards={cards} />
    </div>
  );
}
