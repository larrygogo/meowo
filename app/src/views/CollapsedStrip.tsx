import { useEffect, useRef } from "react";
import { LiveSession } from "../api";

type Item = LiveSession & { connected: boolean };
type Edge = "left" | "right" | "top";

// 竖条：纵向排列各 connected 会话的状态色点（断开/历史会话不显示）。
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
      // 横条量内容宽度、竖条量内容高度；再加 .cstrip padding 余量。
      const content = horizontal ? el.scrollWidth : el.scrollHeight;
      onMeasure(Math.ceil(content) + 12);
    }
  }, [items.length, onMeasure, horizontal]);

  return (
    <div className={"cstrip cstrip-" + edge} onMouseEnter={onExpand}>
      <div className="cstrip-dots" ref={dotsRef}>
        {items.map((l) => {
          const cls =
            l.session.status === "running"
              ? "cstrip-running"
              : l.session.status === "waiting"
              ? "cstrip-waiting"
              : "cstrip-on";
          return <span key={l.session.id} className={"cstrip-dot " + cls} />;
        })}
      </div>
    </div>
  );
}
