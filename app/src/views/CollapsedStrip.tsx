import { useEffect, useRef } from "react";
import { LiveSession } from "../api";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right" | "top";

// 缩略条主轴最小长度：保证空状态/只有一个点时仍是一条好找好点的条，而非细缝。
const STRIP_MIN = 48;

// 缩略条的 app 标记：无活跃会话时居中显示，占位用（看板三列图标）。
function AppMark() {
  return (
    <svg
      width={18}
      height={18}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M9 8v8M15 8v5" />
    </svg>
  );
}

// 竖条：纵向排列各 connected 会话的状态色点（断开/历史会话不显示）。
// 无活跃会话时显示 app 图标占位，保持缩略条合理可点的尺寸。
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
    <div className={"cstrip cstrip-" + edge} onMouseEnter={onExpand}>
      <div className="cstrip-dots" ref={dotsRef}>
        {items.length === 0 ? (
          <span className="cstrip-empty">
            <AppMark />
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
            return <span key={l.session.id} className={"cstrip-dot " + cls} />;
          })
        )}
      </div>
    </div>
  );
}
