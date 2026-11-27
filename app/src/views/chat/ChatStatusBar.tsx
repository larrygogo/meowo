import { Fragment, useRef, useState } from "react";
import { useT } from "../../i18n";
import type { ProfileView, UsageLane } from "../../api";
import { useDismissable } from "../../hooks/useDismissable";
import { Icon } from "../../icons/Icon";
import { icAlert, icCheck, icRefresh } from "../../icons/glyphs";
import { icSwap } from "../../icons/glyphs.chat";
import { profileLabel } from "../../profileLabel";
import { openSettingsAt } from "../../settingsNav";
import { fmtAgo } from "../sticker/helpers";
import { fmtResetIn, laneLabel, usageFailureText, usageTone, type UsageFailure } from "../usage/usageView";
import { AgentAppIcon } from "./AgentAppIcon";
import {
  fmtResetShort, getUsageEntry, isUsageFetching, setUsageDisplay, setUsageMode, shownPct,
  useAccountRoster, useChatAccountUsage, useUsageDisplay, useUsageMode,
  type UsageDisplay, type UsageEntry, type UsageMode, type UsageState,
} from "./chatAccountUsage";

type T = ReturnType<typeof useT>;

/**
 * 对话窗底部常驻状态条（照 Orca 的 StatusBar：24px 一行、窗口底色）。
 *
 * 分工照 Orca：状态条管**账号额度**，输入框旁的上下文圆环只管上下文。
 * - 常驻那一格只显示**这个对话所用的账号**（Orca 也是每家只显示当前账号）。「详细」是
 *   「已用｜图标 迷你条 9% 3h 39m · 79% 3d 13h」，「精简」只留最紧的那个百分比。
 * - 点开是面板：上半是这个对话所用账号的各窗口进度条（≥80% 黄、≥90% 红）；下半照 Orca 的
 *   Claude 账号面板列出系统默认和所有已添加的账号，每个账号各自的额度、重置倒计时、状态，
 *   这个对话的账号打勾，未登录的单独一组给「登录」。主人反馈「Orca 能看到所有账号，Meowo 只能看到一个」。
 * - 状态条上不着色（Orca：usage is colored only inside a provider's details card），面板里的条按阈值上色。
 *
 * 和 Orca 不同：面板里的账号行不能点了就切换。Orca 的状态条讲的是「当前账号」，点哪个就切到哪个；
 * Meowo 这条讲的是「这个对话」，切换账号只对之后新开的对话生效，放在这里点一下会让人以为
 * 当前对话也换了账号。切换留在「管理账号…」进去的设置页。
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
  const mode = useUsageMode();
  const usage = useChatAccountUsage(supportsAccount ? provider : null, profile, supportsAccount);
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  useDismissable(wrapRef, { open, onClose: () => setOpen(false), escFocusReturn: () => btnRef.current });
  const lanes = usedLanes(usage.usage?.lanes);
  const chipLanes = primaryLanes(lanes);
  const tightest = tightestLane(chipLanes);

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
              <ChipBody state={usage} lanes={mode === "compact" && tightest ? [tightest] : chipLanes} compact={mode === "compact"} display={display} t={t} />
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
              <UsagePanel
                provider={provider}
                profile={profile}
                state={usage}
                lanes={lanes}
                display={display}
                mode={mode}
                t={t}
                agentName={agentName}
                onClose={() => setOpen(false)}
              />
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** 有百分比的泳道。 */
function usedLanes(lanes: UsageLane[] | undefined): UsageLane[] {
  return (lanes ?? []).filter((l) => l.used_pct != null);
}
/** 状态条只放时间窗口（5 小时 / 7 天 / 周），模型专属周限、余额这类进面板。 */
function primaryLanes(lanes: UsageLane[]): UsageLane[] {
  return lanes.filter((l) => l.kind === "five_hour" || l.kind === "seven_day" || l.kind === "weekly").slice(0, 2);
}
/** 最紧的那个时间窗口（照 Orca 的 getTightestUsageSection：5 小时或周里用得最多的）。 */
function tightestLane(lanes: UsageLane[]): UsageLane | undefined {
  return lanes.reduce<UsageLane | undefined>((a, l) => (!a || (l.used_pct as number) > (a.used_pct as number) ? l : a), undefined);
}
/** 账号行里的短窗口名：5h / 周 / Fable（照 Orca 面板的「5h ▬ 0%  周 ▬ 2%」）。 */
function shortLaneLabel(lane: UsageLane, t: T): string {
  if (lane.kind === "five_hour") return "5h";
  if (lane.kind === "model_weekly" || lane.kind === "opus") return lane.label ?? (lane.kind === "opus" ? "Opus" : t.chat.usageWeekShort);
  if (lane.kind === "seven_day" || lane.kind === "weekly") return t.chat.usageWeekShort;
  return laneLabel(lane, t);
}

function ChipBody({ state, lanes, compact, display, t }: {
  state: UsageState; lanes: UsageLane[]; compact: boolean; display: UsageDisplay; t: T;
}) {
  // 首次读取：Orca 是静态的「···」（不闪，失焦窗口不重绘）。
  if (state.loading || (!state.usage && !state.failure)) return <span className="chat-statusbar-dim">···</span>;
  if (lanes.length === 0) {
    if (state.failure) {
      return (
        <span className="chat-statusbar-dim" title={usageFailureText(state.failure, t, "")}>
          <Icon glyph={icAlert} size={12} />
          <span>{shortFailure(state.failure, t)}</span>
        </span>
      );
    }
    return <span className="chat-statusbar-dim">--</span>;
  }
  const headline = tightestLane(lanes);
  return (
    <>
      {!compact && headline && (
        <span className="chat-statusbar-mini" aria-hidden="true">
          <i style={{ width: `${shownPct(headline.used_pct as number, display)}%` }} />
        </span>
      )}
      {lanes.map((lane, i) => (
        <Fragment key={`${lane.kind}-${i}`}>
          {i > 0 && <span className="chat-statusbar-dim" aria-hidden="true">·</span>}
          {/* 精简只留百分比，窗口名和重置时间放进悬停提示（照 Orca 的 compact chip title）。 */}
          <span className="chat-statusbar-num" title={lane.resets_at ? `${laneLabel(lane, t)} · ${fmtResetIn(lane.resets_at, t)}` : laneLabel(lane, t)}>
            {shownPct(lane.used_pct as number, display)}%
            {!compact && lane.resets_at && <span className="chat-statusbar-reset">{fmtResetShort(lane.resets_at)}</span>}
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

/** 面板（照 Orca 的用量面板 + Claude 账号卡）。 */
function UsagePanel({ provider, profile, state, lanes, display, mode, t, agentName, onClose }: {
  provider: string; profile: string | null; state: UsageState; lanes: UsageLane[];
  display: UsageDisplay; mode: UsageMode; t: T; agentName: string; onClose: () => void;
}) {
  const { failure, fetchedAt, label } = state;
  const canRetry = failure != null && failure.kind !== "unsupported" && failure.kind !== "expired";
  const next: UsageDisplay = display === "used" ? "remaining" : "used";
  const roster = useAccountRoster(provider, profile, true);
  const signedIn = (roster.rows ?? []).filter((p) => p.account);
  const signedOut = (roster.rows ?? []).filter((p) => !p.account);
  const manage = () => { onClose(); openSettingsAt("account"); };
  return (
    <div className="chat-statusbar-card" role="dialog" aria-label={t.chat.usageBarTitle} data-testid="chat-statusbar-card">
      <div className="chat-statusbar-card-head">
        <span className="chat-context-title">{t.chat.usageBarTitle}</span>
        <button
          type="button"
          className="chat-statusbar-refresh"
          data-testid="chat-statusbar-refresh-all"
          aria-label={t.chat.usageRefresh}
          title={t.chat.usageRefresh}
          onClick={() => { state.retry(); roster.refreshAll(); }}
        >
          <Icon glyph={icRefresh} size={12} className={state.fetching ? "is-spinning" : undefined} />
        </button>
        <span className="chat-statusbar-spacer" />
        <button
          type="button"
          className="chat-statusbar-toggle"
          data-testid="chat-statusbar-toggle"
          title={next === "remaining" ? t.chat.usageShowRemaining : t.chat.usageShowUsed}
          onClick={() => setUsageDisplay(next)}
        >
          {display === "used" ? t.chat.usageUsed : t.chat.usageRemaining}
          <Icon glyph={icSwap} size={12} />
        </button>
      </div>
      {/* 详细 / 精简（照 Orca）：管状态条那一格和下面账号行的详略。 */}
      <div className="chat-statusbar-seg" role="radiogroup" aria-label={t.chat.usageModeLabel}>
        {(["detailed", "compact"] as const).map((m) => (
          <button
            key={m}
            type="button"
            role="radio"
            aria-checked={mode === m}
            data-testid={`chat-statusbar-mode-${m}`}
            className={mode === m ? "is-on" : undefined}
            onClick={() => setUsageMode(m)}
          >
            {m === "detailed" ? t.chat.usageModeDetailed : t.chat.usageModeCompact}
          </button>
        ))}
      </div>

      <section className="chat-statusbar-current">
        <div className="chat-context-row">
          <span className="chat-context-account is-strong">{label ?? agentName}</span>
          {fetchedAt != null && <span className="chat-context-amount">{t.chat.chatUsageUpdated(fmtAgo(fetchedAt, t))}</span>}
        </div>
        <div className="chat-context-note">{t.chat.chatAccountUsage}</div>
        {state.loading ? (
          <div className="chat-context-skeleton" aria-label={t.chat.chatUsageLoading}>
            <i /><i />
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
      </section>

      {/* 账号列表（照 Orca 的 AccountSwitchList）：只有一个账号时它就是上面那张卡，不重复列。 */}
      {(roster.rows?.length ?? 0) > 1 && (
        <section className="chat-statusbar-roster" data-testid="chat-statusbar-roster">
          <div className="chat-statusbar-roster-head">
            <span>{t.chat.usageAccounts}</span>
            <span>{display === "used" ? t.chat.usageUsed : t.chat.usageRemaining}</span>
          </div>
          {signedIn.map((p) => (
            <AccountRow
              key={p.id ?? ""}
              p={p}
              t={t}
              mode={mode}
              display={display}
              agentName={agentName}
              current={p.id === profile}
              entry={getUsageEntry(provider, p.id)}
              fetching={isUsageFetching(provider, p.id)}
            />
          ))}
          {signedOut.length > 0 && (
            <>
              <div className="chat-statusbar-roster-head is-group">{t.chat.usageSignedOut}</div>
              {signedOut.map((p) => (
                <div key={p.id ?? ""} className="chat-statusbar-acct" data-testid="chat-statusbar-acct">
                  <span className="chat-statusbar-acct-check" />
                  <span className="chat-statusbar-acct-name">
                    <span className="chat-statusbar-acct-label">{profileLabel(p, t)}</span>
                    <span className="chat-statusbar-acct-detail">{t.chat.usageSignedOut}</span>
                  </span>
                  <button type="button" className="chat-statusbar-signin" onClick={manage}>{t.chat.usageSignIn}</button>
                </div>
              ))}
            </>
          )}
        </section>
      )}
      <button type="button" className="chat-statusbar-manage" onClick={manage}>
        {t.chat.usageManageAccounts}
      </button>
    </div>
  );
}

/** 账号行（照 Orca 的 AccountSwitchRowItem + AccountUsageGauge）。 */
function AccountRow({ p, t, mode, display, agentName, current, entry, fetching }: {
  p: ProfileView; t: T; mode: UsageMode; display: UsageDisplay; agentName: string;
  current: boolean; entry: UsageEntry | undefined; fetching: boolean;
}) {
  const lanes = usedLanes(entry?.usage?.lanes);
  const primary = primaryLanes(lanes);
  const headline = tightestLane(primary);
  const shown = mode === "compact" ? (headline ? [headline] : []) : lanes;
  const detail = p.account?.plan ?? null;
  let right: React.ReactNode;
  if (shown.length > 0) {
    right = (
      <span className={"chat-statusbar-acct-usage" + (mode === "detailed" ? " is-detailed" : "") + (fetching ? " is-fetching" : "")}>
        {shown.map((lane, i) => {
          const used = Math.max(0, Math.min(100, lane.used_pct as number));
          const pct = shownPct(used, display);
          return (
            <span key={`${lane.kind}-${i}`} className="chat-statusbar-gauge" title={lane.resets_at ? `${laneLabel(lane, t)} · ${fmtResetIn(lane.resets_at, t)}` : laneLabel(lane, t)}>
              {mode === "detailed" && <span className="chat-statusbar-gauge-label">{shortLaneLabel(lane, t)}</span>}
              <span className={"chat-statusbar-gauge-bar" + usageTone(used)} aria-hidden="true"><i style={{ width: `${pct}%` }} /></span>
              <span className="chat-statusbar-gauge-pct">{pct}%</span>
              {mode === "detailed" && <span className="chat-statusbar-gauge-reset">{lane.resets_at ? fmtResetShort(lane.resets_at) : ""}</span>}
            </span>
          );
        })}
        {entry?.failure && <Icon glyph={icAlert} size={12} className="chat-statusbar-stale" label={usageFailureText(entry.failure, t, agentName)} />}
      </span>
    );
  } else if (entry?.failure) {
    right = <span className="chat-statusbar-acct-note" title={usageFailureText(entry.failure, t, agentName)}>{shortFailure(entry.failure, t)}</span>;
  } else if (fetching) {
    right = <span className="chat-statusbar-acct-note">···</span>;
  } else {
    // 还没读过（照 Orca 的「未检查」：原因放提示里，不占一行字）。
    right = <span className="chat-statusbar-acct-note" title={t.chat.usageNotCheckedTip}>{t.chat.usageNotChecked}</span>;
  }
  return (
    <div className={"chat-statusbar-acct" + (current ? " is-current" : "")} data-testid="chat-statusbar-acct" aria-current={current ? "true" : undefined}>
      <span className="chat-statusbar-acct-check">
        {current && <Icon glyph={icCheck} size={12} />}
      </span>
      <span className="chat-statusbar-acct-name">
        <span className="chat-statusbar-acct-label">{profileLabel(p, t)}</span>
        {(detail || current) && (
          <span className="chat-statusbar-acct-detail">{[current ? t.chat.usageThisChat : null, detail].filter(Boolean).join(" · ")}</span>
        )}
      </span>
      {right}
    </div>
  );
}
