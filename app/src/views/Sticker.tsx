import { LiveSession } from "../api";

const DOT: Record<string, string> = {
  waiting: "dot-wait",
  stale: "dot-stale",
};

function Indicator({ status }: { status: string }) {
  // 运行中：转动的缺口圆环；其余：静态圆点
  if (status === "running") return <span className="spinner" />;
  return <span className={"dot " + (DOT[status] ?? "dot-idle")} />;
}

export function Sticker({ data }: { data: LiveSession[] }) {
  return (
    <div className="sticker">
      <div className="drag" data-tauri-drag-region />
      {data.length === 0 ? (
        <div className="stk-empty">无活跃会话</div>
      ) : (
        data.map((l) => {
          const unnamed = !l.task_title || l.task_title === "(未命名会话)";
          const title = unnamed ? "等待首次输入" : l.task_title;
          const sub =
            l.current_activity && l.current_activity !== title ? l.current_activity : null;
          return (
            <div className="stk-row" key={l.session.id}>
              <Indicator status={l.session.status} />
              <div className="stk-main">
                <div className="stk-title">{title}</div>
                {sub && <div className="stk-sub">{sub}</div>}
              </div>
              <span className="stk-tag">{l.project_name}</span>
            </div>
          );
        })
      )}
    </div>
  );
}
