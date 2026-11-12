// 主窗口的名单外壳（规格 .lead/20261111-roster）：左缘一条黑轨，眼睛站在轮到你的那一行旁边；
// 首屏是固定顺序的 agent 名单，点开一行看它的近况，「进入现场」才是完整的对话窗。
//
// 对话窗（ChatWindow）原样包在里面，名单只是盖在它的对话区上：回到名单时对话窗不卸载，
// 审批租约、托管终端、输入框草稿都还在。
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icSearch, icSettings } from "../../icons/glyphs";
import { icList } from "../../icons/glyphs.chat";
import { remoteUi } from "../../remoteMode";
import { ChatMarkdown } from "../ChatMarkdown";
import { runningActivityView } from "../chat/RunningBar";
import { isShellCovered } from "../chat/shellCover";
import { displayKeys } from "../chat/WindowModals";
import { ChatWindow, type ChatShellBridge } from "../ChatWindow";
import { useSidebarGitBadges, type GitBadgeStore } from "../chat/SidebarGitBadge";
import { AgentRing, Eyes } from "../island/islandParts";
import { cardTone, fmtShortAgo } from "../sticker/helpers";
import { UNNAMED_SESSION_SENTINEL, type Item } from "../sticker/types";
import { RosterComposer } from "./RosterComposer";
import { RosterDecision } from "./RosterDecision";
import { RosterGlance } from "./RosterGlance";
import { useAttention, useEyesY, useRosterLease, useRosterRows } from "./useRoster";

/** 眼睛朝右看着那一行。 */
export const GAZE_ROW = { x: 1, y: 0 };

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
  const { sessions, rows } = useRosterRows();
  const [view, setView] = useState<"roster" | "scene">(() => (startsOnRoster() ? "roster" : "scene"));
  const [expanded, setExpanded] = useState<number | null>(null);
  const [sceneId, setSceneId] = useState(0);
  const [request, setRequest] = useState<ChatShellBridge["request"]>(null);
  const [newSession, setNewSession] = useState<ChatShellBridge["newSession"]>(null);
  const [command, setCommand] = useState<ChatShellBridge["command"]>(null);
  // 管理页是从名单进去的：点「返回」回名单。从现场里（侧栏菜单、设置页）进去的照旧回现场。
  const pageFromRosterRef = useRef(false);
  const run = useCallback((name: NonNullable<ChatShellBridge["command"]>["name"]) => {
    if (name !== "search") pageFromRosterRef.current = true;
    setCommand((prev) => ({ name, seq: (prev?.seq ?? 0) + 1 }));
  }, []);
  // Ctrl/Cmd+K 在名单上照样叫出搜索：对话窗自己的那个监听被名单盖住时收不到（chat/shellCover）。
  // 面板已经开着时名单不盖键盘，同键收起由对话窗自己处理，这里不重复。
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!isShellCovered() || event.code !== "KeyK" || !(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
      event.preventDefault();
      run("search");
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [run]);

  const say = useCallback((id: number, draft: string) => {
    setRequest((prev) => ({ id, seq: (prev?.seq ?? 0) + 1, draft }));
    setView("scene");
  }, []);
  const askNewSession = useCallback((message: string) => {
    setNewSession((prev) => ({ message, seq: (prev?.seq ?? 0) + 1 }));
  }, []);

  // 只开了一个 agent：名单只有一行，直接是它的现场。一个都没有时留在名单：空名单加输入框，
  // 从这里派第一个 agent。
  const decidedRef = useRef(false);
  useEffect(() => {
    if (!sessions || decidedRef.current) return;
    decidedRef.current = true;
    if (view === "roster" && sessions.length === 1) setView("scene");
  }, [sessions, view]);

  const enter = useCallback((id: number) => {
    pageFromRosterRef.current = false;
    setRequest((prev) => ({ id, seq: (prev?.seq ?? 0) + 1 }));
    setView("scene");
  }, []);
  const backToRoster = useCallback(() => {
    setExpanded(null);
    setView("roster");
  }, []);
  useTauriEvent("chat-open-roster", backToRoster);

  useRosterLease();

  // 名单里眼睛站在名单行旁边；现场里站在对话窗侧栏那一行（名单缩成的窄列）旁边。行被折叠或
  // 滚出去时停在「回到名单」旁边、神态不变（取舍 4）。
  const attention = useAttention(rows);
  const shellRef = useRef<HTMLDivElement>(null);
  const backRef = useRef<HTMLButtonElement>(null);
  const eyesY = useEyesY(
    shellRef,
    attention.id == null ? null : view === "roster"
      ? `.roster-item[data-session-id="${attention.id}"] .roster-row`
      : `.chat-sidebar-item[data-session-id="${attention.id}"]`,
    backRef,
  );

  // 屏幕上只有一双眼睛：主窗口在前台时眼睛在黑轨上，岛收成细条（App.tsx 听 roster-front）；
  // 窗口不在前台时眼睛回到岛上，黑轨空着。
  const front = useWindowFront();

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
        onSay={say}
        onNewSession={askNewSession}
        onCommand={run}
      />
    ) : null,
    request,
    newSession,
    command,
    onSession: setSceneId,
    onReveal: () => setView("scene"),
    onPageBack: () => {
      if (!pageFromRosterRef.current) return;
      pageFromRosterRef.current = false;
      backToRoster();
    },
  }), [view, rows, sessions, expanded, gitBadges, enter, request, newSession, command, say, askNewSession, run, backToRoster]);

  return (
    <div ref={shellRef} className={`roster-shell is-${view}`}>
      <aside className="roster-rail" aria-label={t.roster.rail}>
        {front && (
          <span className="roster-eyes" style={{ top: eyesY }} data-at={attention.id ?? "home"}>
            <Eyes tone={attention.tone} gaze={attention.id != null ? GAZE_ROW : null} />
          </span>
        )}
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

/** 主窗口是否在前台，并把它广播给灵动岛窗口。查不到窗口（截图页、测试）时按在前台算。 */
function useWindowFront(): boolean {
  const [front, setFront] = useState(true);
  useEffect(() => {
    let alive = true;
    let un: (() => void) | undefined;
    const tell = (focused: unknown) => {
      // 只认明确的布尔值：截图页和测试里的假窗口什么都不回，按「在前台」算。
      if (typeof focused !== "boolean") return;
      if (alive) setFront(focused);
      emit("roster-front", focused).catch(() => {});
    };
    try {
      const win = getCurrentWindow();
      win.isFocused().then(tell).catch(() => {});
      win.onFocusChanged((event) => tell(event.payload)).then((fn) => { if (alive) un = fn; else fn(); }).catch(() => {});
    } catch { /* 不在 Tauri 窗口里 */ }
    // 窗口被销毁时 effect 的清理未必跑得到，页面卸载再补一次：漏了岛会一直是细条。
    const bye = () => { emit("roster-front", false).catch(() => {}); };
    window.addEventListener("pagehide", bye);
    return () => {
      alive = false;
      un?.();
      window.removeEventListener("pagehide", bye);
      bye();
    };
  }, []);
  return front;
}

/** 「此刻在做什么」的那一步，洗成能读的样子：压缩中的哨兵换成文字、剥掉 cd 前缀（同对话窗运行条），
 *  MCP 工具的 `mcp__服务__工具` 写成「服务 · 工具」（同对话窗工具行；真机实拍双下划线像乱码）。 */
function activityText(activity: string | null | undefined, t: ReturnType<typeof useT>): string | null {
  if (!activity) return null;
  const { text } = runningActivityView(activity, t.chat.compacting, t.chat.running);
  const mcp = /^mcp__(.+?)__(.+)$/.exec(text);
  return mcp ? `${mcp[1]} · ${mcp[2]}` : text;
}

function title(item: Item, fallback: string): string {
  return item.task_title && item.task_title !== UNNAMED_SESSION_SENTINEL ? item.task_title : fallback;
}

function RosterList({ rows, loaded, expanded, gitBadges, onToggle, onEnter, onSay, onNewSession, onCommand }: {
  onCommand: (name: "search" | "done" | "admin") => void;
  onSay: (id: number, message: string) => void;
  onNewSession: (message: string) => void;
  rows: readonly Item[];
  loaded: boolean;
  expanded: number | null;
  gitBadges: GitBadgeStore;
  onToggle: (id: number) => void;
  onEnter: (id: number) => void;
}) {
  const t = useT();
  const target = expanded == null ? null : rows.find((row) => row.session.id === expanded) ?? null;
  return (
    <section className="roster" aria-label={t.roster.title}>
      <header className="roster-head" data-tauri-drag-region>
        <h1 className="roster-title">{t.roster.title}</h1>
        {/* 很轻的一排入口，不是页签：已完成和会话管理是另外的页，点了会离开名单。 */}
        <nav className="roster-links" aria-label={t.roster.more}>
          <button type="button" onClick={() => onCommand("done")}>{t.chat.sidebarTabDone}</button>
          <button type="button" onClick={() => onCommand("admin")}>{t.admin.entry}</button>
          <button type="button" className="roster-search" onClick={() => onCommand("search")}>
            <Icon glyph={icSearch} size={14} />
            <span>{t.roster.search}</span>
            <kbd>{displayKeys("Ctrl+K")}</kbd>
          </button>
        </nav>
      </header>
      <div
        className="roster-list"
        role="list"
        // Esc 在名单上只收起展开的那一行，绝不当成「拒绝」（对话窗里 Esc 两下是拒绝，这里没有）。
        onKeyDown={(event) => {
          if (event.key !== "Escape" || expanded == null) return;
          event.stopPropagation();
          onToggle(expanded);
        }}
      >
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
      {/* 常驻、永远完整可见：名单自己滚，输入框不跟着滚走。 */}
      <RosterComposer
        target={target}
        targetTitle={target ? title(target, t.sticker.waitingFirstInput) : ""}
        onSay={onSay}
        onNewSession={onNewSession}
      />
    </section>
  );
}

export function RosterItem({ item, open, gitBadges, onToggle, onEnter, onPrefetch }: {
  /** 这一行多半接下来要进现场：提前把它的首屏读起来（手机页用，点进去不用再等一次整读）。 */
  onPrefetch?: (id: number) => void;
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
    : tone === "running" ? activityText(item.current_activity, t) ?? item.preview : item.preview ?? item.last_ai_text ?? activityText(item.current_activity, t);
  const said = item.last_ai_text ?? item.preview;
  return (
    <div className={`roster-item is-${tone}${open ? " is-open" : ""}`} role="listitem" data-session-id={id}>
      <button
        type="button"
        className="roster-row"
        aria-expanded={open}
        onClick={() => { if (!open) onPrefetch?.(id); onToggle(id); }}
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
          {/* 最近几步在上、它最后说的话在下，顺着时间读。待处理的行不摆：那里要紧的是决定单。 */}
          {tone !== "pending" && id > 0 && <RosterGlance sessionId={id} />}
          {/* 出错的行把错误原文摆出来：行里那一句只是个短标签。 */}
          {tone === "error" && item.error_raw && <pre className="roster-error">{item.error_raw}</pre>}
          {/* 它最后说的话是 Markdown 原文：照对话窗那样渲染，不把星号和反引号直接摆出来（真机实拍）。 */}
          {said ? <div className="roster-said chat-md"><ChatMarkdown text={said} /></div> : null}
          {/* 轮到你的行：决定就在这里做（批准 / 拒绝 / 拍板）。答不了的由它说明原因。 */}
          {tone === "pending" && <RosterDecision item={item} fallback={null} onEnter={onEnter} />}
          <div className="roster-foot">
            {/* 决定单自己有主按钮（允许一次 / 提交）时，「进入现场」退成次要按钮：一屏一个主按钮。 */}
            <button type="button" className={tone === "pending" ? "roster-enter is-quiet" : "btn-primary roster-enter"} onClick={() => onEnter(id)}>{t.roster.enter}</button>
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
