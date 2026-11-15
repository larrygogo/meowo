import { type useT } from "../../i18n";

/** 上下文用量只显示高亮圆环；百分比与窗口详情统一留在 tooltip。60%↑黄、85%↑红。
 *  `pct` 为 null = 这家 CLI 不上报用量（后端声明 supports_context=false）：圆环留在原位、只画底圈，
 *  悬停说明原因。换一家 Provider 位置不变，差别说得清（走查输入 C6）。 */
export function ContextMeter({ pct, window, t }: { pct: number | null; window: number | null; t: ReturnType<typeof useT> }) {
  const used = pct ?? 0;
  const clamped = Math.min(100, Math.max(0, used));
  const R = 8;
  const C = 2 * Math.PI * R;
  const tone = pct == null ? "is-unsupported" : used >= 85 ? "is-full" : used >= 60 ? "is-warn" : "";
  const tip = pct == null ? t.chat.contextUnsupported
    : window ? t.chat.contextTip(pct, Math.round(window / 1000)) : t.chat.contextShort(pct);
  return (
    <span className={"chat-context " + tone} role="img" aria-label={tip} data-tip={tip}>
      <span className="chat-context-ring" aria-hidden="true">
        <svg width="16" height="16" viewBox="0 0 20 20">
          <circle className="chat-context-ring-bg" cx="10" cy="10" r={R} fill="none" strokeWidth="2.5" />
          <circle
            className="chat-context-ring-fg" cx="10" cy="10" r={R} fill="none" strokeWidth="2.5"
            strokeLinecap="round" strokeDasharray={C}
            strokeDashoffset={C * (1 - clamped / 100)} transform="rotate(-90 10 10)"
          />
        </svg>
      </span>
    </span>
  );
}
