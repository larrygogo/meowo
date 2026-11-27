import { useEffect, useRef, useState } from "react";
import { useT } from "../../i18n";
import { useDismissable } from "../../hooks/useDismissable";
import { usageTone } from "../usage/usageView";

type T = ReturnType<typeof useT>;

/**
 * 输入框旁的上下文圆环 + 悬停卡片（照 Orca 的 NativeChatContextUsageRing）。
 *
 * 圆环本身不变：16px，60%↑黄、85%↑红；`pct` 为 null = 这家 CLI 不上报用量，只画底圈（走查输入 C6）。
 * 悬停 150ms 或点击弹出卡片：上下文「已用 / 窗口」和进度条。
 * 账号额度照 Orca 的分工放在窗口底部常驻状态条（ChatStatusBar），这里不再重复——
 * 之前两处都显示，主人看新版时问「状态条呢」，按 Orca 拆开：圆环管上下文，状态条管额度。
 */
export function ContextMeter({ pct, window, t }: {
  pct: number | null;
  window: number | null;
  t: T;
}) {
  const used = pct ?? 0;
  const clamped = Math.min(100, Math.max(0, used));
  const R = 8;
  const C = 2 * Math.PI * R;
  const tone = pct == null ? "is-unsupported" : used >= 85 ? "is-full" : used >= 60 ? "is-warn" : "";
  const tip = pct == null ? t.chat.contextUnsupported
    : window ? t.chat.contextTip(pct, Math.round(window / 1000)) : t.chat.contextShort(pct);

  const wrapRef = useRef<HTMLSpanElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  // hover：指针在上面时开；pinned：点开的，指针离开也不关，点外 / Esc 关。
  const [hover, setHover] = useState(false);
  const [pinned, setPinned] = useState(false);
  const timer = useRef<number | null>(null);
  const open = hover || pinned;
  const clearTimer = () => { if (timer.current != null) { window_.clearTimeout(timer.current); timer.current = null; } };
  useEffect(() => clearTimer, []);
  useDismissable(wrapRef, { open: pinned, onClose: () => { setPinned(false); setHover(false); }, escFocusReturn: () => btnRef.current });

  return (
    <span
      ref={wrapRef}
      className="chat-context-wrap"
      onMouseEnter={() => { clearTimer(); timer.current = window_.setTimeout(() => setHover(true), 150); }}
      onMouseLeave={() => { clearTimer(); timer.current = window_.setTimeout(() => setHover(false), 150); }}
    >
      <button
        ref={btnRef}
        type="button"
        className={"chat-context " + tone}
        aria-label={tip}
        aria-expanded={open}
        aria-haspopup="dialog"
        data-testid="chat-context"
        onClick={() => setPinned((v) => !v)}
      >
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
      </button>
      {open && (
        <div className="chat-context-card" role="dialog" aria-label={t.chat.contextCardTitle} data-testid="chat-context-card">
          <section className="chat-context-sec">
            <div className="chat-context-row">
              <span className="chat-context-title">{t.chat.contextCardTitle}</span>
              {pct != null && window ? (
                <span className="chat-context-amount">{t.chat.contextCardAmount(fmtTokens(Math.round((clamped / 100) * window)), fmtTokens(window))}</span>
              ) : pct != null ? (
                <span className="chat-context-amount">{t.chat.contextCardPct(pct)}</span>
              ) : null}
            </div>
            {pct != null ? (
              <>
                <div className={"chat-context-bar" + usageTone(clamped)}><i style={{ width: `${clamped}%` }} /></div>
                {/* 照 Orca：有窗口时只留「已用/窗口」和条，不另写百分比；CLI 没给窗口才补一句。 */}
                {!window && <div className="chat-context-note">{t.chat.contextCardNoWindow}</div>}
              </>
            ) : (
              <div className="chat-context-note">{t.chat.contextUnsupported}</div>
            )}
          </section>
        </div>
      )}
    </span>
  );
}

// 组件里的 `window` 参数遮住了全局 window。
const window_ = globalThis.window;

/** 607.7k / 1M：千位一位小数，百万同理；整数不带 .0。 */
export function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${+(n / 1_000).toFixed(1)}k`;
  return String(n);
}
