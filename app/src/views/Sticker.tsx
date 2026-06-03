import { LiveSession } from "../api";

const DOT: Record<string, string> = {
  running: "dot-run",
  waiting: "dot-wait",
  stale: "dot-stale",
};

export function Sticker({ data }: { data: LiveSession[] }) {
  return (
    <div className="sticker">
      <div className="drag" data-tauri-drag-region />
      {data.length === 0 ? (
        <div className="stk-empty">无活跃会话</div>
      ) : (
        data.map((l) => {
          const unnamed = !l.task_title || l.task_title === "(未命名会话)";
          const activity = l.current_activity ?? (unnamed ? "等待首次输入" : "");
          return (
            <div className="stk-row" key={l.session.id}>
              <span className={"dot " + (DOT[l.session.status] ?? "dot-idle")} />
              <span className="stk-proj">{l.project_name}</span>
              <span className="stk-act">{activity}</span>
            </div>
          );
        })
      )}
    </div>
  );
}
