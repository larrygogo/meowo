// 灵动岛（Windows 贴纸的新形态，2026-11-04 用户决定替换贴边缩略条；视觉照用户给的参考图）。
//
// 本组件画「刘海」这一层：收起时的胶囊，或有事时弹出的提醒。展开面板在 island/IslandPanel，
// 完整列表仍是 Sticker；形态切换的窗口尺寸由 App 驱动（island.rs 按中心点对称改尺寸）。
// 刘海纯黑、贴屏幕顶边，两上角向外凹成「耳朵」与屏幕边相接——耳朵在刘海左右各占 ISLAND_EAR，
// 窗口宽度要把它算进去。胶囊在窗口里水平居中：窗口长大时它不挪位，展开动画从它的轮廓长出来。
import { useLayoutEffect, useMemo, useRef } from "react";
import type { IslandAlertEvent } from "../generated/contracts/IslandAlertEvent";
import { useT } from "../i18n";
import { cardTone } from "./sticker/helpers";
import { type Item, UNNAMED_SESSION_SENTINEL } from "./sticker/types";
import { AgentRing, Clock, Eyes, UsageRing, useNow, type IslandTone } from "./island/islandParts";

export type { IslandTone } from "./island/islandParts";
export type IslandCompactStyle = "counts" | "focus" | "minimal";

/// 刘海两侧「耳朵」的宽度（逻辑像素）。窗口宽 = 刘海本体 + 2 × 耳朵。
export const ISLAND_EAR = 14;
/// 收起胶囊的高度。
export const ISLAND_PILL_HEIGHT = 46;
/// 提醒形态的窗口尺寸（含耳朵）。
export const ISLAND_ALERT_SIZE = { width: 520 + ISLAND_EAR * 2, height: 68 } as const;
/// 胶囊上最多摆几个会话图标，再多折成「+N」。
const MAX_AGENTS = 4;

type Counts = { pending: number; error: number; running: number; waiting: number };

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

export function Island({
  sessions,
  compactStyle,
  alert,
  usagePct,
  onMeasure,
  onHoverIntent,
  onActivate,
  onOpenSession,
  onPointerDownDrag,
  onAlertHover,
}: {
  sessions: readonly Item[];
  compactStyle: IslandCompactStyle;
  /** 正在弹出的提醒；null = 收起态。 */
  alert: IslandAlertEvent | null;
  /** 额度环的已用百分比；null = 没有额度数据，不画环。 */
  usagePct: number | null;
  /** 刘海本体（不含耳朵）的实测宽度，App 据此让窗口贴合。 */
  onMeasure: (width: number) => void;
  /** 鼠标在胶囊上停够 250ms（U1-9：屏幕顶边常有鼠标路过，不能一碰就展开）。 */
  onHoverIntent: () => void;
  /** 点一下胶囊（没拖动）。 */
  onActivate: () => void;
  onOpenSession: (sessionId: number) => void;
  /** 按下胶囊：交给 App 判断是点击还是左右拖动。 */
  onPointerDownDrag: (event: React.PointerEvent<HTMLElement>) => void;
  /** 鼠标进出提醒：App 据此暂停/恢复提醒的倒计时。 */
  onAlertHover?: (hovering: boolean) => void;
}) {
  const t = useT();
  const now = useNow();
  const notchRef = useRef<HTMLDivElement>(null);
  const hoverTimer = useRef<number | null>(null);
  const counts = useMemo(() => islandCounts(sessions), [sessions]);
  const tone = islandTone(counts);
  const ordered = useMemo(() => byUrgency(sessions), [sessions]);

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
  }, [alert, compactStyle, onMeasure]);

  const clearHover = () => {
    if (hoverTimer.current != null) window.clearTimeout(hoverTimer.current);
    hoverTimer.current = null;
  };

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

  const compact = () => {
    if (compactStyle === "minimal") {
      return (
        <>
          <Eyes tone={tone} />
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
          <Eyes tone={tone} />
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
    const shown = ordered.slice(0, MAX_AGENTS);
    const more = ordered.length - shown.length;
    return (
      <>
        {usagePct != null && <UsageRing pct={usagePct} />}
        <Eyes tone={tone} />
        <Clock now={now} locale={t.locale} />
        {shown.length > 0 && (
          <span className="isl-agents">
            {shown.map((item) => <AgentRing key={item.session.id} item={item} />)}
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
        className={`isl-notch ${alert ? "is-alert" : "is-compact"}`}
        data-tone={alertTone}
        role="button"
        tabIndex={0}
        aria-label={t.island.ariaLabel}
        onPointerEnter={() => {
          clearHover();
          // 提醒在场时悬停只是「我在看」：留住提醒，不展开（展开会把提醒换成面板）。
          if (alert) {
            onAlertHover?.(true);
            return;
          }
          hoverTimer.current = window.setTimeout(() => { hoverTimer.current = null; onHoverIntent(); }, 250);
        }}
        onPointerLeave={() => {
          clearHover();
          if (alert) onAlertHover?.(false);
        }}
        onPointerDown={(event) => {
          clearHover();
          onPointerDownDrag(event);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onActivate();
          }
        }}
      >
        {alert ? (
          <>
            <Eyes tone={alertTone} small />
            <span className={`isl-alert-icon is-${alertTone}`}><AlertIcon kind={alert.kind} /></span>
            <span className="isl-alert-text">
              <span className="isl-alert-title">
                {alert.title && alert.title !== UNNAMED_SESSION_SENTINEL ? alert.title : t.sticker.waitingFirstInput}
              </span>
              <span className="isl-alert-sub">
                <span className={`isl-state is-${alertTone}`}>{alertLabel(alert)}</span>
                {alert.project && <><span className="isl-sep">·</span>{alert.project}</>}
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
