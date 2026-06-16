import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { LiveSession, Settings, getSettings } from "../api";
import { isMacPanel } from "../platform";
import { useT } from "../i18n";
import type { Dict } from "../i18n/zh";

const DAY_MS = 86_400_000;

function fmtAgo(ms: number, t: Dict): string {
  const m = Math.floor((Date.now() - ms) / 60000);
  if (m < 1) return t.time.now;
  if (m < 60) return t.time.minAgo(m);
  const h = Math.floor(m / 60);
  if (h < 24) return t.time.hourAgo(h);
  return t.time.dayAgo(Math.floor(h / 24));
}

function ConnBadge({ connected }: { connected: boolean }) {
  const t = useT();
  return (
    <span className={"conn " + (connected ? "conn-on" : "conn-off")}>
      <svg width="11" height="11" viewBox="0 0 16 16" aria-hidden="true">
        <rect x="1.5" y="2.5" width="13" height="9" rx="1.3" fill="none" stroke="currentColor" strokeWidth="1.4" />
        <line x1="5.5" y1="14" x2="10.5" y2="14" stroke="currentColor" strokeWidth="1.4" />
        {!connected && <line x1="2" y1="13.5" x2="14" y2="2.5" stroke="currentColor" strokeWidth="1.4" />}
      </svg>
      {connected ? t.conn.on : t.conn.off}
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

function StarIcon({ starred }: { starred: boolean }) {
  // lucide star：未星标描边、星标时填充金色以示激活
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill={starred ? "currentColor" : "none"}
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.79 21.55a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.554 10.34a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />
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
    case "waiting": // 举手（待交互）——与 macOS 菜单栏 hand.raised 同隐喻；lucide hand
      return (
        <svg {...common} viewBox="0 0 24 24" fill="none" stroke="currentColor"
          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" />
          <path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" />
          <path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" />
          <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
        </svg>
      );
    case "running": // 循环箭头（运行中）
      return (
        <svg {...common} viewBox="0 0 24 24" fill="none" stroke="currentColor"
          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M3 21v-5h5" />
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
  const t = useT();
  const what = tone === "waiting" ? t.badge.waiting : t.badge.running;
  const label = pct != null ? t.badge.full(what, pct) : what;
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
const STAR_KEY = "cc-kanban-starred";
/** 轻推预览的悬停意图延迟(ms)：光标需在卡片上停留这么久才浮现，
 *  快速划过列表不触发，避免一串卡片预览瞬开瞬关的闪烁。 */
const PREVIEW_DELAY = 250;
const TAB_KEYS: Tab[] = ["all", "waiting", "running", "archived"];

/** 读取已星标会话集合（按 cc_session_id 持久化，跨重启/换库稳定）。 */
function loadStarred(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(STAR_KEY) ?? "[]");
    return new Set(Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

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

function emptyCopy(tab: Tab, t: Dict): { title: string; hint: string | null } {
  switch (tab) {
    case "all": return { title: t.empty.allTitle, hint: t.empty.allHint };
    case "waiting": return { title: t.empty.waitingTitle, hint: t.empty.waitingHint };
    case "running": return { title: t.empty.runningTitle, hint: null };
    case "archived": return { title: t.empty.archivedTitle, hint: t.empty.archivedHint };
  }
}

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
    case "waiting": // 举手（待交互）
      return (
        <svg {...common}>
          <path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" />
          <path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" />
          <path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" />
          <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
        </svg>
      );
    case "running": // 循环箭头
      return (
        <svg {...common}>
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M3 21v-5h5" />
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
  const t = useT();
  const { title, hint } = emptyCopy(tab, t);
  return (
    <div className="stk-empty">
      <span className="stk-empty-icon"><EmptyIcon tab={tab} /></span>
      <div className="stk-empty-title">{title}</div>
      {hint && <div className="stk-empty-hint">{hint}</div>}
    </div>
  );
}

export function Sticker({ data }: { data: Item[] }) {
  const t = useT();
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
    // cleanup 可能先于 listen resolve 执行：用 cancelled 标记，resolve 后立即注销，防监听器泄漏。
    let cancelled = false;
    let un: (() => void) | undefined;
    try {
      listen<Settings>("settings-changed", (e) => setHideDays(e.payload.archive_hide_days))
        .then((f) => {
          if (cancelled) f();
          else un = f;
        })
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（测试/浏览器） */
    }
    return () => {
      cancelled = true;
      try { un?.(); } catch { /* noop */ }
    };
  }, []);

  // 相对时间（fmtAgo）每分钟重算：递增计数触发轻量重渲染。
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setTick((n) => n + 1), 60_000);
    return () => window.clearInterval(id);
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

  // 轻推预览的悬停意图：只有「停留」在某卡片上超过 PREVIEW_DELAY 才显示其预览，
  // 快速划过不触发（避免连串闪烁）；移开即时隐藏。previewId 存停留卡片的 session.id。
  const [previewId, setPreviewId] = useState<number | null>(null);
  const previewTimer = useRef<number | undefined>(undefined);
  const onCardEnter = (id: number) => {
    if (previewTimer.current !== undefined) clearTimeout(previewTimer.current);
    previewTimer.current = window.setTimeout(() => setPreviewId(id), PREVIEW_DELAY);
  };
  const onCardLeave = () => {
    if (previewTimer.current !== undefined) {
      clearTimeout(previewTimer.current);
      previewTimer.current = undefined;
    }
    setPreviewId(null);
  };
  // 卸载时清掉悬而未决的定时器，防泄漏。
  useEffect(() => () => {
    if (previewTimer.current !== undefined) clearTimeout(previewTimer.current);
  }, []);

  // 会话星标：星标的会话永远排到列表最前（跨重启保留）。与「置顶窗口(pin)」是两回事。
  const [starred, setStarred] = useState<Set<string>>(loadStarred);
  const toggleStar = (sid: string) => {
    setStarred((prev) => {
      const next = new Set(prev);
      if (next.has(sid)) next.delete(sid);
      else next.add(sid);
      localStorage.setItem(STAR_KEY, JSON.stringify([...next]));
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

  // 先按当前 tab 过滤，再把星标会话稳定排到最前（Array.sort 稳定，组内保留服务端顺序）。
  const isStarred = (l: Item) => starred.has(l.session.cc_session_id);
  const shown = data
    .filter((l) => match(tab, l, hideDays))
    .sort((a, b) => Number(isStarred(b)) - Number(isStarred(a)));

  return (
    <div className="sticker">
      {!isMacPanel() && <div className="drag" data-tauri-drag-region />}
      <div className="tabs">
        {TAB_KEYS.map((k) => {
          const n = data.filter((l) => match(k, l, hideDays)).length;
          return (
            <span
              key={k}
              className={"stab " + (tab === k ? "stab-on" : "")}
              onClick={() => pick(k)}
            >
              <TabIcon tab={k} />
              {t.tabs[k]}
              <span className="stab-n">{n}</span>
            </span>
          );
        })}
        {!isMacPanel() && (
          <span
            className={"stk-pin " + (pinned ? "stk-pin-on" : "")}
            title={pinned ? t.sticker.pinOn : t.sticker.pinOff}
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
            const title = unnamed ? t.sticker.waitingFirstInput : l.task_title;
            const sub = l.errored && l.error_label
              ? t.errorLabels[l.error_label] ?? l.error_label
              : l.current_activity && l.current_activity !== title
              ? l.current_activity
              : null;
            const pct = l.todo_total > 0 ? Math.round((l.todo_done / l.todo_total) * 100) : 0;
            const indicator = !l.connected ? (
              <span className="ring-stop" title={t.sticker.stopped} />
            ) : l.errored ? (
              <span className="needs-error" title={l.error_raw ?? t.sticker.sessionError} />
            ) : l.session.status === "running" ? (
              <RunBadge pct={l.context_pct} />
            ) : l.session.status === "waiting" ? (
              <RunBadge pct={l.context_pct} tone="waiting" />
            ) : (
              <span className="sdot sdot-on" title={t.sticker.online} />
            );
            return (
              <div
                className={"stk-card" + (isStarred(l) ? " is-star" : "")}
                key={l.session.id}
                onMouseEnter={() => onCardEnter(l.session.id)}
                onMouseLeave={onCardLeave}
                onClick={() => {
                  if (l.connected) {
                    // 连接中：跳转到对应 WT 标签页。
                    if (l.pid)
                      invoke("focus_session", {
                        pid: l.pid,
                        title: l.task_title,
                        cwd: l.cwd,
                        sessionId: l.session.cc_session_id,
                      }).catch(() => {});
                  } else if (!l.archived) {
                    // 已断开且未归档：开新 WT 标签页跑 claude --resume 恢复会话。
                    // 归档的会话点击不恢复（归档即收纳，避免误开终端）。
                    invoke("resume_session", { cwd: l.cwd, sessionId: l.session.cc_session_id }).catch(() => {});
                  }
                }}
                style={{ cursor: l.connected || !l.archived ? "pointer" : "default" }}
                title={l.connected ? t.sticker.jumpToTerminal : l.archived ? "" : t.sticker.resumeInTerminal}
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
                          placeholder={t.sticker.renamePlaceholder}
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
                          <span className="stk-time">{fmtAgo(l.session.last_event_at, t)}</span>
                          <span
                            className={"stk-star" + (isStarred(l) ? " stk-star-on" : "")}
                            title={isStarred(l) ? t.sticker.unstar : t.sticker.star}
                            onClick={(e) => { e.stopPropagation(); toggleStar(l.session.cc_session_id); }}
                          ><StarIcon starred={isStarred(l)} /></span>
                          <span
                            className="stk-rename"
                            title={t.sticker.renameTitle}
                            onClick={(e) => { e.stopPropagation(); startRename(l); }}
                          ><PencilIcon /></span>
                          <span
                            className="stk-arch"
                            title={l.archived ? t.sticker.unarchive : t.sticker.archive}
                            onClick={(e) => { e.stopPropagation(); invoke("set_archived", { sessionId: l.session.id, archived: !l.archived }).catch(() => {}); }}
                          ><ArchiveIcon archived={l.archived} /></span>
                        </>
                      )}
                    </div>
                    {editingId === l.session.id && l.connected && (
                      <div className="stk-edit-hint">{t.sticker.renameHint}</div>
                    )}
                    <div className="stk-line2">
                      <ConnBadge connected={l.connected} />
                      <span className="stk-repo">{l.project_name}</span>
                    </div>
                  </div>
                </div>
                {sub && <div className={"stk-sub" + (l.errored ? " stk-sub-err" : "")} title={l.error_raw ?? undefined}>{sub}</div>}
                {previewId === l.session.id && l.preview && (
                  <div className="stk-preview">
                    <span className="stk-preview-mark">{t.sticker.previewMark}</span>
                    <span className="stk-preview-txt">{l.preview}</span>
                  </div>
                )}
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
