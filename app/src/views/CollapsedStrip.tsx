import { LiveSession } from "../api";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right";

// 竖条：纵向排列各非归档会话的状态色点。悬停展开、离开收回由 App 注入回调。
export function CollapsedStrip({
  data,
  edge,
  onExpand,
  onLeave,
}: {
  data: Item[];
  edge: Edge;
  onExpand: () => void;
  onLeave: () => void;
}) {
  const items = data.filter((l) => !l.archived);
  return (
    <div
      className={"cstrip cstrip-" + edge}
      onMouseEnter={onExpand}
      onMouseLeave={onLeave}
    >
      <div className="cstrip-drag" data-tauri-drag-region />
      <div className="cstrip-dots">
        {items.map((l) => {
          const cls = !l.connected
            ? "cstrip-stop"
            : l.session.status === "running"
            ? "cstrip-running"
            : l.session.status === "waiting"
            ? "cstrip-waiting"
            : "cstrip-on";
          return <span key={l.session.id} className={"cstrip-dot " + cls} />;
        })}
      </div>
    </div>
  );
}
