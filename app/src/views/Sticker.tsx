import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
type Tab = "all" | "waiting" | "running" | "archived";

function TabIcon({ tab }: { tab: Tab }) {
  const common = { width: 11, height: 11, viewBox: "0 0 16 16", "aria-hidden": true } as const;
  switch (tab) {
    case "all": // 列表
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
          <line x1="3" y1="4.5" x2="13" y2="4.5" />
          <line x1="3" y1="8" x2="13" y2="8" />
          <line x1="3" y1="11.5" x2="13" y2="11.5" />
        </svg>
      );
    case "waiting": // 对话气泡（待交互）
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round">
          <path d="M2.5 3.5h11v7h-6l-3 2.5v-2.5h-2z" />
        </svg>
      );
    case "running": // 播放（运行中）
      return (
        <svg {...common} fill="currentColor">
          <path d="M5 3.2 12.5 8 5 12.8z" />
        </svg>
      );
    case "archived": // 归档盒
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
          <rect x="2.5" y="3" width="11" height="3" rx="0.5" />
          <path d="M3.5 6v6.5h9V6" />
          <line x1="6.5" y1="9" x2="9.5" y2="9" strokeLinecap="round" />
        </svg>
      );
  }
}

const TAB_KEY = "cc-kanban-tab";
const TABS: { key: Tab; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "waiting", label: "待交互" },
  { key: "running", label: "运行中" },
  { key: "archived", label: "已归档" },
];

function match(tab: Tab, l: Item): boolean {
  if (tab === "archived") return l.archived;
  if (l.archived) return false; // 已归档的不在其它分类显示
  if (tab === "all") return true;
  if (tab === "waiting") return l.connected && l.session.status === "waiting";
  if (tab === "running") return l.connected && l.session.status === "running";
  return true;
}

export function Sticker({ data }: { data: Item[] }) {
  const [tab, setTab] = useState<Tab>(() => {
    const s = localStorage.getItem(TAB_KEY);
    return s === "waiting" || s === "running" || s === "archived" ? s : "all";
  });

  const pick = (t: Tab) => {
    setTab(t);
    localStorage.setItem(TAB_KEY, t);
  };

  const shown = data.filter((l) => match(tab, l));

  return (
    <div className="sticker">
      <div className="drag" data-tauri-drag-region />
      <div className="tabs">
        {TABS.map((t) => {
          const n = data.filter((l) => match(t.key, l)).length;
          return (
            <span
              key={t.key}
              className={"stab " + (tab === t.key ? "stab-on" : "")}
              onClick={() => pick(t.key)}
            >
              <TabIcon tab={t.key} />
              {t.label}
              <span className="stab-n">{n}</span>
            </span>
          );
        })}
      </div>
      <div className="stk-scroll">
        {shown.length === 0 ? (
          <div className="stk-empty">（空）</div>
        ) : (
          shown.map((l) => {
            const unnamed = !l.task_title || l.task_title === "(未命名会话)";
            const title = unnamed ? "等待首次输入" : l.task_title;
            const sub = l.current_activity && l.current_activity !== title ? l.current_activity : null;
            const pct = l.todo_total > 0 ? Math.round((l.todo_done / l.todo_total) * 100) : 0;
            const indicator = !l.connected ? (
              <span className="sdot sdot-off" title="已断开" />
            ) : l.session.status === "running" ? (
              <span className="spinner" />
            ) : l.session.status === "waiting" ? (
              <span className="needs" title="等待输入" />
            ) : (
              <span className="sdot sdot-on" title="在线" />
            );
            return (
              <div
                className="stk-card"
                key={l.session.id}
                onClick={() => { if (l.pid) invoke("focus_session", { pid: l.pid }).catch(() => {}); }}
                style={{ cursor: l.pid ? "pointer" : "default" }}
              >
                <div className="stk-line1">
                  {indicator}
                  <span className="stk-title">{title}</span>
                  <span className="stk-time">{fmtAgo(l.session.last_event_at)}</span>
                  <span
                    className="stk-arch"
                    title={l.archived ? "取消归档" : "归档"}
                    onClick={(e) => { e.stopPropagation(); invoke("set_archived", { sessionId: l.session.id, archived: !l.archived }).catch(() => {}); }}
                  >{l.archived ? "↩" : "▾"}</span>
                </div>
                <div className="stk-line2">
                  <ConnBadge connected={l.connected} />
                  <span className="stk-repo">{l.project_name}</span>
                </div>
                {sub && <div className="stk-sub">{sub}</div>}
                {l.todo_total > 0 && (
                  <div className="stk-prog">
                    <div className="bar">
                      <i style={{ width: `${pct}%` }} />
                    </div>
                    <span className="stk-prog-txt">
                      {l.todo_done}/{l.todo_total}
                    </span>
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
      <div
        className="resize-grip"
        onMouseDown={(e) => {
          e.preventDefault();
          getCurrentWindow().startResizeDragging("SouthEast").catch(() => {});
        }}
      />
    </div>
  );
}
