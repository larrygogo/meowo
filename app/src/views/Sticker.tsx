import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { LiveSession, Settings, TerminalOpenMode, getSettings } from "../api";
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

function OpenIcon() {
  // lucide square-arrow-out-up-right：从方框向外跳出的箭头，表达「打开/跳转终端」
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h6" />
      <path d="M15 3h6v6" />
      <path d="m10 14 11-11" />
    </svg>
  );
}

function NoteIcon() {
  // lucide sticky-note：折角便签纸，区别于 rename 的铅笔
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M16 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11l5-5V5a2 2 0 0 0-2-2z" />
      <path d="M15 21v-5a1 1 0 0 1 1-1h5" />
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

  // 归档自动隐藏天数 + 打开终端方式：启动时读设置，并监听设置窗口的实时变更。
  const [hideDays, setHideDays] = useState(0);
  const [openMode, setOpenMode] = useState<TerminalOpenMode>("card");
  const [previewEnabled, setPreviewEnabled] = useState(true);
  useEffect(() => {
    const apply = (s: Settings) => {
      setHideDays(s.archive_hide_days);
      setOpenMode(s.terminal_open_mode);
      setPreviewEnabled(s.preview_enabled);
    };
    getSettings().then(apply).catch(() => {});
    // cleanup 可能先于 listen resolve 执行：用 cancelled 标记，resolve 后立即注销，防监听器泄漏。
    let cancelled = false;
    let un: (() => void) | undefined;
    try {
      listen<Settings>("settings-changed", (e) => apply(e.payload))
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
    setNotingId(null); // 与便签编辑互斥，同卡只开一个编辑器
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

  // 便签编辑：notingId 为正在编辑便签的会话 id，noteDraft 为输入内容。与重命名互斥（同卡只开一个）。
  const [notingId, setNotingId] = useState<number | null>(null);
  const [noteDraft, setNoteDraft] = useState("");
  const startNote = (l: Item) => {
    setEditingId(null);
    setNoteDraft(l.note ?? "");
    setNotingId(l.session.id);
  };
  const submitNote = (l: Item) => {
    if (noteDraft !== (l.note ?? "")) {
      invoke("set_session_note", { sessionId: l.session.cc_session_id, note: noteDraft }).catch(() => {});
    }
    setNotingId(null);
  };

  // 打开终端：连接中→跳转 WT 标签页；已断开未归档→新建终端 resume；归档不开。
  const buttonMode = openMode === "button";
  const canOpen = (l: Item) => l.connected || !l.archived;
  const openTerminal = (l: Item) => {
    if (l.connected) {
      if (l.pid)
        invoke("focus_session", {
          pid: l.pid,
          title: l.task_title,
          cwd: l.cwd,
          sessionId: l.session.cc_session_id,
        }).catch(() => {});
    } else if (!l.archived) {
      invoke("resume_session", { cwd: l.cwd, sessionId: l.session.cc_session_id }).catch(() => {});
    }
  };

  // 先按当前 tab 过滤，再排序：星标恒在最前；「待交互」标签内按等待最久优先（先处理被晾最久的）；
  // 其它标签保留服务端顺序（连接中优先 → 最近活跃）。Array.sort 稳定，组内次序不乱。
  // useMemo 缓存：编辑便签/重命名时每次按键都会重渲染，不必每次重跑 filter+sort。
  const isStarred = (l: Item) => starred.has(l.session.cc_session_id);
  const shown = useMemo(
    () =>
      data
        .filter((l) => match(tab, l, hideDays))
        .sort((a, b) => {
          const star =
            Number(starred.has(b.session.cc_session_id)) -
            Number(starred.has(a.session.cc_session_id));
          if (star !== 0) return star;
          if (tab === "waiting") return a.session.last_event_at - b.session.last_event_at;
          return 0;
        }),
    [data, tab, hideDays, starred]
  );

  // 各标签角标计数：同样随每次按键重渲染，缓存避免对 4 个标签各跑一遍全量 filter。
  const counts = useMemo(() => {
    const c = {} as Record<Tab, number>;
    for (const k of TAB_KEYS) c[k] = data.filter((l) => match(k, l, hideDays)).length;
    return c;
  }, [data, hideDays]);

  return (
    <div className="sticker">
      {!isMacPanel() && <div className="drag" data-tauri-drag-region />}
      <div className="tabs">
        {TAB_KEYS.map((k) => {
          const n = counts[k];
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
            // 活动行统一显示「最近一条 AI 正文」(preview)；出错优先显示错误标签；
            // previewEnabled 关闭则不显示 AI 正文（仅保留错误）。不再显示底层 Bash 命令。
            const sub = l.errored && l.error_label
              ? t.errorLabels[l.error_label] ?? l.error_label
              : previewEnabled && l.preview
              ? l.preview
              : null;
            const subTitle = l.errored ? l.error_raw ?? undefined : sub ?? undefined;
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
                onClick={() => {
                  // 编辑态(重命名/便签)下，点击卡片仅用于关闭编辑器（失焦），绝不导航开终端。
                  // 注：编辑输入框已 stopPropagation，这里只会被「点击卡片空白处」触发。
                  if (editingId !== null || notingId !== null) {
                    setEditingId(null);
                    setNotingId(null);
                    return;
                  }
                  // 按钮模式：点击卡片不开终端，改由卡片上的打开按钮触发。
                  if (buttonMode) return;
                  openTerminal(l);
                }}
                style={{ cursor: !buttonMode && canOpen(l) ? "pointer" : "default" }}
                title={buttonMode ? "" : l.connected ? t.sticker.jumpToTerminal : l.archived ? "" : t.sticker.resumeInTerminal}
              >
                <div className="stk-top">
                  <span className="stk-ind">{indicator}</span>
                  <div className="stk-top-body">
                    <div className="stk-line1">
                      {editingId === l.session.id ? (
                        <div className="stk-edit-row" onClick={(e) => e.stopPropagation()}>
                          <input
                            className="stk-edit"
                            autoFocus
                            value={draft}
                            placeholder={t.sticker.renamePlaceholder}
                            onChange={(e) => setDraft(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") submitRename(l);
                              else if (e.key === "Escape") setEditingId(null);
                            }}
                          />
                          <button
                            type="button"
                            className="stk-btn-save"
                            onMouseDown={(e) => e.preventDefault()}
                            onClick={() => submitRename(l)}
                          >{t.sticker.noteSave}</button>
                          <button
                            type="button"
                            className="stk-btn-cancel"
                            onMouseDown={(e) => e.preventDefault()}
                            onClick={() => setEditingId(null)}
                          >{t.sticker.noteCancel}</button>
                        </div>
                      ) : (
                        <>
                          <span className="stk-title">{title}</span>
                          <span className="stk-time">{fmtAgo(l.session.last_event_at, t)}</span>
                          {/* 操作按钮默认收起，hover 卡片才浮现，避免每张卡 4 个图标拥挤。
                              星标态由卡片金边、便签由便签块表达，静止时藏图标不丢信息。 */}
                          <span className="stk-actions">
                            <span
                              className={"stk-star" + (isStarred(l) ? " stk-star-on" : "")}
                              title={isStarred(l) ? t.sticker.unstar : t.sticker.star}
                              onClick={(e) => { e.stopPropagation(); toggleStar(l.session.cc_session_id); }}
                            ><StarIcon starred={isStarred(l)} /></span>
                            <span
                              className={"stk-noteb" + (l.note ? " stk-noteb-on" : "")}
                              title={l.note ? t.sticker.noteEdit : t.sticker.noteAdd}
                              onClick={(e) => { e.stopPropagation(); startNote(l); }}
                            ><NoteIcon /></span>
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
                          </span>
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
                {notingId === l.session.id ? (
                  <div className="stk-note-editbox" onClick={(e) => e.stopPropagation()}>
                    <input
                      className="stk-note-edit"
                      autoFocus
                      value={noteDraft}
                      placeholder={t.sticker.notePlaceholder}
                      onChange={(e) => setNoteDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") submitNote(l);
                        else if (e.key === "Escape") setNotingId(null);
                      }}
                    />
                    <div className="stk-note-actions">
                      {/* mousedown preventDefault：点按钮不抢走输入框焦点，避免触发其它失焦逻辑 */}
                      <button
                        type="button"
                        className="stk-btn-save"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => submitNote(l)}
                      >{t.sticker.noteSave}</button>
                      <button
                        type="button"
                        className="stk-btn-cancel"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => setNotingId(null)}
                      >{t.sticker.noteCancel}</button>
                    </div>
                  </div>
                ) : l.note ? (
                  <div
                    className="stk-note"
                    title={t.sticker.noteEdit}
                    onClick={(e) => { e.stopPropagation(); startNote(l); }}
                  >
                    <span className="stk-note-icon"><NoteIcon /></span>
                    <span className="stk-note-txt">{l.note}</span>
                  </div>
                ) : null}
                {(sub || (buttonMode && canOpen(l))) && (
                  <div className="stk-subrow">
                    {/* 活动行：最近一条 AI 正文(或错误标签)，单行截断；title 给完整文本，hover 原生提示可读全文 */}
                    {sub && <span className={"stk-sub" + (l.errored ? " stk-sub-err" : "")} title={subTitle}>{sub}</span>}
                    {/* 按钮模式：打开终端按钮内联在该行末尾，不突兀 */}
                    {buttonMode && canOpen(l) && (
                      <button
                        type="button"
                        className="stk-open"
                        title={l.connected ? t.sticker.jumpToTerminal : t.sticker.resumeInTerminal}
                        onClick={(e) => { e.stopPropagation(); openTerminal(l); }}
                      ><OpenIcon /></button>
                    )}
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
