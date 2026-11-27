import { Fragment, useRef, useState } from "react";
import { useT } from "../../i18n";
import type { UsageLane } from "../../api";
import { useDismissable } from "../../hooks/useDismissable";
import { Icon } from "../../icons/Icon";
import { icAlert, icRefresh } from "../../icons/glyphs";
import { icSwap } from "../../icons/glyphs.chat";
import { openSettingsAt } from "../../settingsNav";
import { fmtAgo } from "../sticker/helpers";
import { fmtResetIn, laneLabel, usageFailureText, usageTone, type UsageFailure } from "../usage/usageView";
import { AgentAppIcon } from "./AgentAppIcon";
import {
  fmtResetShort, setUsageDisplay, shownPct, useChatAccountUsage, useUsageDisplay,
  type UsageDisplay, type UsageState,
} from "./chatAccountUsage";

type T = ReturnType<typeof useT>;

/**
 * 对话窗底部常驻状态条（照 Orca 的 StatusBar：24px 一行、顶部细线、窗口底色）。
 *
 * 分工照 Orca：状态条管**账号额度**——左侧「已用｜图标 迷你条 9% 3h 39m · 79% 3d 13h」加刷新钮，
 * 点开是详情面板（每个窗口一条进度条，≥80% 黄 ≥90% 红，重置倒计时、更新于…，已用/剩余切换）；
 * 输入框旁的上下文圆环只管上下文。主人反馈之前「只有悬停圆环才看得到额度」。
 *
 * 状态条上不着色（Orca：usage is colored only inside a provider's details card），只有详情里的条按阈值上色。
 * 取不到时显示 ⚠ + 短文案（需重新登录 / 限流中…），保留上次数字时数字后挂 ⚠。
 */
export function ChatStatusBar({ provider, profile, agentName, supportsAccount }: {
  provider: string;
  profile: string | null;
  agentName: string;
  /** 后端声明这家 agent 有账号额度（supports_account）；否则只显示「--」并说明。 */
  supportsAccount: boolean;
}) {
  const t = useT();
  const display = useUsageDisplay();
  const usage = useChatAccountUsage(supportsAccount ? provider : null, profile, supportsAccount);
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  useDismissable(wrapRef, { open, onClose: () => setOpen(false), escFocusReturn: () => btnRef.current });
  const lanes = (usage.usage?.lanes ?? []).filter((l) => l.used_pct != null);
  // 状态条只放时间窗口（5 小时 / 7 天 / 周），模型专属周限、余额这类进详情。
  const chipLanes = lanes.filter((l) => l.kind === "five_hour" || l.kind === "seven_day" || l.kind === "weekly").slice(0, 2);
  const headline = chipLanes[0] ?? lanes[0];

  return (
    <div className="chat-statusbar" data-testid="chat-statusbar" role="contentinfo">
      <div className="chat-statusbar-usage" ref={wrapRef}>
        {!supportsAccount ? (
          <span className="chat-statusbar-chip is-muted" title={t.chat.usageBarUnsupported(agentName)}>
            <AgentAppIcon provider={provider} className="chat-statusbar-icon" />
            <span>--</span>
          </span>
        ) : (
          <>
            <button
              ref={btnRef}
              type="button"
              className="chat-statusbar-chip"
              data-testid="chat-statusbar-usage"
              aria-label={t.chat.usageBarTitle}
              aria-haspopup="dialog"
              aria-expanded={open}
              onClick={() => setOpen((v) => !v)}
            >
              {chipLanes.length > 0 && (
                <span className="chat-statusbar-unit">
                  {display === "used" ? t.chat.usageUsed : t.chat.usageRemaining}
                  <i aria-hidden="true" />
                </span>
              )}
              <AgentAppIcon provider={provider} className="chat-statusbar-icon" />
              <ChipBody state={usage} lanes={chipLanes} headline={headline} display={display} t={t} />
            </button>
            <button
              type="button"
              className="chat-statusbar-refresh"
              data-testid="chat-statusbar-refresh"
              aria-label={t.chat.usageRefresh}
              title={t.chat.usageRefresh}
              disabled={usage.fetching}
              onClick={usage.retry}
            >
              <Icon glyph={icRefresh} size={12} className={usage.fetching ? "is-spinning" : undefined} />
            </button>
            {open && (
              <UsageDetails state={usage} lanes={lanes} display={display} t={t} agentName={agentName} onClose={() => setOpen(false)} />
            )}
          </>
        )}
      </div>
    </div>
  );
}

function ChipBody({ state, lanes, headline, display, t }: {
  state: UsageState; lanes: UsageLane[]; headline: UsageLane | undefined; display: UsageDisplay; t: T;
}) {
  // 首次读取：Orca 是静态的「···」（不闪，失焦窗口不重绘）。
  if (state.loading || (!state.usage && !state.failure)) return <span className="chat-statusbar-dim">···</span>;
  if (lanes.length === 0) {
    if (state.failure) {
      return (
        <span className="chat-statusbar-dim" title={state.failure ? usageFailureText(state.failure, t, "") : undefined}>
          <Icon glyph={icAlert} size={12} />
          <span>{shortFailure(state.failure, t)}</span>
        </span>
      );
    }
    return <span className="chat-statusbar-dim">--</span>;
  }
  return (
    <>
      {headline && (
        <span className="chat-statusbar-mini" aria-hidden="true">
          <i style={{ width: `${shownPct(headline.used_pct as number, display)}%` }} />
        </span>
      )}
      {lanes.map((lane, i) => (
        <Fragment key={`${lane.kind}-${i}`}>
          {i > 0 && <span className="chat-statusbar-dim" aria-hidden="true">·</span>}
          <span className="chat-statusbar-num" title={laneLabel(lane, t)}>
            {shownPct(lane.used_pct as number, display)}%
            {lane.resets_at && <span className="chat-statusbar-reset">{fmtResetShort(lane.resets_at)}</span>}
          </span>
        </Fragment>
      ))}
      {state.failure && <Icon glyph={icAlert} size={12} className="chat-statusbar-stale" label={usageFailureText(state.failure, t, "")} />}
    </>
  );
}

function shortFailure(f: UsageFailure, t: T): string {
  switch (f.kind) {
    case "expired": return t.chat.usageShortExpired;
    case "rateLimited": return t.chat.usageShortRateLimited;
    case "network": return t.chat.usageShortNetwork;
    case "server": return t.chat.usageShortServer;
    case "unsupported": return t.chat.usageShortUnsupported;
    default: return t.chat.usageShortUnknown;
  }
}

/** 详情面板（照 Orca 的 provider 详情卡）：账号行 + 每个窗口一条带阈值色的进度条 + 重置倒计时。 */
function UsageDetails({ state, lanes, display, t, agentName, onClose }: {
  state: UsageState; lanes: UsageLane[]; display: UsageDisplay; t: T; agentName: string; onClose: () => void;
}) {
  const { failure, fetchedAt, label } = state;
  const canRetry = failure != null && failure.kind !== "unsupported" && failure.kind !== "expired";
  const next: UsageDisplay = display === "used" ? "remaining" : "used";
  return (
    <div className="chat-statusbar-card" role="dialog" aria-label={t.chat.usageBarTitle} data-testid="chat-statusbar-card">
      <div className="chat-context-row">
        <span className="chat-context-title">{t.chat.chatAccountUsage}</span>
        {fetchedAt != null && <span className="chat-context-amount">{t.chat.chatUsageUpdated(fmtAgo(fetchedAt, t))}</span>}
      </div>
      <div className="chat-statusbar-card-account">
        <span className="chat-context-account">{label ?? agentName}</span>
        {lanes.length > 0 && <button
          type="button"
          className="chat-statusbar-toggle"
          data-testid="chat-statusbar-toggle"
          title={next === "remaining" ? t.chat.usageShowRemaining : t.chat.usageShowUsed}
          onClick={() => setUsageDisplay(next)}
        >
          {display === "used" ? t.chat.usageUsed : t.chat.usageRemaining}
          <Icon glyph={icSwap} size={12} />
        </button>}
      </div>
      {state.loading ? (
        <div className="chat-context-skeleton" aria-label={t.chat.chatUsageLoading}>
          <i /><i />
          <span className="chat-context-note">{t.chat.chatUsageLoading}</span>
        </div>
      ) : lanes.map((lane, i) => {
        const used = Math.max(0, Math.min(100, lane.used_pct as number));
        const shown = shownPct(used, display);
        return (
          <div key={`${lane.kind}-${i}`} className="chat-context-lane">
            <div className="chat-context-row">
              <span className="chat-context-lane-label">
                {laneLabel(lane, t)}
                {lane.resets_at && <span className="chat-context-note"> · {fmtResetIn(lane.resets_at, t)}</span>}
              </span>
              <span className="chat-context-amount">{shown}%</span>
            </div>
            {/* 色阶始终按已用算（照 Orca：Tone always follows consumption）。 */}
            <div className={"chat-context-bar" + usageTone(used)} role="progressbar" aria-label={laneLabel(lane, t)} aria-valuemin={0} aria-valuemax={100} aria-valuenow={shown}>
              <i style={{ width: `${shown}%` }} />
            </div>
          </div>
        );
      })}
      {failure && (
        <div className="chat-context-fail" role="status">
          <span>{lanes.length > 0 && failure.kind === "unknown" ? t.account.refreshFailed
            : `${usageFailureText(failure, t, agentName)}${lanes.length > 0 ? `。${t.account.usageStale}` : ""}`}</span>
          {canRetry && (
            <button type="button" className="chat-context-retry" data-testid="chat-statusbar-retry" onClick={state.retry}>{t.chat.chatUsageRetry}</button>
          )}
        </div>
      )}
      <button type="button" className="chat-statusbar-manage" onClick={() => { onClose(); openSettingsAt("account"); }}>
        {t.chat.usageManageAccounts}
      </button>
    </div>
  );
}
