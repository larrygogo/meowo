import { LiveSession } from "../api";

const STATUS: Record<string, { cls: string; label: string }> = {
  running: { cls: "dot-run", label: "运行中" },
  waiting: { cls: "dot-wait", label: "等待输入" },
  stale: { cls: "dot-stale", label: "可能已结束" },
};

export function LiveView({ data }: { data: LiveSession[] }) {
  if (data.length === 0) {
    return <div className="empty">当前没有活跃会话。</div>;
  }
  return (
    <div className="live-grid">
      {data.map((l) => {
        const st = STATUS[l.session.status] ?? { cls: "dot-idle", label: l.session.status };
        const unnamed = !l.task_title || l.task_title === "(未命名会话)";
        const percent = l.todo_total === 0 ? 0 : Math.round((l.todo_done / l.todo_total) * 100);
        return (
          <div className="live-card" key={l.session.id}>
            <div className="live-head">
              <span className={"dot " + st.cls} />
              <span className="live-proj">{l.project_name}</span>
              <span className="live-status">{st.label}</span>
            </div>
            <div className="live-title">{unnamed ? "等待首次输入…" : l.task_title}</div>
            {l.current_activity && <div className="task-act">{l.current_activity}</div>}
            {l.todo_total > 0 && (
              <>
                <div className="bar"><i style={{ width: `${percent}%` }} /></div>
                <div className="task-act">
                  {l.todo_done}/{l.todo_total} · {percent}%
                </div>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
