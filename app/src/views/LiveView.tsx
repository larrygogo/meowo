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
  const [density, setDensity] = useState<Density>(() => {
    const saved = localStorage.getItem(KEY);
    return saved === "minimal" || saved === "progress" || saved === "rich" ? saved : "progress";
  });
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

function fmtAgo(ts: number): string {
  const m = Math.floor((Date.now() - ts) / 60000);
  if (m < 1) return "刚刚";
  if (m < 60) return `${m} 分钟前`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前`;
  return `${Math.floor(h / 24)} 天前`;
}

function box(s: string): string {
  return s === "completed" ? "✔" : s === "in_progress" ? "▸" : "☐";
}

function Card({ l, density }: { l: LiveSession; density: Density }) {
  const st = STATUS[l.session.status] ?? { cls: "dot-idle", label: l.session.status };
  const unnamed = !l.task_title || l.task_title === "(未命名会话)";
  const hasTodos = l.todo_total > 0;
  const percent = hasTodos ? Math.round((l.todo_done / l.todo_total) * 100) : 0;
  const detailed = density !== "minimal";

  return (
    <div className={"live-card lc-" + density}>
      <div className="live-head">
        <span className={"dot " + st.cls} />
        <span className="live-proj">{l.project_name}</span>
        {detailed && <span className="live-status">{st.label}</span>}
      </div>

      <div className="live-title">{unnamed ? "等待首次输入…" : l.task_title}</div>

      {detailed && l.current_activity && !unnamed && (
        <div className="task-act">{l.current_activity}</div>
      )}

      {detailed &&
        (hasTodos ? (
          <>
            <div className="bar">
              <i style={{ width: `${percent}%` }} />
            </div>
            <div className="task-act">
              {l.todo_done}/{l.todo_total} · {percent}%
            </div>
          </>
        ) : (
          <div className="task-act muted">暂无子任务</div>
        ))}

      {density === "rich" && hasTodos && (
        <div className="checklist">
          {l.todos.map((t) => (
            <div className={"chk " + (t.status === "completed" ? "chk-done" : "")} key={t.id}>
              <span className="chk-box">{box(t.status)}</span>
              {t.content}
            </div>
          ))}
        </div>
      )}

      {density === "rich" && (
        <div className="live-time">最近活跃 · {fmtAgo(l.session.last_event_at)}</div>
      )}
    </div>
  );
}
