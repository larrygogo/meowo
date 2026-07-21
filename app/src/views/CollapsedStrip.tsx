import { useEffect, useRef } from "react";
import { LiveSession } from "../api";
import { useT } from "../i18n";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right" | "top";

// 缩略条主轴最小长度：保证空状态/只有一个点时仍是一条好找好点的条，而非细缝。
const STRIP_MIN = 48;

// 缩略条的空态占位：无活跃会话时居中显示一双灰色眼睛，呼应 Meowo logo。
function EyesMark() {
  return (
    <svg
      width={22}
      height={22}
      viewBox="0 0 24 24"
      fill="currentColor"
      className="cstrip-eyes"
      aria-hidden
    >
      <circle cx="6.5" cy="12" r="4.5" />
      <circle cx="17.5" cy="12" r="4.5" />
    </svg>
  );
}

// 竖条：纵向排列各 connected 会话的状态色点（断开/历史会话不显示）。
// 无活跃会话时显示灰色眼睛占位，保持缩略条合理可点的尺寸。
// 悬停即偷看展开（onExpand）；测量真实内容高度上报（onMeasure）让窗口贴合，避免滚动条。
export function CollapsedStrip({
  data,
  edge,
  onExpand,
  onMeasure,
}: {
  data: Item[];
  edge: Edge;
  onExpand: () => void;
  onMeasure?: (heightPx: number) => void;
}) {
  const t = useT();
  const items = data.filter((l) => !l.archived && l.connected);
  const dotsRef = useRef<HTMLDivElement>(null);
  const horizontal = edge === "top"; // 顶部为横条，沿宽度排列

  useEffect(() => {
    const el = dotsRef.current;
    if (el && onMeasure) {
      // 横条量内容宽度、竖条量内容高度；再加 .cstrip padding 余量；不低于 STRIP_MIN。
      const content = horizontal ? el.scrollWidth : el.scrollHeight;
      onMeasure(Math.max(Math.ceil(content) + 12, STRIP_MIN));
    }
  }, [items.length, onMeasure, horizontal]);

  return (
    // 键盘可达：可聚焦，聚焦/Enter/Space 与悬停一样展开。用 group 而非 button——
    // button 会把内部状态点的 img 语义压掉，状态点的可访问文本就丢了。
    <div
      className={"cstrip cstrip-" + edge}
      role="group"
      tabIndex={0}
      aria-label={t.sticker.expandBoard}
      onMouseEnter={onExpand}
      onFocus={onExpand}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onExpand();
        }
      }}
    >
      <div className="cstrip-dots" ref={dotsRef}>
        {items.length === 0 ? (
          <span className="cstrip-empty">
            <EyesMark />
          </span>
        ) : (
          items.map((l) => {
            const cls = l.errored
              ? "cstrip-error"
              : l.session.status === "running"
              ? "cstrip-running"
              : l.session.status === "waiting"
              ? "cstrip-waiting"
              : "cstrip-on";
            const status = l.errored
              ? t.sticker.sessionError
              : l.session.status === "running"
              ? t.badge.running
              : l.session.status === "waiting"
              ? t.badge.waiting
              : t.sticker.online;
            return (
              <span
                key={l.session.id}
                className={"cstrip-dot " + cls}
                role="img"
                aria-label={`${l.task_title || t.sticker.waitingFirstInput} · ${status}`}
              />
            );
          })
        )}
      </div>
    </div>
  );
}
