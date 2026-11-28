import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useT } from "../../i18n";
import type { ProfileView } from "../../api";
import type { Dict } from "../../i18n/zh";
import { useDismissable } from "../../hooks/useDismissable";
import { profileLabel } from "../../profileLabel";
import { openSettingsAt } from "../../settingsNav";
import { usageFailureText, type UsageFailure } from "../usage/usageView";
import {
  Lucide, ProviderIcon, UsageBar, chipLabel, fmtDuration, panelLabel, shortLabel, shown, tightest, timeAgo, usedSections,
  type OSection,
} from "../usage/orcaParts";
import {
  getUsageEntry, isUsageFetching, setUsageDisplay, setUsageMode, useAccountRoster, useChatAccountUsage,
  useUsageDisplay, useUsageMode, type UsageDisplay, type UsageEntry, type UsageMode, type UsageState,
} from "./chatAccountUsage";

type T = Dict;

/**
 * 对话窗底部常驻状态条 + 用量面板 + 账号面板，**照 Orca 源码逐个元素移植**（主人第三次打回：
 * 「同样的数据、同样的主题，放在一起看分不出是两个产品」）。三层分别对应：
 * - 状态条：StatusBarSurface（24px、titlebar/card 底、上边框）里的 StatusBarUsageArea + ProviderSegment；
 * - 点开：UsageRosterPanel（440px 无内边距菜单：标题行、详细/精简、每家一行、管理账号）；
 * - 悬停或点那一行：ClaudeSwitcherForOwner 的子菜单（ProviderIdentityCard + AccountSwitchList）。
 * 尺寸、字号、颜色以 Orca 夹具的计算样式为准（scratchpad 的 dump 对照），样式在 styles.css 的 .ou 块。
 *
 * 和 Orca 刻意不同的地方（各有理由，PR 里有对照表）：
 * - 用量面板只列这个对话的 agent，副标题写「此对话」而不是「所有代理」：这条状态条属于一个对话窗。
 * - 没有「使用详情与历史」：Meowo 没有 token 用量统计页，放一个点了没去处的入口是骗人。
 * - 账号行点了不切换账号：Orca 的状态条讲「当前账号」，Meowo 这条讲「这个对话」，切换只对之后新开的
 *   对话生效，放在这里点一下会让人以为当前对话也换了。所以行没有悬停底色（Orca 也只给能点的行上底色）。
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
  const now = useMinuteClock();
  const sections = usedSections(usage.usage?.lanes);
  const compactBar = mode === "compact";

  return (
    <div className="chat-statusbar ou" data-testid="chat-statusbar" role="contentinfo">
      <div className="ou-usage-row" ref={wrapRef}>
        {!supportsAccount ? (
          <span className="ou-trigger is-static" title={t.chat.usageBarUnsupported(agentName)}>
            <span className="ou-seg ou-muted-50"><ProviderIcon provider={provider} /> --</span>
          </span>
        ) : (
          <>
            <button
              ref={btnRef}
              type="button"
              className={"ou-trigger" + (compactBar ? " is-compact" : "")}
              data-testid="chat-statusbar-usage"
              aria-label={t.chat.usageBarTitle}
              aria-haspopup="menu"
              aria-expanded={open}
              onClick={() => setOpen((v) => !v)}
            >
              {sections.length > 0 && <UnitLabel display={display} compact={compactBar} t={t} />}
              <Segment provider={provider} state={usage} sections={sections} mode={mode} display={display} now={now} t={t} />
            </button>
            <button
              type="button"
              className="ou-bar-refresh"
              data-testid="chat-statusbar-refresh"
              aria-label={t.chat.usageRefresh}
              title={t.chat.usageRefresh}
              disabled={usage.fetching}
              onClick={usage.retry}
            >
              <Lucide name="refresh" size={11} className={usage.fetching ? "is-spinning" : undefined} />
            </button>
            {open && (
              <Roster
                provider={provider}
                profile={profile}
                state={usage}
                sections={sections}
                display={display}
                mode={mode}
                now={now}
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

/** 倒计时按分钟走（Orca useResetCountdownClock 按标签边界刷新，分钟粒度看起来一样）。 */
function useMinuteClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

/** Orca UsageUnitLabel：详细是一个灰字；精简多一根细竖线把它和数字隔开。 */
function UnitLabel({ display, compact, t }: { display: UsageDisplay; compact: boolean; t: T }) {
  const label = display === "used" ? t.chat.usageUsed : t.chat.usageRemaining;
  if (!compact) return <span className="ou-muted">{label}</span>;
  return <span className="ou-unit-compact ou-muted">{label}<i aria-hidden="true" /></span>;
}

/** Orca ProviderSegment。 */
function Segment({ provider, state, sections, mode, display, now, t }: {
  provider: string; state: UsageState; sections: OSection[]; mode: UsageMode; display: UsageDisplay; now: number; t: T;
}) {
  const compact = mode === "compact";
  const head = tightest(sections, "all");
  // 首次读取：Orca 是静态的「···」（不闪，失焦窗口不重绘）。
  if (!head && (state.loading || (!state.usage && !state.failure))) {
    return <span className="ou-seg is-pending ou-muted"><ProviderIcon provider={provider} /><span>···</span></span>;
  }
  if (!head) {
    if (state.failure) {
      return (
        <span className="ou-seg is-pending ou-muted" title={usageFailureText(state.failure, t, "")}>
          <ProviderIcon provider={provider} />
          <Lucide name="alert" size={11} className="ou-alert" />
          <span className="ou-seg-status">{shortFailure(state.failure, t)}</span>
        </span>
      );
    }
    return <span className="ou-seg ou-muted-50"><ProviderIcon provider={provider} /> --</span>;
  }
  // 精简只留最紧那一项的百分比（时间窗口不写名字，Fable 这类留名字），窗口名和倒计时进悬停提示。
  const list = compact ? [head] : sections;
  const title = compact ? `${panelLabel(head, t)} · ${shown(head.used, display)}%${head.resetsAt ? ` · ${resetsCaption(head.resetsAt, now, t)}` : ""}` : undefined;
  return (
    <span className={"ou-seg" + (compact ? " is-compact" : "")} title={title}>
      <ProviderIcon provider={provider} />
      {!compact && <UsageBar used={head.used} display={display} className="ou-mini" />}
      {list.map((s, i) => {
        const label = compact ? (s.scoped ? chipLabel(s, now) : "") : chipLabel(s, now);
        return (
          <Fragment key={`${s.lane.kind}-${i}`}>
            {i > 0 && <span className="ou-muted">·</span>}
            {compact ? (
              <span className="ou-num">
                <span className="ou-fg">{shown(s.used, display)}<span className="ou-unit">%</span></span>
                {label && <span className="ou-muted">{` ${label}`}</span>}
              </span>
            ) : (
              <span className="ou-num">{`${shown(s.used, display)}% ${label}`}</span>
            )}
          </Fragment>
        );
      })}
      {state.failure && <Lucide name="alert" size={11} className="ou-alert" />}
    </span>
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

/** 面板里的时长：中文 2小时 43分钟，英文 2h 43m（Orca formatLocalizedResetDuration）。 */
function localDuration(at: number, now: number, t: T): string | null {
  return fmtDuration(at - now, (v, u) => t.chat.usageUnit[u](v));
}
/** 「2小时 43分钟后重置」（Orca formatResetsInCaption）。 */
function resetsCaption(at: number, now: number, t: T): string {
  const d = localDuration(at, now, t);
  return d == null ? t.chat.usageResetsNow : t.chat.usageResetsIn(d);
}

/** Orca UsageDisplayToggle：「已用 ⇅」，点一下在已用/剩余之间切。 */
function DisplayToggle({ display, t }: { display: UsageDisplay; t: T }) {
  const next: UsageDisplay = display === "used" ? "remaining" : "used";
  return (
    <button
      type="button"
      className="ou-toggle"
      data-testid="chat-statusbar-toggle"
      title={next === "remaining" ? t.chat.usageShowRemaining : t.chat.usageShowUsed}
      onClick={() => setUsageDisplay(next)}
    >
      {display === "used" ? t.chat.usageUsed : t.chat.usageRemaining}
      <Lucide name="updown" size={12} strokeWidth={1.5} />
    </button>
  );
}

/** Orca UsageRosterPanel（点开状态条的那一层）。 */
function Roster({ provider, profile, state, sections, display, mode, now, t, agentName, onClose }: {
  provider: string; profile: string | null; state: UsageState; sections: OSection[];
  display: UsageDisplay; mode: UsageMode; now: number; t: T; agentName: string; onClose: () => void;
}) {
  const [panel, setPanel] = useState(false);
  const rowRef = useRef<HTMLButtonElement>(null);
  const rosterRef = useRef<HTMLDivElement>(null);
  const hoverTimer = useRef<number | null>(null);
  const roster = useAccountRoster(provider, profile, true, panel);
  const own = roster.rows?.find((p) => p.id === profile);
  const plan = own?.account?.plan ?? null;
  const manage = () => { onClose(); openSettingsAt("account"); };
  const clearHover = () => { if (hoverTimer.current != null) { window.clearTimeout(hoverTimer.current); hoverTimer.current = null; } };
  useEffect(() => clearHover, []);
  const detailed = mode === "detailed";
  const head = tightest(sections, "primary");
  const soonest = sections.map((s) => s.resetsAt).filter((x): x is number => x != null);
  const pending = soonest.filter((x) => x > now);
  const nextReset = soonest.length ? Math.min(...(pending.length ? pending : soonest)) : null;

  return (
    <div className="ou-menu ou-roster" role="menu" aria-label={t.chat.usageBarTitle} data-testid="chat-statusbar-card" ref={rosterRef}>
      <div className="ou-roster-head">
        <div className="ou-roster-title ou-muted">
          <span className="ou-roster-name">{t.chat.usageRosterTitle}</span>
          <span className="ou-roster-scope">· {t.chat.usageRosterScope}</span>
          <button
            type="button"
            className="ou-item ou-icon-item"
            data-testid="chat-statusbar-refresh-all"
            aria-label={t.chat.usageRefresh}
            title={t.chat.usageRefresh}
            onClick={() => { state.retry(); roster.refreshAll(); }}
          >
            <Lucide name="refresh" size={15} strokeWidth={1.5} className={state.fetching ? "is-spinning" : undefined} />
          </button>
        </div>
        <DisplayToggle display={display} t={t} />
      </div>
      {/* 详细 / 精简（Orca SettingsSegmentedControl size="sm" equalWidth）：管状态条那一格和这一行的详略。 */}
      <div className="ou-roster-seg-wrap">
        <div className="ou-seg-ctl" role="radiogroup" aria-label={t.chat.usageModeLabel}>
          {(["detailed", "compact"] as const).map((m) => (
            <button
              key={m}
              type="button"
              role="radio"
              aria-checked={mode === m}
              data-testid={`chat-statusbar-mode-${m}`}
              className={mode === m ? "is-on" : undefined}
              title={m === "detailed" ? t.chat.usageModeDetailedTip : t.chat.usageModeCompactTip}
              onClick={() => setUsageMode(m)}
            >
              {m === "detailed" ? t.chat.usageModeDetailed : t.chat.usageModeCompact}
            </button>
          ))}
        </div>
      </div>
      <div className="ou-rule" />
      <button
        ref={rowRef}
        type="button"
        className={"ou-item ou-roster-row" + (panel ? " is-open" : "")}
        data-testid="chat-statusbar-provider-row"
        aria-haspopup="menu"
        aria-expanded={panel}
        onClick={() => setPanel((v) => !v)}
        onPointerEnter={() => { clearHover(); hoverTimer.current = window.setTimeout(() => setPanel(true), 100); }}
        onPointerLeave={clearHover}
      >
        <span className="ou-urow" data-usage-mode={detailed ? "verbose" : "compact"}>
          <span className="ou-urow-line">
            <span className="ou-urow-icon"><ProviderIcon provider={provider} /></span>
            <span className={"ou-urow-name" + (detailed && sections.length ? "" : " is-min")}>
              {agentName}
              {plan && <span className="ou-urow-plan"> · {plan}</span>}
            </span>
            {sections.length === 0 ? (
              <span className="ou-urow-status">
                {state.failure ? shortFailure(state.failure, t) : state.fetching || !state.usage ? t.chat.usageLoading : t.chat.usageNoData}
              </span>
            ) : detailed ? (
              nextReset != null && <span className="ou-urow-reset">{resetsCaption(nextReset, now, t)}</span>
            ) : head ? (
              <span className="ou-urow-summary">
                {head.resetsAt != null && <span className="ou-urow-reset">{localDuration(head.resetsAt, now, t) ?? t.chat.usageNow}</span>}
                <UsageBar used={head.used} display={display} className="ou-g30" />
                <span className="ou-urow-value">{shown(head.used, display)}%</span>
              </span>
            ) : (
              <span className="ou-urow-summary"><span className="ou-urow-status">{t.chat.usageNotChecked}</span></span>
            )}
          </span>
          {detailed && sections.length > 0 && (
            <span className="ou-urow-metrics">
              {sections.map((s, i) => (
                <span key={`${s.lane.kind}-${i}`} className="ou-metric" title={`${panelLabel(s, t)} · ${shown(s.used, display)}%`}>
                  <span className="ou-metric-label">{shortLabel(s, t)}</span>
                  <UsageBar used={s.used} display={display} className="ou-g28" />
                  <span className="ou-metric-value">{shown(s.used, display)}%</span>
                </span>
              ))}
            </span>
          )}
        </span>
        <Lucide name="chevron" size={14} strokeWidth={1.5} className="ou-chevron" />
      </button>
      <div className="ou-rule" />
      <button type="button" className="ou-item ou-roster-foot" onClick={manage} onPointerEnter={() => { clearHover(); setPanel(false); }}>
        {t.chat.usageManageAccounts}
        <Lucide name="chevron" size={14} strokeWidth={1.5} className="ou-chevron" />
      </button>
      {panel && (
        <AccountPanel
          anchor={rowRef}
          roster={rosterRef}
          provider={provider}
          profile={profile}
          state={state}
          sections={sections}
          rows={roster.rows}
          display={display}
          now={now}
          t={t}
          agentName={agentName}
          manage={manage}
        />
      )}
    </div>
  );
}

/**
 * Orca ClaudeSwitcherForOwner 的子菜单：右侧贴着那一行展开（Radix SubContent：side right、alignOffset -5），
 * 右边放不下就翻到左边，两边都放不下就贴着窗口右缘（Radix 的 shift）。
 */
function AccountPanel({ anchor, roster, provider, profile, state, sections, rows, display, now, t, agentName, manage }: {
  anchor: React.RefObject<HTMLButtonElement | null>; roster: React.RefObject<HTMLDivElement | null>;
  provider: string; profile: string | null; state: UsageState; sections: OSection[]; rows: ProfileView[] | null;
  display: UsageDisplay; now: number; t: T; agentName: string; manage: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const a = anchor.current?.getBoundingClientRect();
    const r = roster.current?.getBoundingClientRect();
    const me = ref.current?.getBoundingClientRect();
    if (!a || !r || !me) return;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left = r.right;
    if (left + me.width > vw - 8) left = r.left - me.width >= 8 ? r.left - me.width : Math.max(8, vw - 8 - me.width);
    // 底部留出状态条（Orca collisionPadding bottom 32）。
    const top = Math.max(8, Math.min(a.top - 5, vh - 32 - me.height));
    setPos({ left: left - r.left, top: top - r.top });
  }, [anchor, roster, rows, sections.length]);
  const signedIn = (rows ?? []).filter((p) => p.account);
  const signedOut = (rows ?? []).filter((p) => !p.account);
  const own = rows?.find((p) => p.id === profile);
  return (
    <div
      ref={ref}
      className="ou-menu ou-panel"
      role="menu"
      data-testid="chat-statusbar-panel"
      style={pos ? { left: pos.left, top: pos.top } : { visibility: "hidden" }}
    >
      <div className="ou-panel-pad">
        <div className="ou-idcard">
          <div className="ou-idcard-head">
            <div className="ou-idcard-provider"><ProviderIcon provider={provider} />{agentName}</div>
            <span className="ou-muted">{state.fetchedAt != null ? t.chat.usageUpdatedAgo(timeAgo(state.fetchedAt, t, now)) : t.chat.usageNotYetUpdated}</span>
          </div>
          <div className="ou-card">
            <div className="ou-card-body">
              <div className="ou-card-top">
                <div className="ou-card-label">{own ? profileLabel(own, t) : ""}</div>
                <DisplayToggle display={display} t={t} />
              </div>
              <CardUsage state={state} sections={sections} display={display} now={now} t={t} agentName={agentName} />
            </div>
          </div>
        </div>
      </div>
      {/* 账号列表（Orca AccountSwitchSection）：只有一个账号时它就是上面那张卡，改成「管理账号…」。 */}
      {(rows?.length ?? 0) > 1 && (
        <div className="ou-acct-list" data-testid="chat-statusbar-roster">
          <div className="ou-label ou-label-row">
            <span className="ou-label-name">{t.chat.usageAccounts}</span>
            <span className="ou-label-col">{display === "used" ? t.chat.usageUsed : t.chat.usageRemaining}</span>
          </div>
          <div className="ou-acct-scroll">
            {signedIn.map((p) => (
              <AccountRow
                key={p.id ?? ""}
                p={p}
                t={t}
                display={display}
                agentName={agentName}
                current={p.id === profile}
                entry={p.id === profile ? { usage: state.usage, failure: state.failure, at: 0, fetchedAt: state.fetchedAt } : getUsageEntry(provider, p.id)}
                fetching={isUsageFetching(provider, p.id)}
              />
            ))}
            {signedOut.length > 0 && (
              <>
                <div className="ou-label ou-label-group">{t.chat.usageSignedOut}</div>
                {signedOut.map((p) => (
                  <div key={p.id ?? ""} className="ou-acct" data-testid="chat-statusbar-acct">
                    <div className="ou-acct-item">
                      <span className="ou-acct-check" />
                      <span className="ou-acct-name">
                        <span className="ou-acct-label">{profileLabel(p, t)}</span>
                        <span className="ou-acct-detail">{[t.chat.usageSignedOut, orgOf(p, t)].filter(Boolean).join(" · ")}</span>
                      </span>
                    </div>
                    <button type="button" className="ou-signin" aria-label={`${t.chat.usageSignIn}: ${profileLabel(p, t)}`} onClick={manage}>{t.chat.usageSignIn}</button>
                  </div>
                ))}
              </>
            )}
          </div>
        </div>
      )}
      <div className="ou-sep" />
      <button type="button" className="ou-item ou-panel-manage" onClick={manage}>{t.chat.usageManageAccounts}</button>
    </div>
  );
}

/** 卡片里这个账号的各窗口（Orca ProviderPanel embedded）或出错说明（ErrorMessage）。 */
function CardUsage({ state, sections, display, now, t, agentName }: {
  state: UsageState; sections: OSection[]; display: UsageDisplay; now: number; t: T; agentName: string;
}) {
  if (state.loading) {
    return <div className="ou-skeleton" aria-label={t.chat.chatUsageLoading}><i /><i /></div>;
  }
  if (sections.length === 0) {
    if (!state.failure) return <span className="ou-muted">{t.chat.usageNoData}</span>;
    return (
      <div className="ou-err" role="status" title={usageFailureText(state.failure, t, agentName)}>
        <div className="ou-err-label">{shortFailure(state.failure, t)}</div>
        <div className="ou-muted">{usageFailureText(state.failure, t, agentName)}</div>
      </div>
    );
  }
  return (
    <div className="ou-windows">
      {sections.map((s, i) => (
        <div key={`${s.lane.kind}-${i}`} className="ou-window" data-usage-window={panelLabel(s, t)}>
          <div className="ou-window-top">
            <span className="ou-window-name">
              <span className="ou-window-label">{panelLabel(s, t)}</span>
              {s.resetsAt != null && <span className="ou-muted">{` · ${resetsCaption(s.resetsAt, now, t)}`}</span>}
            </span>
            <span className="ou-window-value">{shown(s.used, display)}%</span>
          </div>
          {/* 色阶始终按已用算（Orca：Tone always follows consumption）。 */}
          <UsageBar used={s.used} display={display} tone="health" className="ou-window-bar" />
        </div>
      ))}
      {state.failure && (
        <div className="ou-err" role="status">
          <div className="ou-err-label">{t.chat.usageRefreshFailedCached}</div>
          <div className="ou-muted">{usageFailureText(state.failure, t, agentName)}</div>
        </div>
      )}
    </div>
  );
}

/** 账号行副标题：组织名，没有就「个人」；系统默认不写（Orca buildClaudeStatusSwitchGroups）。 */
function orgOf(p: ProfileView, t: T): string | null {
  if (p.id === null) return null;
  return p.account?.organization?.trim() || t.chat.usagePersonal;
}

/** Orca AccountSwitchRowItem + AccountUsageGauge。 */
function AccountRow({ p, t, display, agentName, current, entry, fetching }: {
  p: ProfileView; t: T; display: UsageDisplay; agentName: string;
  current: boolean; entry: UsageEntry | undefined; fetching: boolean;
}) {
  const sections = usedSections(entry?.usage?.lanes);
  const head = tightest(sections, "primary");
  let right: React.ReactNode;
  if (head) {
    right = (
      <div className={"ou-gauge" + (fetching ? " is-fetching" : "")} role="img" aria-label={`${shown(head.used, display)}%`}>
        <UsageBar used={head.used} display={display} tone="health" className="ou-g30" />
        <span className="ou-gauge-value">{shown(head.used, display)}%</span>
      </div>
    );
  } else if (fetching && !entry?.usage) {
    right = <div className="ou-skeleton ou-skeleton-row"><i /><i /></div>;
  } else if (entry?.failure && entry.failure.kind === "expired") {
    right = <span className="ou-acct-signin-note" title={usageFailureText(entry.failure, t, agentName)}>{t.chat.usageSignInToSee}</span>;
  } else if (entry?.failure) {
    right = <span className="ou-acct-signin-note" title={usageFailureText(entry.failure, t, agentName)}>{shortFailure(entry.failure, t)}</span>;
  } else {
    // 还没读过（Orca「未检查」：原因放提示里，不占一行字）。
    right = <span className="ou-not-checked" data-usage-not-checked="" title={t.chat.usageNotCheckedTip}>{t.chat.usageNotChecked}</span>;
  }
  const detail = orgOf(p, t);
  const label = profileLabel(p, t);
  return (
    <div className="ou-acct" data-current={current ? "true" : undefined} data-testid="chat-statusbar-acct" aria-current={current ? "true" : undefined}>
      <div className="ou-acct-item" title={detail ? `${label}\n${detail}` : label}>
        <span className="ou-acct-check">
          {current && <Lucide name="check" size={14} strokeWidth={1.5} />}
        </span>
        <span className="ou-acct-name">
          <span className="ou-acct-label">{label}</span>
          {detail && <span className="ou-acct-detail">{detail}</span>}
        </span>
        <div className="ou-acct-usage">{right}</div>
      </div>
    </div>
  );
}
