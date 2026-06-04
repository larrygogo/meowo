import { LiveSession } from "../api";

function fmtAgo(ms: number): string {
  const m = Math.floor((Date.now() - ms) / 60000);
  if (m < 1) return "now";
  if (m < 60) return `${m} 分钟前`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前`;
  return `${Math.floor(h / 24)} 天前`;
}

function ConnBadge({ connected }: { connected: boolean }) {
  return (
    <span className={"conn " + (connected ? "conn-on" : "conn-off")}>
      <svg width="11" height="11" viewBox="0 0 16 16" aria-hidden="true">
        <rect x="1.5" y="2.5" width="13" height="9" rx="1.3" fill="none" stroke="currentColor" strokeWidth="1.4" />
        <line x1="5.5" y1="14" x2="10.5" y2="14" stroke="currentColor" strokeWidth="1.4" />
        {!connected && <line x1="2" y1="13.5" x2="14" y2="2.5" stroke="currentColor" strokeWidth="1.4" />}
      </svg>
      {connected ? "Connected" : "Disconnected"}
    </span>
  );
}

type Item = LiveSession & { connected: boolean };

export function Sticker({ data }: { data: Item[] }) {
  return (
    <div className="sticker">
      <div className="drag" data-tauri-drag-region />
      {data.length === 0 ? (
        <div className="stk-empty">无活跃会话</div>
      ) : (
        data.map((l) => {
          const unnamed = !l.task_title || l.task_title === "(未命名会话)";
          const title = unnamed ? "等待首次输入" : l.task_title;
          const sub = l.current_activity && l.current_activity !== title ? l.current_activity : null;
          const indicator =
            l.connected && l.session.status === "running" ? (
              <span className="spinner" />
            ) : l.connected && l.session.status === "waiting" ? (
              <span className="needs" title="等待输入" />
            ) : null;
          return (
            <div className="stk-card" key={l.session.id}>
              <div className="stk-line1">
                {indicator}
                <span className="stk-title">{title}</span>
                <span className="stk-time">{fmtAgo(l.session.last_event_at)}</span>
              </div>
              <div className="stk-line2">
                <ConnBadge connected={l.connected} />
                <span className="stk-repo">{l.project_name}</span>
              </div>
              {sub && <div className="stk-sub">{sub}</div>}
            </div>
          );
        })
      )}
    </div>
  );
}
