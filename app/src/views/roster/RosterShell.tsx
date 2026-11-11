// 主窗口的名单外壳（规格 .lead/20261111-roster）：左缘一条黑轨，眼睛站在轮到你的那一行旁边；
// 首屏是固定顺序的 agent 名单，点开一行看它的近况，「进入现场」才是完整的对话窗。
//
// 对话窗（ChatWindow）原样包在里面，名单只是盖在它的对话区上：回到名单时对话窗不卸载，
// 审批租约、托管终端、输入框草稿都还在。
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getLiveSessionsPage } from "../../api";
import { useBoardRefresh } from "../../hooks/useBoardRefresh";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icSettings } from "../../icons/glyphs";
import { icList } from "../../icons/glyphs.chat";
import { remoteUi } from "../../remoteMode";
import { ChatWindow, type ChatShellBridge } from "../ChatWindow";
import { useSidebarGitBadges, type GitBadgeStore } from "../chat/SidebarGitBadge";
import { AgentRing, Eyes } from "../island/islandParts";
import { cardTone, fmtShortAgo, useStarred } from "../sticker/helpers";
import { UNNAMED_SESSION_SENTINEL, type Item } from "../sticker/types";
import { attentionFocus } from "./attention";

/** 名单一次取多少行。名单不翻页：超过这个数的老会话去「已完成 / 工作区」里找。 */
const ROSTER_LIMIT = 200;
/** 眼睛回轨顶时离轨顶边的距离（眼睛中心，px）。 */
const EYES_HOME_Y = 20;
/** 眼睛朝右看着那一行。 */
const GAZE_ROW = { x: 1, y: 0 };

/** 手机远程页先不套壳（规格里程碑 5 再做同构）。 */
export function RosterShell() {
  return remoteUi() ? <ChatWindow /> : <DesktopRoster />;
}

function startsOnRoster(): boolean {
  const q = new URLSearchParams(window.location.search);
  // 托盘 / 菜单栏打开带 roster=1；点具体会话、新建页、管理页都是冲着那一页来的，直接进现场。
  if (q.get("roster") === "1") return true;
  return !q.get("sessionId") && !q.get("newSession") && !q.get("admin");
}

function DesktopRoster() {
  const t = useT();
  const [sessions, setSessions] = useState<Item[] | null>(null);
  const [view, setView] = useState<"roster" | "scene">(() => (startsOnRoster() ? "roster" : "scene"));
  const [expanded, setExpanded] = useState<number | null>(null);
  const [sceneId, setSceneId] = useState(0);
  const [request, setRequest] = useState<ChatShellBridge["request"]>(null);
  const { starred } = useStarred();

  const load = useCallback(() => {
    // 手动顺序：没拖过就是创建时间倒序，任何事件都不改它（规格：顺序不随事件跳动）。
    getLiveSessionsPage("all", null, null, ROSTER_LIMIT, null, false, false, "manual")
      .then((page) => setSessions(page.items))
      .catch(() => {});
  }, []);
  useBoardRefresh(load);
  useEffect(load, [load]);

  // 置顶的在前（人自己动的），其余保持后端给的顺序。
  const rows = useMemo(() => {
    if (!sessions) return [];
    const pinned = sessions.filter((s) => starred.has(s.session.cc_session_id));
    return pinned.length ? [...pinned, ...sessions.filter((s) => !starred.has(s.session.cc_session_id))] : sessions;
  }, [sessions, starred]);

  // 只开了一个 agent（或一个都没有）：名单没有东西可列，直接是现场。
  const decidedRef = useRef(false);
  useEffect(() => {
    if (!sessions || decidedRef.current) return;
    decidedRef.current = true;
    if (view === "roster" && sessions.length <= 1) setView("scene");
  }, [sessions, view]);

  const enter = useCallback((id: number) => {
    setRequest((prev) => ({ id, seq: (prev?.seq ?? 0) + 1 }));
    setView("scene");
  }, []);
  const backToRoster = useCallback(() => {
    setExpanded(null);
    setView("roster");
  }, []);
  useTauriEvent("chat-open-roster", backToRoster);

  // 眼睛站谁那里。stickyRef 记着它此刻站的会话：它还需要你就不换（新来的不抢）。
  const stickyRef = useRef<number | null>(null);
  const attention = useMemo(() => attentionFocus(rows, stickyRef.current), [rows]);
  stickyRef.current = attention.id;

  const shellRef = useRef<HTMLDivElement>(null);
  const backRef = useRef<HTMLButtonElement>(null);
  const [eyesY, setEyesY] = useState(EYES_HOME_Y);
  const place = useCallback(() => {
    const root = shellRef.current;
    if (!root) return;
    const top = root.getBoundingClientRect().top;
    const centerOf = (el: Element | null | undefined) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return r.height > 0 ? r.top - top + r.height / 2 : null;
    };
    let y: number | null = null;
    if (attention.id != null) {
      // 名单里是名单行；现场里是对话窗侧栏那一行（名单缩成的窄列）。行被折叠或滚出去时，
      // 眼睛停在「回到名单」旁边、神态不变（取舍 4）。
      const selector = view === "roster"
        ? `.roster-item[data-session-id="${attention.id}"] .roster-row`
        : `.chat-sidebar-item[data-session-id="${attention.id}"]`;
      y = centerOf(root.querySelector(selector)) ?? centerOf(backRef.current);
    }
    const max = root.clientHeight - 96;
    const next = Math.round(Math.max(EYES_HOME_Y, Math.min(y ?? EYES_HOME_Y, max)));
    setEyesY((prev) => (prev === next ? prev : next));
  }, [attention.id, view]);
  useLayoutEffect(place);
  useEffect(() => {
    const root = shellRef.current;
    if (!root) return;
    // 老内核 / 测试环境没有 ResizeObserver：退化成只在渲染和滚动时重新量。
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    observer?.observe(root);
    // 名单或侧栏滚动时眼睛跟着那一行走（scroll 不冒泡，用捕获）。
    root.addEventListener("scroll", place, true);
    return () => {
      observer?.disconnect();
      root.removeEventListener("scroll", place, true);
    };
  }, [place]);

  const gitBadges = useSidebarGitBadges(useMemo(() => rows.map((row) => row.cwd), [rows]));

  const shell = useMemo<ChatShellBridge>(() => ({
    overlay: view === "roster" ? (
      <RosterList
        rows={rows}
        loaded={sessions != null}
        expanded={expanded}
        gitBadges={gitBadges}
        onToggle={(id) => setExpanded((prev) => (prev === id ? null : id))}
        onEnter={enter}
      />
    ) : null,
    request,
    onSession: setSceneId,
    onReveal: () => setView("scene"),
  }), [view, rows, sessions, expanded, gitBadges, enter, request]);

  return (
    <div ref={shellRef} className={`roster-shell is-${view}`}>
      <aside className="roster-rail" aria-label={t.roster.rail}>
        <span className="roster-eyes" style={{ top: eyesY }} data-at={attention.id ?? "home"}>
          <Eyes tone={attention.tone} gaze={attention.id != null ? GAZE_ROW : null} />
        </span>
        <div className="roster-rail-foot">
          {view === "scene" && rows.length > 1 && (
            <button ref={backRef} type="button" className="roster-rail-btn" aria-label={t.roster.back} data-tip={t.roster.back} onClick={backToRoster}>
              <Icon glyph={icList} size={16} />
            </button>
          )}
          <button type="button" className="roster-rail-btn" aria-label={t.sticker.openSettings} data-tip={t.sticker.openSettings} onClick={() => { invoke("open_settings").catch(() => {}); }}>
            <Icon glyph={icSettings} size={16} />
          </button>
        </div>
      </aside>
      <div className="roster-stage" data-scene-id={sceneId}>
        <ChatWindow shell={shell} />
      </div>
    </div>
  );
}

function title(item: Item, fallback: string): string {
  return item.task_title && item.task_title !== UNNAMED_SESSION_SENTINEL ? item.task_title : fallback;
}

function RosterList({ rows, loaded, expanded, gitBadges, onToggle, onEnter }: {
  rows: readonly Item[];
  loaded: boolean;
  expanded: number | null;
  gitBadges: GitBadgeStore;
  onToggle: (id: number) => void;
  onEnter: (id: number) => void;
}) {
  const t = useT();
  return (
    <section className="roster" aria-label={t.roster.title}>
      <header className="roster-head" data-tauri-drag-region>
        <h1 className="roster-title">{t.roster.title}</h1>
      </header>
      <div className="roster-list" role="list">
        {loaded && rows.length === 0 && <p className="roster-empty">{t.empty.allTitle}</p>}
        {rows.map((item) => (
          <RosterItem
            key={item.session.id}
            item={item}
            open={expanded === item.session.id}
            gitBadges={gitBadges}
            onToggle={onToggle}
            onEnter={onEnter}
          />
        ))}
      </div>
    </section>
  );
}

function RosterItem({ item, open, gitBadges, onToggle, onEnter }: {
  item: Item;
  open: boolean;
  gitBadges: GitBadgeStore;
  onToggle: (id: number) => void;
  onEnter: (id: number) => void;
}) {
  const t = useT();
  const id = item.session.id;
  const tone = cardTone(item);
  // 状态词：待处理说清是哪一种（待批准 / 待回答 / 待拍板）；屏幕上认出来但说不出哪种的是「待处理」。
  const word = tone === "pending" ? t.pending[item.pending_review ?? "blocked"] : t.island.toneLabel[tone];
  // 此刻在做什么：在跑的给它正在做的那一步；出错给错误；其余给它最后说的话。
  const now = tone === "error"
    ? item.error_label
    : tone === "running" ? item.current_activity ?? item.preview : item.preview ?? item.last_ai_text ?? item.current_activity;
  const said = item.last_ai_text ?? item.preview;
  return (
    <div className={`roster-item is-${tone}${open ? " is-open" : ""}`} role="listitem" data-session-id={id}>
      <button
        type="button"
        className="roster-row"
        aria-expanded={open}
        onClick={() => onToggle(id)}
        onDoubleClick={() => onEnter(id)}
      >
        <span className="roster-av"><AgentRing item={item} size={32} /></span>
        <span className="roster-name">
          <b>{title(item, t.sticker.waitingFirstInput)}</b>
          <i>{item.project_name}</i>
        </span>
        <span className={`roster-word is-${tone}`}>{word}</span>
        <span className={`roster-now${tone === "running" ? " is-mono" : ""}`}>{now}</span>
        <span className="roster-time">{fmtShortAgo(item.session.last_event_at, t)}</span>
        <RosterDiff store={gitBadges} cwd={item.cwd} />
      </button>
      {open && (
        <div className="roster-sheet">
          {said ? <p className="roster-said">{said}</p> : <p className="roster-said is-empty">{t.roster.nothingSaid}</p>}
          <div className="roster-foot">
            <button type="button" className="btn-primary roster-enter" onClick={() => onEnter(id)}>{t.roster.enter}</button>
          </div>
        </div>
      )}
    </div>
  );
}

/** 未提交的增删行数。每个格子自己订阅摘要仓库：结果回来只重画有变化的那一格，整张名单不动。 */
function RosterDiff({ store, cwd }: { store: GitBadgeStore; cwd: string | null | undefined }) {
  const badge = useSyncExternalStore(store.subscribe, () => (cwd ? store.get(cwd) : undefined));
  const lines = badge ? badge.additions + badge.deletions : 0;
  return (
    <span className="roster-diff">
      {lines > 0 && badge && <><span className="roster-add">+{badge.additions}</span><span className="roster-del">−{badge.deletions}</span></>}
    </span>
  );
}
