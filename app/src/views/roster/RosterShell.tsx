// 主窗口的名单外壳（规格 .lead/20261111-roster）：左缘一条黑轨，眼睛站在轮到你的那一行旁边；
// 首屏是固定顺序的 agent 名单，点开一行看它的近况，「进入现场」才是完整的对话窗。
//
// 对话窗（ChatWindow）原样包在里面，名单只是盖在它的对话区上：回到名单时对话窗不卸载，
// 审批租约、托管终端、输入框草稿都还在。
import { createContext, lazy, memo, Suspense, useCallback, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setRosterShowing } from "../../api";
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
import { useApprovalLease, useAttention, useEyesY, useRosterClock, useRosterLease, useRosterRows } from "./useRoster";

// 速览只在点开一行之后才用得上，而且本来就要等一次读取：拆成单独的分包，不压在主窗口首屏的
// 那一包里（打包实测：不拆时对话窗这一包比改前大 3.05%，预算是 3%）。
// 分包拿不到（手机页走网络：资产陈旧、网络抖一下）就不摆这一块，下面的话和「进入现场」照常；
// 不接住的话 React.lazy 把错误抛到窗口级错误页，点开一行整个窗口就没了（做空审查第二轮，问题 3）。
const RosterGlance = lazy(() => import("./RosterGlance").then((m) => ({ default: m.RosterGlance })).catch(() => ({ default: () => null })));

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
  const [sceneId, setSceneIdState] = useState(0);
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

  // 名单上直接对某个 agent 说话：不进现场，由名单底下的对话窗用它自己的发送发出去（见
  // ChatShellBridge.request.send）。发不出去时对话窗让名单让位，人看到现场里的原因。
  const [sent, setSent] = useState<{ id: number; seq: number } | null>(null);
  const say = useCallback((id: number, message: string) => {
    setRequest((prev) => ({ id, seq: (prev?.seq ?? 0) + 1, send: message }));
    setSent((prev) => ({ id, seq: (prev?.seq ?? 0) + 1 }));
  }, []);
  // 出错行的「恢复」：CLI 已经退出的，在名单底下把它拉起来。
  const resume = useCallback((id: number) => {
    setRequest((prev) => ({ id, seq: (prev?.seq ?? 0) + 1, resume: true }));
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

  // 对话窗换了会话（比如在管理页里点了一条）：人已经落在某个现场里，之后再开管理页点「返回」
  // 该回那个现场，不是名单。
  const setSceneId = useCallback((id: number) => {
    pageFromRosterRef.current = false;
    setSceneIdState(id);
  }, []);
  const enter = useCallback((id: number, takeover = false) => {
    pageFromRosterRef.current = false;
    setRequest((prev) => ({ id, seq: (prev?.seq ?? 0) + 1, ...(takeover ? { takeover: true } : null) }));
    setView("scene");
  }, []);
  const backToRoster = useCallback(() => {
    setExpanded(null);
    setView("roster");
  }, []);
  useTauriEvent("chat-open-roster", backToRoster);

  useRosterLease();
  // 告诉后端名单是否摆在眼前（通知据此闭嘴，见 watch.rs suppressed_for）。卸载时收回。
  useEffect(() => {
    setRosterShowing(view === "roster").catch(() => {});
  }, [view]);
  useEffect(() => () => { setRosterShowing(false).catch(() => {}); }, []);

  // 名单里眼睛站在名单行旁边；现场里站在对话窗侧栏那一行（名单缩成的窄列）旁边。行被折叠或
  // 滚出去时停在「回到名单」旁边、神态不变（取舍 4）。
  const attention = useAttention(rows, expanded);
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

  // 名单的数据经 context 送到盖在对话窗里的那一层，不放进 shell：shell 一变整个对话窗（五千多行的
  // 状态机）就要重渲染一遍，而名单每次 board-changed 都在变。量测（visual/perf-roster.mjs，12 个在跑
  // 的会话每 450ms 全部换一句话）：放进 shell 时每次刷新主线程约 6ms，是旧侧栏的 3 倍。
  const toggle = useCallback((id: number) => setExpanded((prev) => (prev === id ? null : id)), []);
  const list = useMemo<RosterListProps>(() => ({
    rows, loaded: sessions != null, expanded, gitBadges, onToggle: toggle, onEnter: enter, onSay: say, onResume: resume, onNewSession: askNewSession, onCommand: run, sent,
  }), [rows, sessions, expanded, gitBadges, toggle, enter, say, resume, askNewSession, run, sent]);
  const shell = useMemo<ChatShellBridge>(() => ({
    overlay: view === "roster" ? ROSTER_OVERLAY : null,
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
  }), [view, request, newSession, command, backToRoster, setSceneId]);
  // 元素本身也要稳住：每次渲染新建一个 <ChatWindow> 元素，React 照样会重渲染它。
  const chat = useMemo(() => <ChatWindow shell={shell} />, [shell]);

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
        <RosterContext.Provider value={list}>{chat}</RosterContext.Provider>
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

type RosterListProps = {
  onCommand: (name: "search" | "done" | "admin") => void;
  onSay: (id: number, message: string) => void;
  onResume: (id: number) => void;
  /** 刚在名单上发出去的那一句是给谁的（输入框上方报一声「已发给…」）。 */
  sent: { id: number; seq: number } | null;
  onNewSession: (message: string) => void;
  rows: readonly Item[];
  loaded: boolean;
  expanded: number | null;
  gitBadges: GitBadgeStore;
  onToggle: (id: number) => void;
  onEnter: (id: number, takeover?: boolean) => void;
};
const RosterContext = createContext<RosterListProps | null>(null);
/** 盖在对话窗里的那一层：自己从 context 取名单数据，对话窗不知道名单变了。元素是模块级常量。 */
function RosterOverlay() {
  const props = useContext(RosterContext);
  return props ? <RosterList {...props} /> : null;
}
const ROSTER_OVERLAY = <RosterOverlay />;

function RosterList({ rows, loaded, expanded, gitBadges, onToggle, onEnter, onSay, onResume, onNewSession, onCommand, sent }: RosterListProps) {
  const t = useT();
  const target = expanded == null ? null : rows.find((row) => row.session.id === expanded) ?? null;
  const clock = useRosterClock();
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
            clock={clock}
            gitBadges={gitBadges}
            onToggle={onToggle}
            onEnter={onEnter}
            onSay={onSay}
            onResume={onResume}
          />
        ))}
      </div>
      {/* 常驻、永远完整可见：名单自己滚，输入框不跟着滚走。 */}
      <RosterComposer
        target={target}
        targetTitle={target ? title(target, t.sticker.waitingFirstInput) : ""}
        sentTo={sent ? { seq: sent.seq, title: title(rows.find((row) => row.session.id === sent.id) ?? ({ task_title: null } as unknown as Item), t.sticker.waitingFirstInput) } : null}
        onSay={onSay}
        onNewSession={onNewSession}
      />
    </section>
  );
}

/** 一行。后端每次刷新给的都是新对象：按内容比，没变的行不重渲染（12 行里通常只有在跑的几行在变）。 */
export const RosterItem = memo(RosterItemView, (a, b) => (
  a.open === b.open && a.clock === b.clock && a.gitBadges === b.gitBadges && a.onToggle === b.onToggle && a.onEnter === b.onEnter && a.onPrefetch === b.onPrefetch
  && a.onSay === b.onSay && a.onResume === b.onResume
  && (a.item === b.item || JSON.stringify(a.item) === JSON.stringify(b.item))
));

function RosterItemView({ item, open, gitBadges, onToggle, onEnter, onPrefetch, onSay, onResume }: {
  /** 出错行的「继续」：在名单上直接对它说一声继续。手机页不传，那里没有这两个按钮。 */
  onSay?: (id: number, message: string) => void;
  /** 出错行的「恢复」：CLI 已经退出的，把它拉起来。 */
  onResume?: (id: number) => void;
  /** 半分钟一格的钟（useRosterClock）：只为让「多久之前」跟着走，组件里不读它。 */
  clock?: number;
  /** 这一行多半接下来要进现场：提前把它的首屏读起来（手机页用，点进去不用再等一次整读）。 */
  onPrefetch?: (id: number) => void;
  item: Item;
  open: boolean;
  gitBadges: GitBadgeStore;
  onToggle: (id: number) => void;
  onEnter: (id: number, takeover?: boolean) => void;
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
  // 展开着的这一行算「正在看」：持有它的专属租约，后端据此清掉「跑完没看」的未读（眼睛才会去下一行），
  // 也不再为它弹系统通知——决定单就在眼前。收起即注销；名单租约还在，它挂着的审批不会被退回终端。
  useApprovalLease(id, open && id > 0);
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
          {/* 最近几步在上、它最后说的话在下，顺着时间读。待处理的行只摆三步：做决定要知道它是怎么走到
              这一步的，但不能把决定单挤到屏幕外面（做空审查第二轮：终态要求「之前几步」在手边）。 */}
          {id > 0 && <Suspense fallback={null}><RosterGlance sessionId={id} count={tone === "pending" ? 3 : undefined} /></Suspense>}
          {/* 出错的行把错误原文摆出来：行里那一句只是个短标签。 */}
          {tone === "error" && item.error_raw && <pre className="roster-error">{item.error_raw}</pre>}
          {/* 它最后说的话是 Markdown 原文：照对话窗那样渲染，不把星号和反引号直接摆出来（真机实拍）。 */}
          {said ? <div className="roster-said chat-md"><ChatMarkdown text={said} /></div> : null}
          {/* 轮到你的行：决定就在这里做（批准 / 拒绝 / 拍板）。答不了的由它说明原因。 */}
          {tone === "pending" && <RosterDecision item={item} fallback={null} onEnter={onEnter} />}
          <div className="roster-foot">
            {/* 出错的行：还活着的给「继续」（对它说一声继续），CLI 已经退出的给「恢复」（关卡 A 取舍 3）。 */}
            {tone === "error" && item.session.status === "ended" && onResume && (
              <button type="button" className="btn-primary roster-enter" onClick={() => onResume(id)}>{t.roster.resume}</button>
            )}
            {tone === "error" && item.session.status !== "ended" && onSay && (
              <button type="button" className="btn-primary roster-enter" onClick={() => onSay(id, t.roster.continuePrompt)}>{t.roster.continue}</button>
            )}
            {/* 决定单自己有主按钮（允许一次 / 提交）时，「进入现场」退成次要按钮：一屏一个主按钮。 */}
            <button type="button" className={tone === "pending" || (tone === "error" && (onSay || onResume)) ? "roster-enter is-quiet" : "btn-primary roster-enter"} onClick={() => onEnter(id)}>{t.roster.enter}</button>
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
