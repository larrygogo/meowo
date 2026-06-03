import { ProjectOverview } from "../api";

export function Overview({
  data,
  onOpen,
}: {
  data: ProjectOverview[];
  onOpen: (projectId: number, name: string) => void;
}) {
  if (data.length === 0) {
    return <div className="empty">还没有任何项目。打开一个 Claude Code 会话试试。</div>;
  }
  return (
    <div className="proj-grid">
      {data.map((o) => (
        <div className="proj-card" key={o.project.id} onClick={() => onOpen(o.project.id, o.project.name)}>
          <div className="proj-name">
            <span className={"dot " + (o.active_sessions > 0 ? "dot-active" : "dot-idle")} />
            {o.project.name}
          </div>
          <div className="proj-meta">
            <span>{o.active_sessions} 活跃</span>
            <span>{o.todo_count} 待办</span>
            <span>{o.doing_count} 进行</span>
            <span>{o.done_count} 完成</span>
          </div>
        </div>
      ))}
    </div>
  );
}
