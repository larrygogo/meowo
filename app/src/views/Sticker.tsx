import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { LiveSession, Settings, getSettings } from "../api";
import { isMacPanel } from "../platform";

const DAY_MS = 86_400_000;

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

function ArchiveIcon({ archived }: { archived: boolean }) {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect width="20" height="5" x="2" y="3" rx="1" />
      <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
      {archived ? <path d="m9 15 3-3 3 3" /> /* 还原：向上箭头 */ : <path d="M10 12h4" /> /* 归档：把手 */}
    </svg>
  );
}

function PencilIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
    </svg>
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

function PinIcon({ pinned }: { pinned: boolean }) {
  // lucide pin：未置顶描边、置顶时填充以示激活
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill={pinned ? "currentColor" : "none"}
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 17v5" />
      <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />
    </svg>
  );
}

/** 状态徽标：圆角矩形边框上流动的亮线（conic 渐变 + transform 旋转，纯 GPU 合成，
 *  拖动窗口不占主线程）+ 中心实心圆，圆内显示 Content 已用百分比。
 *  tone：running=绿（运行中），waiting=黄（待交互），结构一致仅换色。 */
function RunBadge({
  pct,
  tone = "running",
}: {
  pct: number | null;
  tone?: "running" | "waiting";
}) {
  const what = tone === "waiting" ? "等待输入" : "运行中";
  const label = pct != null ? `${what} · Content 已用 ${pct}%` : what;
  return (
    <span
      className={"run-badge" + (tone === "waiting" ? " run-badge--waiting" : "")}
      role="img"
      aria-label={label}
      title={label}
    >
      {/* 旋转的亮段（被 .run-badge 的圆角裁剪 → 光点沿边框跑） */}
      <span className="run-sweep" />
      {/* 遮住中心黑底，只露出外圈一圈边框 */}
      <span className="run-mask" />
      {/* 中心实心圆 + 百分比 */}
      <span className="run-core">{pct != null ? `${pct}%` : ""}</span>
    </span>
  );
}

const TAB_KEY = "cc-kanban-tab";
const PIN_KEY = "cc-kanban-pinned";
const TABS: { key: Tab; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "waiting", label: "待交互" },
  { key: "running", label: "运行中" },
  { key: "archived", label: "已归档" },
];

function match(tab: Tab, l: Item, hideDays = 0): boolean {
  if (tab === "archived") {
    if (!l.archived) return false;
    // 归档超过 hideDays 天自动隐藏；archived_at 缺失的旧条目不隐藏。
    if (hideDays > 0 && l.archived_at && Date.now() - l.archived_at > hideDays * DAY_MS) {
      return false;
    }
    return true;
  }
  if (l.archived) return false; // 已归档的不在其它分类显示
  if (tab === "all") return true;
  if (tab === "waiting") return l.connected && (l.session.status === "waiting" || l.errored);
  if (tab === "running") return l.connected && l.session.status === "running" && !l.errored;
  return true;
}

const EMPTY: Record<Tab, { title: string; hint: string | null }> = {
  all: { title: "还没有会话", hint: "在终端运行 Claude Code，进度会自动出现在这里" },
  waiting: { title: "没有等待交互的会话", hint: "有会话需要你回复时会出现在这里" },
  running: { title: "当前没有运行中的会话", hint: null },
  archived: { title: "没有归档的会话", hint: "点卡片右上角按钮可收纳会话" },
};

function EmptyIcon({ tab }: { tab: Tab }) {
  const common = {
    width: 28, height: 28, viewBox: "0 0 24 24", fill: "none",
    stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round",
    strokeLinejoin: "round", "aria-hidden": true,
  } as const;
  switch (tab) {
    case "all": // 显示器
      return (
        <svg {...common}>
          <rect width="20" height="14" x="2" y="3" rx="2" />
          <line x1="8" y1="21" x2="16" y2="21" />
          <line x1="12" y1="17" x2="12" y2="21" />
        </svg>
      );
    case "waiting": // 对话气泡
      return (
        <svg {...common}>
          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
        </svg>
      );
    case "running": // 播放
      return (
        <svg {...common}>
          <polygon points="6 3 20 12 6 21 6 3" />
        </svg>
      );
    case "archived": // 归档盒
      return (
        <svg {...common}>
          <rect width="20" height="5" x="2" y="3" rx="1" />
          <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
          <path d="M10 12h4" />
        </svg>
      );
  }
}

export function EmptyState({ tab }: { tab: Tab }) {
  const { title, hint } = EMPTY[tab];
  return (
    <div className="stk-empty">
      <span className="stk-empty-icon"><EmptyIcon tab={tab} /></span>
      <div className="stk-empty-title">{title}</div>
      {hint && <div className="stk-empty-hint">{hint}</div>}
    </div>
  );
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

  // 归档自动隐藏天数：启动时读设置，并监听设置窗口的实时变更。
  const [hideDays, setHideDays] = useState(0);
  useEffect(() => {
    getSettings().then((s) => setHideDays(s.archive_hide_days)).catch(() => {});
    let un: (() => void) | undefined;
    try {
      listen<Settings>("settings-changed", (e) => setHideDays(e.payload.archive_hide_days))
        .then((f) => { un = f; })
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（测试/浏览器） */
    }
    return () => {
      try { un?.(); } catch { /* noop */ }
    };
  }, []);

  // 置顶开关：默认不置顶，激活后才把窗口设为 alwaysOnTop，状态持久化。
  const [pinned, setPinned] = useState<boolean>(() => localStorage.getItem(PIN_KEY) === "1");
  useEffect(() => {
    // 非 Tauri 环境（测试/浏览器）下 getCurrentWindow 会抛错，吞掉即可。
    try {
      getCurrentWindow().setAlwaysOnTop(pinned).catch(() => {});
    } catch {
      /* noop */
    }
  }, [pinned]);
  const togglePin = () => {
    setPinned((p) => {
      const next = !p;
      localStorage.setItem(PIN_KEY, next ? "1" : "0");
      return next;
    });
  };

  // 重命名：editingId 为正在编辑的会话 id，draft 为输入内容。
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const startRename = (l: Item) => {
    const cur = l.task_title && l.task_title !== "(未命名会话)" ? l.task_title : "";
    setDraft(cur);
    setEditingId(l.session.id);
  };
  const submitRename = (l: Item) => {
    const t = draft.trim();
    if (t && t !== l.task_title) {
      invoke("rename_session", { cwd: l.cwd, sessionId: l.session.cc_session_id, title: t }).catch(() => {});
    }
    setEditingId(null);
  };

  const shown = data.filter((l) => match(tab, l, hideDays));

  return (
    <div className="sticker">
      {!isMacPanel() && <div className="drag" data-tauri-drag-region />}
      <div className="tabs">
        {TABS.map((t) => {
          const n = data.filter((l) => match(t.key, l, hideDays)).length;
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
        {!isMacPanel() && (
          <span
            className={"stk-pin " + (pinned ? "stk-pin-on" : "")}
            title={pinned ? "已置顶：点击取消" : "置顶窗口"}
            onClick={togglePin}
          >
            <PinIcon pinned={pinned} />
          </span>
        )}
      </div>
      <div className="stk-scroll">
        {shown.length === 0 ? (
          <EmptyState tab={tab} />
        ) : (
          shown.map((l) => {
            const unnamed = !l.task_title || l.task_title === "(未命名会话)";
            const title = unnamed ? "等待首次输入" : l.task_title;
            const sub = l.errored && l.error_label
              ? l.error_label
              : l.current_activity && l.current_activity !== title
              ? l.current_activity
              : null;
            const pct = l.todo_total > 0 ? Math.round((l.todo_done / l.todo_total) * 100) : 0;
            const indicator = !l.connected ? (
              <span className="ring-stop" title="已断开/已停止" />
            ) : l.errored ? (
              <span className="needs-error" title={l.error_raw ?? "会话出错"} />
            ) : l.session.status === "running" ? (
              <RunBadge pct={l.context_pct} />
            ) : l.session.status === "waiting" ? (
              <RunBadge pct={l.context_pct} tone="waiting" />
            ) : (
              <span className="sdot sdot-on" title="在线" />
            );
            return (
              <div
                className="stk-card"
                key={l.session.id}
                onClick={() => {
                  if (l.connected) {
                    // 连接中：跳转到对应 WT 标签页。
                    if (l.pid) invoke("focus_session", { pid: l.pid, title: l.task_title }).catch(() => {});
                  } else if (!l.archived) {
                    // 已断开且未归档：开新 WT 标签页跑 claude --resume 恢复会话。
                    // 归档的会话点击不恢复（归档即收纳，避免误开终端）。
                    invoke("resume_session", { cwd: l.cwd, sessionId: l.session.cc_session_id }).catch(() => {});
                  }
                }}
                style={{ cursor: l.connected || !l.archived ? "pointer" : "default" }}
                title={l.connected ? "点击跳转到该会话的终端" : l.archived ? "" : "点击新建终端恢复该会话"}
              >
                <div className="stk-top">
                  <span className="stk-ind">{indicator}</span>
                  <div className="stk-top-body">
                    <div className="stk-line1">
                      {editingId === l.session.id ? (
                        <input
                          className="stk-edit"
                          autoFocus
                          value={draft}
                          placeholder="输入名称，回车保存"
                          onChange={(e) => setDraft(e.target.value)}
                          onClick={(e) => e.stopPropagation()}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") submitRename(l);
                            else if (e.key === "Escape") setEditingId(null);
                          }}
                          onBlur={() => setEditingId(null)}
                        />
                      ) : (
                        <>
                          <span className="stk-title">{title}</span>
                          <span className="stk-time">{fmtAgo(l.session.last_event_at)}</span>
                          <span
                            className="stk-rename"
                            title="重命名（同步到 Claude）"
                            onClick={(e) => { e.stopPropagation(); startRename(l); }}
                          ><PencilIcon /></span>
                          <span
                            className="stk-arch"
                            title={l.archived ? "取消归档" : "归档"}
                            onClick={(e) => { e.stopPropagation(); invoke("set_archived", { sessionId: l.session.id, archived: !l.archived }).catch(() => {}); }}
                          ><ArchiveIcon archived={l.archived} /></span>
                        </>
                      )}
                    </div>
                    {editingId === l.session.id && l.connected && (
                      <div className="stk-edit-hint">运行中：改名后需在该终端 /resume 才生效</div>
                    )}
                    <div className="stk-line2">
                      <ConnBadge connected={l.connected} />
                      <span className="stk-repo">{l.project_name}</span>
                    </div>
                  </div>
                </div>
                {sub && <div className={"stk-sub" + (l.errored ? " stk-sub-err" : "")} title={l.error_raw ?? undefined}>{sub}</div>}
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
      {!isMacPanel() && (
        <div
          className="resize-grip"
          onMouseDown={(e) => {
            e.preventDefault();
            getCurrentWindow().startResizeDragging("SouthEast").catch(() => {});
          }}
        />
      )}
    </div>
  );
}
