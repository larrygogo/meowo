// 灵动岛（Windows 贴纸的新形态，2026-11-04 用户决定替换贴边缩略条）。
//
// 本组件只画「胶囊」这一层：收起时的状态摘要，或有事时弹出的提醒。展开成完整列表由
// App 渲染 Sticker 完成，形态切换的窗口尺寸也由 App 驱动（island.rs 按中心点对称改尺寸）。
// 胶囊在窗口里水平居中：窗口长大/缩小时它在屏幕上的位置不动，展开动画从它的轮廓长出来。
import { useLayoutEffect, useMemo, useRef } from "react";
import type { IslandAlertEvent } from "../generated/contracts/IslandAlertEvent";
import { useT } from "../i18n";
import { cardTone } from "./sticker/helpers";
import { type Item, UNNAMED_SESSION_SENTINEL } from "./sticker/types";

export type IslandCompactStyle = "counts" | "focus" | "minimal";
export type IslandTone = "error" | "pending" | "running" | "waiting" | "idle";

/// 胶囊收起时的高度（逻辑像素）。App 按它和测得的宽度摆窗口。
export const ISLAND_PILL_HEIGHT = 36;
/// 提醒形态的尺寸。
export const ISLAND_ALERT_SIZE = { width: 380, height: 64 } as const;

type Counts = { pending: number; error: number; running: number; waiting: number };

function live(items: readonly Item[]): Item[] {
  return items.filter((item) => !item.archived && item.connected);
}

export function islandCounts(items: readonly Item[]): Counts {
  const counts: Counts = { pending: 0, error: 0, running: 0, waiting: 0 };
  for (const item of live(items)) {
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

/// 「最要紧的那一个」：待处理 > 出错 > 未读的跑完 > 跑完 > 在跑（同档取最近有动静的）。
export function focusSession(items: readonly Item[]): Item | null {
  const rank = (item: Item) => {
    const tone = cardTone(item);
    if (tone === "pending") return 0;
    if (tone === "error") return 1;
    if (tone === "waiting") return item.unread ? 2 : 3;
    if (tone === "running") return 4;
    return 9;
  };
  const candidates = live(items).filter((item) => rank(item) < 9);
  candidates.sort((a, b) => rank(a) - rank(b) || b.session.last_event_at - a.session.last_event_at);
  return candidates[0] ?? null;
}

/// 两只眼睛（Meowo 标志）。动作全在 CSS：在跑左右张望、待处理琥珀色一闪一闪、出错变红、闲着偶尔眨眼。
function Eyes({ tone }: { tone: IslandTone }) {
  return (
    <svg className="island-eyes" data-tone={tone} width={22} height={14} viewBox="0 0 22 14" aria-hidden="true">
      <g className="island-eyes-pair">
        <ellipse cx="5.5" cy="7" rx="4.5" ry="5.5" />
        <ellipse cx="16.5" cy="7" rx="4.5" ry="5.5" />
      </g>
    </svg>
  );
}

export function Island({
  sessions,
  compactStyle,
  alert,
  onMeasure,
  onHoverIntent,
  onActivate,
  onOpenSession,
  onPointerDownDrag,
}: {
  sessions: readonly Item[];
  compactStyle: IslandCompactStyle;
  /** 正在弹出的提醒；null = 收起态。 */
  alert: IslandAlertEvent | null;
  /** 胶囊内容的实测宽度（逻辑像素），App 据此让窗口贴合。 */
  onMeasure: (width: number) => void;
  /** 鼠标在胶囊上停够 250ms（U1-9：屏幕顶边常有鼠标路过，不能一碰就展开）。 */
  onHoverIntent: () => void;
  /** 点一下胶囊（没拖动）。 */
  onActivate: () => void;
  onOpenSession: (sessionId: number) => void;
  /** 按下胶囊：交给 App 判断是点击还是左右拖动。 */
  onPointerDownDrag: (event: React.PointerEvent<HTMLElement>) => void;
}) {
  const t = useT();
  const pillRef = useRef<HTMLDivElement>(null);
  const hoverTimer = useRef<number | null>(null);
  const counts = useMemo(() => islandCounts(sessions), [sessions]);
  const tone = islandTone(counts);
  const focus = useMemo(() => (compactStyle === "focus" ? focusSession(sessions) : null), [compactStyle, sessions]);

  // 收起态的宽度随内容变（数字位数、标题长短），实测后上报。提醒形态尺寸固定，不量。
  useLayoutEffect(() => {
    if (alert) return;
    const el = pillRef.current;
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

  const summary = () => {
    if (compactStyle === "minimal") {
      const dots = live(sessions).slice(0, 6);
      return (
        <span className="island-dots">
          {dots.map((item) => <span key={item.session.id} className={`island-dot is-${cardTone(item)}`} />)}
        </span>
      );
    }
    if (compactStyle === "focus") {
      if (!focus) return <span className="island-text is-faint">{t.island.idle}</span>;
      const focusTone = cardTone(focus);
      return (
        <span className="island-text">
          <span className="island-title">{focus.task_title && focus.task_title !== UNNAMED_SESSION_SENTINEL ? focus.task_title : t.sticker.waitingFirstInput}</span>
          <span className="island-sep">·</span>
          <span className={`island-state is-${focusTone}`}>{t.island.toneLabel[focusTone] ?? focusTone}</span>
        </span>
      );
    }
    const parts = (["pending", "error", "running", "waiting"] as const).filter((key) => counts[key] > 0);
    if (parts.length === 0) return <span className="island-text is-faint">{t.island.idle}</span>;
    return (
      <span className="island-text">
        {parts.map((key) => (
          <span key={key} className="island-count">
            <span className={`island-dot is-${key}`} />
            {counts[key]} {t.island.toneLabel[key]}
          </span>
        ))}
      </span>
    );
  };

  return (
    <div className="island-stage">
      <div
        ref={pillRef}
        className={`island-pill${alert ? " is-alert" : ""}`}
        data-tone={alertTone}
        role="button"
        tabIndex={0}
        aria-label={t.island.ariaLabel}
        onPointerEnter={() => {
          clearHover();
          hoverTimer.current = window.setTimeout(() => { hoverTimer.current = null; onHoverIntent(); }, 250);
        }}
        onPointerLeave={clearHover}
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
        <Eyes tone={alertTone} />
        {alert ? (
          <span className="island-alert">
            <span className="island-alert-title">{alert.title && alert.title !== UNNAMED_SESSION_SENTINEL ? alert.title : t.sticker.waitingFirstInput}</span>
            <span className="island-alert-sub">
              <span className={`island-state is-${alertTone}`}>{alertLabel(alert)}</span>
              {alert.project && <><span className="island-sep">·</span>{alert.project}</>}
            </span>
          </span>
        ) : summary()}
        {alert && (
          <button
            type="button"
            className="island-open"
            // 按钮上按下不该开始拖动，也不该冒泡成「点胶囊 = 展开」。
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => {
              event.stopPropagation();
              onOpenSession(alert.sessionId);
            }}
          >{t.island.open}</button>
        )}
      </div>
    </div>
  );
}
