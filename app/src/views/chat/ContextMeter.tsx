import { useCallback, useEffect, useRef, useState } from "react";
import { useT } from "../../i18n";
import { listProfiles, refreshProfileUsage, type AgentId, type ProviderUsage } from "../../api";
import { useDismissable } from "../../hooks/useDismissable";
import { profileLabel } from "../../profileLabel";
import { fmtAgo } from "../sticker/helpers";
import { fmtResetIn, laneLabel, usageFailure, usageFailureText, usageTone, type UsageFailure } from "../usage/usageView";

type T = ReturnType<typeof useT>;

/**
 * 输入框旁的上下文圆环 + 悬停卡片（照 Orca 的 NativeChatContextUsageRing 与 /usage 账号卡）。
 *
 * 圆环本身不变：16px，60%↑黄、85%↑红；`pct` 为 null = 这家 CLI 不上报用量，只画底圈（走查输入 C6）。
 * 悬停 150ms 或点击弹出卡片：上半是上下文「已用 / 窗口」和进度条；下半是**这个对话所用账号**的额度
 * （5 小时 / 7 天等泳道，≥80% 黄、≥90% 红，按分钟倒数重置，「更新于…」）。取不到额度时说清原因
 * （过期 / 限流 / 服务端 / 网络 / 不支持），能重试的给「重试」——之前对话页没有任何额度信息，
 * 设置页取不到也只有一句「用量获取失败」。
 */
export function ContextMeter({ pct, window, t, provider, profile = null, agentName, showUsage = false }: {
  pct: number | null;
  window: number | null;
  t: T;
  /** 这个对话的 agent；给了且 showUsage 才显示账号额度。 */
  provider?: string;
  /** 这个对话跑在哪个账号上（null = 默认账号）。 */
  profile?: string | null;
  agentName?: string;
  showUsage?: boolean;
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
  const usage = useChatAccountUsage(showUsage && provider ? provider : null, profile, open);

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
          {showUsage && provider && <AccountUsageSection state={usage} t={t} agentName={agentName ?? provider} />}
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

type UsageState = {
  label: string | null;
  usage: ProviderUsage | null;
  failure: UsageFailure | null;
  loading: boolean;
  fetchedAt: number | null;
  retry: () => void;
};

/** 账号额度在各对话之间共用一份：同一账号 5 分钟内重开卡片不重复请求（照 Orca 的去重窗口）。 */
const usageCache = new Map<string, { usage: ProviderUsage | null; failure: UsageFailure | null; at: number; fetchedAt: number | null }>();
const REUSE_MS = 5 * 60_000;
/** 测试用：模块级缓存跨用例会串。 */
export function resetChatUsageCache() { usageCache.clear(); }

function useChatAccountUsage(provider: string | null, profile: string | null, open: boolean): UsageState {
  const key = provider ? `${provider}|${profile ?? ""}` : "";
  const [, bump] = useState(0);
  const [loading, setLoading] = useState(false);
  const [label, setLabel] = useState<string | null>(null);
  const inflight = useRef(false);
  const t = useT();

  const load = useCallback((force: boolean) => {
    if (!provider || inflight.current) return;
    const hit = usageCache.get(key);
    if (!force && hit && Date.now() - hit.at < REUSE_MS) return;
    inflight.current = true;
    setLoading(true);
    refreshProfileUsage(provider as AgentId, profile)
      .then((u) => usageCache.set(key, { usage: u, failure: null, at: Date.now(), fetchedAt: Date.now() }))
      // 出错保留上次的数字（照 Orca），只换上失败原因。
      .catch((e) => usageCache.set(key, { usage: hit?.usage ?? null, failure: usageFailure(e), at: Date.now(), fetchedAt: hit?.fetchedAt ?? null }))
      .finally(() => { inflight.current = false; setLoading(false); bump((n) => n + 1); });
  }, [key, provider, profile]);

  useEffect(() => {
    if (!open || !provider) return;
    load(false);
    // 账号名（起过名的显示名字，否则邮箱）：本地读，不联网。
    listProfiles(provider as AgentId)
      .then((rows) => {
        const row = rows.find((p) => p.id === profile);
        setLabel(row ? [profileLabel(row, t), row.account?.plan].filter(Boolean).join(" · ") : null);
      })
      .catch(() => setLabel(null));
    // 卡片开着时窗口重新聚焦就刷新一次（照 Orca：focus 时刷新），仍受 5 分钟去重约束。
    const onFocus = () => load(false);
    window_.addEventListener("focus", onFocus);
    return () => window_.removeEventListener("focus", onFocus);
  }, [open, provider, profile, load, t]);

  const hit = usageCache.get(key);
  return {
    label,
    usage: hit?.usage ?? null,
    failure: hit?.failure ?? null,
    loading: loading && !hit?.usage,
    fetchedAt: hit?.fetchedAt ?? null,
    retry: () => load(true),
  };
}

function AccountUsageSection({ state, t, agentName }: { state: UsageState; t: T; agentName: string }) {
  const { usage, failure, loading, fetchedAt, label } = state;
  const lanes = usage?.lanes.filter((l) => l.used_pct != null) ?? [];
  const canRetry = failure != null && failure.kind !== "unsupported" && failure.kind !== "expired";
  return (
    <section className="chat-context-sec chat-context-usage" data-testid="chat-context-usage">
      <div className="chat-context-row">
        <span className="chat-context-title">{t.chat.chatAccountUsage}</span>
        {fetchedAt != null && <span className="chat-context-amount">{t.chat.chatUsageUpdated(fmtAgo(fetchedAt, t))}</span>}
      </div>
      {label && <div className="chat-context-account">{label}</div>}
      {loading ? (
        <div className="chat-context-skeleton" aria-label={t.chat.chatUsageLoading}>
          <i /><i />
          <span className="chat-context-note">{t.chat.chatUsageLoading}</span>
        </div>
      ) : (
        lanes.map((lane, i) => {
          const p = Math.max(0, Math.min(100, lane.used_pct as number));
          return (
            <div key={`${lane.kind}-${i}`} className="chat-context-lane">
              <div className="chat-context-row">
                <span className="chat-context-lane-label">{laneLabel(lane, t)}</span>
                <span className="chat-context-amount">{Math.round(p)}%</span>
              </div>
              <div className={"chat-context-bar" + usageTone(p)} role="progressbar" aria-label={laneLabel(lane, t)} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(p)}>
                <i style={{ width: `${p}%` }} />
              </div>
              {lane.resets_at && <div className="chat-context-note">{fmtResetIn(lane.resets_at, t)}</div>}
            </div>
          );
        })
      )}
      {failure && (
        <div className="chat-context-fail" role="status">
          <span>{lanes.length > 0 && failure.kind === "unknown" ? t.account.refreshFailed
            : `${usageFailureText(failure, t, agentName)}${lanes.length > 0 ? `。${t.account.usageStale}` : ""}`}</span>
          {canRetry && (
            <button type="button" className="chat-context-retry" data-testid="chat-context-retry" onClick={state.retry}>{t.chat.chatUsageRetry}</button>
          )}
        </div>
      )}
    </section>
  );
}
