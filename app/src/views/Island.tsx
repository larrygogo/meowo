// 灵动岛（Windows 贴纸的新形态，2026-11-04 用户决定替换贴边缩略条；视觉照用户给的参考图）。
//
// 本组件画「刘海」这一层：收起时的胶囊，或有事时弹出的提醒。展开面板在 island/IslandPanel，
// 完整列表仍是 Sticker；形态切换的窗口尺寸由 App 驱动（island.rs 按中心点对称改尺寸）。
// 刘海纯黑、贴屏幕顶边，两上角向外凹成「耳朵」与屏幕边相接——耳朵在刘海左右各占 ISLAND_EAR，
// 窗口宽度要把它算进去。胶囊在窗口里水平居中：窗口长大时它不挪位，展开动画从它的轮廓长出来。
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { IslandAlertEvent } from "../generated/contracts/IslandAlertEvent";
import { useT } from "../i18n";
import { useSettingsEffect } from "../hooks/useSettings";
import { cardTone, readSidebarGroupMode, readSidebarSortMode, useStarred, type SidebarGroupMode, type SidebarSortMode } from "./sticker/helpers";
import { sidebarDirGroups } from "./sidebarDrag";
import { type Item, UNNAMED_SESSION_SENTINEL } from "./sticker/types";
import { AgentRing, Clock, Eyes, EYES_BLUSH_AFTER_MS, EYES_BUSY_AT, UsageRing, useEyeCue, useNow, type IslandTone } from "./island/islandParts";

export type { IslandTone } from "./island/islandParts";
import type { IslandCompactStyle } from "../api";
export type { IslandCompactStyle };

/// 刘海两侧「耳朵」的宽度（逻辑像素）。窗口宽 = 刘海本体 + 2 × 耳朵。
export const ISLAND_EAR = 14;
/// 收起胶囊的高度。
export const ISLAND_PILL_HEIGHT = 46;
/// 提醒形态的窗口尺寸（含耳朵）。
export const ISLAND_ALERT_SIZE = { width: 520 + ISLAND_EAR * 2, height: 68 } as const;
/// 胶囊上最多摆几个会话图标，再多折成「+N」。
const MAX_AGENTS = 4;

export type IslandCounts = { pending: number; error: number; running: number; waiting: number };
type Counts = IslandCounts;

export function liveSessions(items: readonly Item[]): Item[] {
  return items.filter((item) => !item.archived && item.connected);
}

export function islandCounts(items: readonly Item[]): Counts {
  const counts: Counts = { pending: 0, error: 0, running: 0, waiting: 0 };
  for (const item of liveSessions(items)) {
    const tone = cardTone(item);
    if (tone === "pending" || tone === "error" || tone === "running" || tone === "waiting") counts[tone] += 1;
  }
  return counts;
}

/// macOS 菜单栏图标右边富文本的一段（macos/status_title.rs 画）：文字或 SF Symbol，颜色是系统语义色名。
export type MenubarSegment =
  | { kind: "text"; text: string; color: string }
  | { kind: "symbol"; name: string; color: string };

/// 菜单栏上会话标题最多几个字，再长省略。
const MENUBAR_TITLE_MAX = 16;

/// macOS 菜单栏图标右边显示什么（用户 2026-11-05：「像 QQ 音乐那种」）：像 QQ 音乐显示歌词那样，
/// 显示最要紧的那个会话的标题和状态词，其余会话折成「+N」；没有会话时显示额度。
///
/// 只用菜单栏主文字色（label），不用彩色和发灰的次要色：颜色只放在眼睛上（眼睛就是状态灯），
/// 文字前也不再放状态图标。实拍过绿眼睛 + 灰勾 + 白标题 + 灰状态词，用户嫌「太不和谐」。
/// 显示的会话就是决定眼睛神态的那个（tone 同 islandTone）：眼睛绿了，文字就得是在跑的那个。
export function menubarSegments(
  sessions: readonly Item[],
  tone: IslandTone,
  quotaUsedPct: number | null,
  labels: { toneLabel: Record<string, string>; untitled: string },
): MenubarSegment[] {
  const ordered = byUrgency(sessions);
  const focus = ordered.find((item) => cardTone(item) === tone) ?? ordered[0];
  if (focus) {
    const state = labels.toneLabel[cardTone(focus)] ?? "";
    const chars = Array.from(sessionTitle(focus, labels.untitled));
    const title = chars.length > MENUBAR_TITLE_MAX ? `${chars.slice(0, MENUBAR_TITLE_MAX).join("")}…` : chars.join("");
    const more = ordered.length > 1 ? `  +${ordered.length - 1}` : "";
    return [{ kind: "text", text: `${title}${state ? ` · ${state}` : ""}${more}`, color: "label" }];
  }
  if (quotaUsedPct != null && Number.isFinite(quotaUsedPct)) {
    const used = Math.round(Math.min(100, Math.max(0, quotaUsedPct)));
    return [
      { kind: "symbol", name: "gauge.medium", color: "label" },
      { kind: "text", text: ` ${used}%`, color: "label" },
    ];
  }
  return [];
}

/// 岛的整体情绪：要人处理的优先（出错 > 待处理），其次在跑，再次跑完等输入。眼睛据此变色、变动作。
export function islandTone(counts: Counts): IslandTone {
  if (counts.error > 0) return "error";
  if (counts.pending > 0) return "pending";
  if (counts.running > 0) return "running";
  if (counts.waiting > 0) return "waiting";
  return "idle";
}

function urgency(item: Item): number {
  const tone = cardTone(item);
  if (tone === "pending") return 0;
  if (tone === "error") return 1;
  if (tone === "waiting") return item.unread ? 2 : 3;
  if (tone === "running") return 4;
  return 9;
}

/// 按要紧程度排：待处理 > 出错 > 未读的跑完 > 跑完 > 在跑（同档取最近有动静的）。
export function byUrgency(items: readonly Item[]): Item[] {
  return liveSessions(items)
    .filter((item) => urgency(item) < 9)
    .sort((a, b) => urgency(a) - urgency(b) || b.session.last_event_at - a.session.last_event_at);
}

/// 岛上会话头像的排列：和对话窗侧栏同一个顺序（用户 2026-11-06 实拍：岛上的顺序和侧栏对不上）。
/// 置顶的在前；其余按侧栏选的排序方式，「最近活跃」按最后动静，默认的手动排序按 manual_key（没拖过
/// 就是创建时间倒序）。侧栏按工作区分组时，再按侧栏同一个组序（sidebarDirGroups：手动排序用拖出来的
/// 工作区顺序，最近活动随组内第一条）分组后摊平——只排扁平顺序的话，工作区 A 拖到 B 上面后，
/// B 里排序键更大的会话仍会排在 A 的前面，和侧栏对不上（代码审查指出）。
/// 只排连着的、有状态的会话，与 byUrgency 取同一批。
/// 要紧程度仍决定眼睛神态和「聚焦」样式显示哪个会话，只是不再决定头像的先后。
export function sidebarOrder(
  items: readonly Item[],
  starred: ReadonlySet<string>,
  sortMode: SidebarSortMode,
  groupMode: SidebarGroupMode = "none",
  workspaceOrder: readonly string[] = [],
): Item[] {
  const recent = sortMode === "recent";
  const key = (item: Item) => (recent ? item.session.last_event_at : item.manual_key ?? item.session.started_at * 1024);
  const flat = byUrgency(items).sort((a, b) =>
    Number(starred.has(b.session.cc_session_id)) - Number(starred.has(a.session.cc_session_id))
    || key(b) - key(a)
    || b.session.id - a.session.id);
  if (groupMode !== "dir") return flat;
  return sidebarDirGroups(flat, sortMode, workspaceOrder).flatMap((group) => group.items);
}

/// 岛上按侧栏顺序排会话：收起的胶囊和展开面板共用（用户 2026-11-06：面板里的列表也要和侧栏一致）。
/// 工作区的手动顺序在设置里（workspace_order），侧栏拖分组头写入后广播 settings-changed，岛跟着收。
/// 排序 / 分组方式只存在对话窗的 localStorage 里；岛窗口与它同源（同一个 Tauri 应用的页面），
/// 读到的是同一份。岛跟着会话列表的轮询重读重算，慢一拍无妨。
export function useSidebarOrder(sessions: readonly Item[]): Item[] {
  const { starred } = useStarred();
  const [workspaceOrder, setWorkspaceOrder] = useState<readonly string[]>([]);
  useSettingsEffect((settings) => setWorkspaceOrder(settings?.workspace_order ?? []));
  return useMemo(
    () => sidebarOrder(sessions, starred, readSidebarSortMode(), readSidebarGroupMode(), workspaceOrder),
    [sessions, starred, workspaceOrder],
  );
}

export function focusSession(items: readonly Item[]): Item | null {
  return byUrgency(items)[0] ?? null;
}

export function sessionTitle(item: Item, fallback: string): string {
  return item.task_title && item.task_title !== UNNAMED_SESSION_SENTINEL ? item.task_title : fallback;
}

/// 提醒左侧的状态图标：审批是盾牌、提问是问号、跑完是对勾、出错是感叹号。
function AlertIcon({ kind }: { kind: string }) {
  const path = kind === "question"
    ? "M9.2 9a2.9 2.9 0 1 1 4.1 2.6c-.8.4-1.3 1-1.3 1.9v.5M12 17.2v.1"
    : kind === "waiting"
    ? "M5 12.5l4.2 4.2L19 7"
    : kind === "error"
    ? "M12 7v6M12 16.5v.1"
    : "M12 3l8 4v5c0 4.5-3.3 8-8 9-4.7-1-8-4.5-8-9V7zM12 9v4M12 16.2v.1";
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={path} />
    </svg>
  );
}

/** 极简模式的细条：宽度与高度（逻辑像素）。贴着屏幕顶边，鼠标甩到顶上就能点到。 */
export const ISLAND_STRIP_SIZE = { width: 160, height: 8 } as const;

/** 极简模式收起时的细条（用户 2026-11-05 要求）：只留一条，颜色跟整体状态走；鼠标移上去
 *  变回正常胶囊，点一下展开。 */
export function IslandStrip({
  sessions,
  onPointerDownDrag,
  onActivate,
  onHover,
}: {
  sessions: readonly Item[];
  onPointerDownDrag: (event: React.PointerEvent<HTMLElement>) => void;
  onActivate: () => void;
  /** 鼠标移上细条：App 把它临时换回正常胶囊。 */
  onHover?: () => void;
}) {
  const t = useT();
  const tone = islandTone(islandCounts(sessions));
  return (
    <div
      className="isl-strip"
      data-tone={tone}
      role="button"
      tabIndex={0}
      aria-label={t.island.ariaLabel}
      onPointerEnter={onHover}
      onPointerDown={onPointerDownDrag}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onActivate();
        }
      }}
    />
  );
}

export function Island({
  sessions,
  compactStyle,
  alert,
  usages,
  onMeasure,
  onActivate,
  onOpenSession,
  onPointerDownDrag,
  onAlertHover,
  alertQueued = 0,
  notch,
}: {
  sessions: readonly Item[];
  compactStyle: IslandCompactStyle;
  /** 正在弹出的提醒；null = 收起态。 */
  alert: IslandAlertEvent | null;
  /** 各家额度（已按「正在用的」排好序）；空 = 没有额度数据，不画环。 */
  usages: readonly { provider: string; pct: number }[];
  /** 刘海本体（不含耳朵）的实测宽度，App 据此让窗口贴合。 */
  onMeasure: (width: number) => void;
  /** 点一下胶囊（没拖动）。 */
  onActivate: () => void;
  onOpenSession: (sessionId: number) => void;
  /** 按下胶囊：交给 App 判断是点击还是左右拖动。 */
  onPointerDownDrag: (event: React.PointerEvent<HTMLElement>) => void;
  /** 鼠标进出提醒：App 据此暂停/恢复提醒的倒计时。 */
  onAlertHover?: (hovering: boolean) => void;
  /** 提醒队列里还排着几条（不含正在弹的）。 */
  alertQueued?: number;
  /** macOS 有刘海时刘海的宽高（逻辑点）。给了就把收起的内容分到刘海左右两翼，中间留空：
   *  刘海是摄像头，那块什么也画不出来。 */
  notch?: { width: number; height: number };
}) {
  const t = useT();
  const now = useNow();
  const notchRef = useRef<HTMLDivElement>(null);
  const counts = useMemo(() => islandCounts(sessions), [sessions]);
  const tone = islandTone(counts);
  const ordered = useMemo(() => byUrgency(sessions), [sessions]);
  const listed = useSidebarOrder(sessions);
  // 眼睛的神态：整体状态 + 正在用的那家额度 + 忙不忙 + 一闪而过的表情（跑完、新会话、鼠标移上来）。
  const { cue, poke } = useEyeCue(sessions);
  const eyes = { tone, cue, quotaPct: usages[0]?.pct ?? null, busy: counts.running >= EYES_BUSY_AT, errors: counts.error };
  const lastWink = useRef(0);
  // 鼠标在岛上停一会儿（没点开）就害羞脸红；移开就取消。
  const blushTimer = useRef(0);
  useEffect(() => () => window.clearTimeout(blushTimer.current), []);

  // 收起态的宽度随内容变（会话个数、标题长短），实测后上报。提醒形态尺寸固定，不量。
  useLayoutEffect(() => {
    if (alert) return;
    const el = notchRef.current;
    if (!el) return;
    const report = () => onMeasure(Math.ceil(el.getBoundingClientRect().width));
    report();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [alert, compactStyle, onMeasure, notch?.width, notch?.height]);

  const alertLabel = (event: IslandAlertEvent) => {
    switch (event.kind) {
      case "approval": return t.pending.approval;
      case "question": return t.pending.question;
      case "plan": return t.pending.plan;
      case "blocked": return t.pending.blocked;
      case "error": return event.detail || t.chat.status.error;
      default: return t.island.finished;
    }
  };
  const alertTone: IslandTone = alert
    ? alert.kind === "error" ? "error" : alert.kind === "waiting" ? "waiting" : "pending"
    : tone;

  // 刘海两翼（macOS 有刘海时）：左翼放额度环和眼睛，右翼放会话。时间不放，菜单栏右上角本来就有。
  // 图标缩到能塞进刘海高度（约 32~38pt）。
  const notchWings = (): [React.ReactNode, React.ReactNode] => {
    if (compactStyle === "minimal") {
      return [
        <Eyes key="eyes" {...eyes} small />,
        <span key="dots" className="isl-dots">
          {liveSessions(sessions).slice(0, 6).map((item) => (
            <span key={item.session.id} className={`isl-dot is-${cardTone(item)}`} />
          ))}
        </span>,
      ];
    }
    if (compactStyle === "focus") {
      const focus = ordered[0];
      return [
        <Eyes key="eyes" {...eyes} small />,
        focus
          ? <span key="focus" className={`isl-state is-${cardTone(focus)}`}>{t.island.toneLabel[cardTone(focus)] ?? ""}</span>
          : null,
      ];
    }
    const shown = listed.slice(0, MAX_AGENTS);
    const more = listed.length - shown.length;
    return [
      <>
        {usages.length > 0 && <UsageRing entries={usages} size={22} />}
        <Eyes {...eyes} small />
      </>,
      shown.length > 0 ? (
        <span className="isl-agents">
          {shown.map((item) => (
            <AgentRing
              key={item.session.id}
              item={item}
              size={22}
              tip={`${sessionTitle(item, t.sticker.waitingFirstInput)} · ${t.island.toneLabel[cardTone(item)] ?? ""}`}
            />
          ))}
          {more > 0 && <span className="isl-more">+{more}</span>}
        </span>
      ) : null,
    ];
  };

  const compact = () => {
    if (notch) {
      const [left, right] = notchWings();
      return (
        <>
          <span className="isl-wing is-left">{left}</span>
          <span className="isl-notch-gap" aria-hidden="true" />
          <span className="isl-wing is-right">{right}</span>
        </>
      );
    }
    if (compactStyle === "minimal") {
      return (
        <>
          <Eyes {...eyes} />
          <span className="isl-dots">
            {liveSessions(sessions).slice(0, 6).map((item) => (
              <span key={item.session.id} className={`isl-dot is-${cardTone(item)}`} />
            ))}
          </span>
        </>
      );
    }
    if (compactStyle === "focus") {
      const focus = ordered[0];
      return (
        <>
          <Eyes {...eyes} />
          {focus ? (
            <span className="isl-focus">
              <span className="isl-focus-title">{sessionTitle(focus, t.sticker.waitingFirstInput)}</span>
              <span className={`isl-state is-${cardTone(focus)}`}>{t.island.toneLabel[cardTone(focus)] ?? ""}</span>
            </span>
          ) : <span className="isl-faint">{t.island.idle}</span>}
        </>
      );
    }
    // 概览（默认）：额度环 · 眼睛 · 时间 · 每个会话一颗带进度环的 agent 图标。
    const shown = listed.slice(0, MAX_AGENTS);
    const more = listed.length - shown.length;
    return (
      <>
        {usages.length > 0 && <UsageRing entries={usages} />}
        <Eyes {...eyes} />
        <Clock now={now} locale={t.locale} />
        {shown.length > 0 && (
          <span className="isl-agents">
            {/* 悬停提示「标题 · 状态」：一排同家的图标长得一样，不提示分不清谁是谁。 */}
            {shown.map((item) => (
              <AgentRing
                key={item.session.id}
                item={item}
                tip={`${sessionTitle(item, t.sticker.waitingFirstInput)} · ${t.island.toneLabel[cardTone(item)] ?? ""}`}
              />
            ))}
            {more > 0 && <span className="isl-more">+{more}</span>}
          </span>
        )}
      </>
    );
  };

  return (
    <div className="isl-stage">
      <div
        ref={notchRef}
        className={`isl-notch ${alert ? "is-alert" : "is-compact"}${notch && !alert ? " is-split" : ""}`}
        style={notch && !alert
          ? { "--isl-notch-w": `${notch.width}px`, "--isl-notch-h": `${notch.height}px` } as React.CSSProperties
          : undefined}
        data-tone={alertTone}
        role="button"
        tabIndex={0}
        aria-label={t.island.ariaLabel}
        // 只点击展开，悬停不展开（用户要求，2026-11-04）。提醒在场时悬停是「我在看」：
        // 留住提醒、暂停倒计时。
        // 收起时鼠标移上来，眼睛冲你眨一下（隔 8 秒以上才再眨，来回划过不至于一直眨）。
        onPointerEnter={() => {
          if (alert) {
            onAlertHover?.(true);
          } else if (Date.now() - lastWink.current > 8_000) {
            lastWink.current = Date.now();
            poke("wink");
          }
          if (!alert) {
            window.clearTimeout(blushTimer.current);
            blushTimer.current = window.setTimeout(() => poke("blush"), EYES_BLUSH_AFTER_MS);
          }
        }}
        onPointerLeave={() => {
          window.clearTimeout(blushTimer.current);
          if (alert) onAlertHover?.(false);
        }}
        onPointerDown={onPointerDownDrag}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onActivate();
          }
        }}
      >
        {alert ? (
          <>
            <Eyes tone={alertTone} cue={cue} small />
            <span className={`isl-alert-icon is-${alertTone}`}><AlertIcon kind={alert.kind} /></span>
            <span className="isl-alert-text">
              <span className="isl-alert-title-row">
                <span className="isl-alert-title">
                  {alert.title && alert.title !== UNNAMED_SESSION_SENTINEL ? alert.title : t.sticker.waitingFirstInput}
                </span>
                {alertQueued > 0 && <span className="isl-alert-more">{t.island.moreAlerts(alertQueued)}</span>}
              </span>
              <span className="isl-alert-sub">
                <span className={`isl-state is-${alertTone}`}>{alertLabel(alert)}</span>
                {/* 跑完的提醒带 agent 最后一句话（后端取的第一行），比项目名更能说明它做完了什么。 */}
                {alert.kind === "waiting" && alert.detail
                  ? <><span className="isl-sep">·</span><span className="isl-alert-detail" title={alert.detail}>{alert.detail}</span></>
                  : alert.project && <><span className="isl-sep">·</span>{alert.project}</>}
              </span>
            </span>
            <button
              type="button"
              className="isl-btn is-primary"
              // 按钮上按下不该开始拖动，也不该冒泡成「点胶囊 = 展开」。
              onPointerDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.stopPropagation();
                onOpenSession(alert.sessionId);
              }}
            >{t.island.open}</button>
          </>
        ) : compact()}
      </div>
    </div>
  );
}
