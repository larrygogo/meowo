import { useState } from "react";
import { LiveSession } from "../api";

type Density = "minimal" | "progress" | "rich";
const KEY = "cc-kanban-density";

const STATUS: Record<string, { cls: string; label: string }> = {
  running: { cls: "dot-run", label: "运行中" },
  waiting: { cls: "dot-wait", label: "等待输入" },
  stale: { cls: "dot-stale", label: "可能已结束" },
};

const DENSITIES: { key: Density; label: string }[] = [
  { key: "minimal", label: "极简" },
  { key: "progress", label: "进度卡" },
  { key: "rich", label: "信息丰富" },
];

export function LiveView({ data }: { data: LiveSession[] }) {
  const [density, setDensity] = useState<Density>(
    () => (localStorage.getItem(KEY) as Density) || "progress",
  );
  const pick = (d: Density) => {
    setDensity(d);
    localStorage.setItem(KEY, d);
  };

  return (
    <>
      <div className="density">
        {DENSITIES.map((d) => (
          <span
            key={d.key}
            className={"chip " + (density === d.key ? "chip-active" : "")}
            onClick={() => pick(d.key)}
          >
            {d.label}
          </span>
        ))}
      </div>
      {data.length === 0 ? (
        <div className="empty">当前没有活跃会话。</div>
      ) : (
        <div className="live-grid">
          {data.map((l) => (
            <Card key={l.session.id} l={l} density={density} />
          ))}
        </div>
      )}
    </>
  );
}

function Card({ l, density }: { l: LiveSession; density: Density }) {
  const st = STATUS[l.session.status] ?? { cls: "dot-idle", label: l.session.status };
  const unnamed = !l.task_title || l.task_title === "(未命名会话)";
  const percent = l.todo_total === 0 ? 0 : Math.round((l.todo_done / l.todo_total) * 100);
  const showBar = density !== "minimal" && l.todo_total > 0;
  const showRich = density === "rich";
  const box = (s: string) => (s === "completed" ? "✔" : s === "in_progress" ? "▸" : "☐");
  return (
    <div className="live-card">
      <div className="live-head">
        <span className={"dot " + st.cls} />
        <span className="live-proj">{l.project_name}</span>
        <span className="live-status">{st.label}</span>
      </div>
      <div className="live-title">{unnamed ? "等待首次输入…" : l.task_title}</div>
      {showRich && l.current_activity && <div className="task-act">{l.current_activity}</div>}
      {showBar && (
        <>
          <div className="bar">
            <i style={{ width: `${percent}%` }} />
          </div>
          <div className="task-act">
            {l.todo_done}/{l.todo_total} · {percent}%
          </div>
        </>
      )}
      {showRich && l.todos.length > 0 && (
        <div className="checklist">
          {l.todos.map((t) => (
            <div className={"chk " + (t.status === "completed" ? "chk-done" : "")} key={t.id}>
              <span className="chk-box">{box(t.status)}</span>
              {t.content}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
