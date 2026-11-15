import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  applyActiveProfileToSessions,
  getAccounts,
  listProfiles,
  refreshProfileUsage,
  refreshUsage,
  sessionsOffActiveProfileCount,
  setActiveProfile,
  type ProfileView,
  type ProviderAccountPayload,
  type ProviderUsage,
  type UsageLane,
} from "../api";
import { appConfirm } from "../confirm";
import { formatBackendError } from "../i18n/errors";
import { useT } from "../i18n";
import type { Dict } from "../i18n/zh";
import { agentAssets, tintStyle } from "../providers";
import { Icon } from "../icons/Icon";
import { icRefreshCw } from "../icons/glyphs";
import { busyProps } from "../busyButton";

function providerLabel(provider: string): string {
  return provider ? provider[0].toUpperCase() + provider.slice(1) : provider;
}

function fmtResetIn(iso: string, t: Dict): string {
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return "";
  const now = Date.now();
  const diffMs = ts - now;
  if (diffMs <= 0) return t.account.resetSoon;
  const startOf = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const dayDiff = Math.round((startOf(ts) - startOf(now)) / 86_400_000);
  if (dayDiff <= 0) {
    const min = Math.round(diffMs / 60000);
    if (min < 1) return t.account.resetSoon;
    if (min < 60) return t.account.resetInMin(min);
    const h = Math.floor(min / 60);
    const m = min % 60;
    return m > 0 ? t.account.resetInHourMin(h, m) : t.account.resetInHour(h);
  }
  const r = new Date(ts);
  const clock = new Intl.DateTimeFormat(t.locale, { hour: "numeric", minute: "2-digit" }).format(r);
  if (dayDiff === 1) return t.account.resetTomorrow(clock);
  if (dayDiff === 2) return t.account.resetDayAfter(clock);
  const date = new Intl.DateTimeFormat(t.locale, { month: "short", day: "numeric" }).format(r);
  return t.account.resetOnDate(date, clock);
}

function laneLabel(lane: UsageLane, t: Dict): string {
  switch (lane.kind) {
    case "five_hour": return t.account.laneFiveHour;
    case "seven_day": return t.account.laneSevenDay;
    case "opus": return t.account.laneOpus;
    case "weekly": return t.account.laneWeekly;
    case "model_weekly": return lane.label ? t.account.laneModelWeekly(lane.label) : t.account.laneWeekly;
    case "balance": return t.account.laneBalance;
    default: return lane.kind;
  }
}

function renderNote(note: string, t: Dict): string {
  if (note === "extra_usage_enabled") return t.account.extraUsage;
  if (note.startsWith("credits:")) return t.account.credits(note.slice("credits:".length));
  return note;
}

function UsageBar({ lane, label }: { lane: UsageLane; label: string }) {
  const t = useT();
  if (lane.used_pct != null) {
    const pct = Math.max(0, Math.min(100, lane.used_pct));
    return (
      <div className="usage-row">
        <div className="usage-head">
          <span className="usage-label">{label}</span>
          <span className="usage-pct">{pct.toFixed(0)}%</span>
        </div>
        <div className="usage-track" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)}>
          <i style={{ width: `${pct}%` }} />
        </div>
        {lane.resets_at && <div className="usage-reset">{fmtResetIn(lane.resets_at, t)}</div>}
      </div>
    );
  }
  const valText = lane.used != null ? `${lane.used}${lane.unit ? ` ${lane.unit}` : ""}` : "—";
  return (
    <div className="usage-row">
      <div className="usage-head">
        <span className="usage-label">{label}</span>
        <span className="usage-pct">{valText}</span>
      </div>
    </div>
  );
}

type UsageError = "unsupported" | "error";

const usageErrorKind = (error: unknown): UsageError =>
  String(error).includes("USAGE_UNSUPPORTED") ? "unsupported" : "error";

/** 默认账号在前端的行 key：后端给的 id 是 null（同桌面 ProfileList 的 DEFAULT_KEY）。 */
const DEFAULT_KEY = "__default__";
const profileKey = (p: ProfileView) => p.id ?? DEFAULT_KEY;

function UsageLanes({ usage }: { usage: ProviderUsage }) {
  const t = useT();
  return (
    <>
      {usage.lanes.map((lane, index) => (
        <UsageBar key={`${lane.kind}-${index}`} lane={lane} label={laneLabel(lane, t)} />
      ))}
      {usage.note && <div className="usage-extra">{renderNote(usage.note, t)}</div>}
    </>
  );
}

/// 远程设置里的账号与用量。打开时只读桌面缓存 + 每家一次本地 list_profiles，不逐家联网；
/// 用户点刷新或点开非当前账号才查用量（后端仍有 60s 限频）。
/// 账号切换在手机上放开（2026-11-15 leader：「手机上也能看到多个账号，以及切换」）；
/// 登录、退出、新建、删除、改名、合并要交互式终端或不可逆，仍只在电脑上做。
export function RemoteUsage() {
  const t = useT();
  const mounted = useRef(true);
  const [accounts, setAccounts] = useState<ProviderAccountPayload[] | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  // 每家切换一次 +1，作为小节 key 的一部分：切完整节重挂载，旧账号的用量、刷新时刻、
  // 正在看的账号一并清掉，不会把旧额度挂在新账号头上。
  const [generation, setGeneration] = useState<Record<string, number>>({});
  // 「账号已切、重启会话失败」的错误由这里保管：小节重挂载后仍要就地显示。
  const [switchErrors, setSwitchErrors] = useState<Record<string, string | undefined>>({});

  const load = useCallback(() => {
    getAccounts()
      .then((list) => {
        if (!mounted.current) return;
        setLoadFailed(false);
        setAccounts(list);
      })
      .catch(() => {
        if (mounted.current) setLoadFailed(true);
      });
  }, []);

  useEffect(() => {
    mounted.current = true;
    load();
    return () => { mounted.current = false; };
  }, [load]);

  const onSwitchStart = (provider: string) =>
    setSwitchErrors((current) => ({ ...current, [provider]: undefined }));
  const onSwitched = (provider: string, error: string | null) => {
    setSwitchErrors((current) => ({ ...current, [provider]: error ?? undefined }));
    setGeneration((current) => ({ ...current, [provider]: (current[provider] ?? 0) + 1 }));
    load();
  };

  return (
    <>
      <div className="remote-settings-group">{t.remote.usageTitle}</div>
      <div className="remote-settings-card remote-usage-card">
        {loadFailed ? (
          <div className="usage-stale" role="alert">{t.account.accountsLoadFailed}</div>
        ) : accounts == null ? (
          <div className="usage-stale">{t.account.loading}</div>
        ) : accounts.length === 0 ? (
          <div className="usage-stale">{t.remote.usageEmpty}</div>
        ) : accounts.map((account) => (
          <ProviderUsageSection
            key={`${account.provider}#${generation[account.provider] ?? 0}`}
            account={account}
            switchError={switchErrors[account.provider] ?? null}
            onSwitchStart={() => onSwitchStart(account.provider)}
            onSwitched={(error) => onSwitched(account.provider, error)}
          />
        ))}
      </div>
      <div className="remote-usage-hint">{t.remote.profilesDesktopOnly}</div>
    </>
  );
}

function ProviderUsageSection({ account, switchError, onSwitchStart, onSwitched }: {
  account: ProviderAccountPayload;
  switchError: string | null;
  onSwitchStart: () => void;
  /** 账号已切成（error 非空 = 切了但重启会话失败）。由父级重载整页。 */
  onSwitched: (error: string | null) => void;
}) {
  const t = useT();
  const provider = account.provider;
  const name = providerLabel(provider);
  const { Icon: ProviderIcon } = agentAssets(provider);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  // 当前账号：缓存 + 手动刷新。
  const [usage, setUsage] = useState<ProviderUsage | null>(account.usage);
  const [error, setError] = useState<UsageError | undefined>(undefined);
  const [refreshing, setRefreshing] = useState(false);
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  // 父级重载后（切换完成）缓存跟着换。
  useEffect(() => { setUsage(account.usage); }, [account.usage]);

  // 账号列表：只读本地文件。多于一条才出列表（gemini 这类单账号家恒为一条）。
  const [profiles, setProfiles] = useState<ProfileView[] | null>(null);
  const [profilesFailed, setProfilesFailed] = useState(false);
  useEffect(() => {
    listProfiles(provider)
      .then((rows) => {
        if (!mounted.current) return;
        setProfilesFailed(false);
        setProfiles(rows);
      })
      .catch(() => {
        if (mounted.current) setProfilesFailed(true);
      });
  }, [provider]);

  // 正在看的非当前账号（null = 看当前账号）。它的用量只住在这一节，不写当前账号缓存。
  const [viewing, setViewing] = useState<ProfileView | null>(null);
  const [viewUsage, setViewUsage] = useState<ProviderUsage | null>(null);
  const [viewError, setViewError] = useState<UsageError | null>(null);
  const [viewLoading, setViewLoading] = useState(false);
  const [viewUpdatedAt, setViewUpdatedAt] = useState<number | null>(null);
  const viewRequest = useRef(0);

  const [switching, setSwitching] = useState(false);
  const [localSwitchError, setLocalSwitchError] = useState<string | null>(null);

  const refreshActive = () => {
    if (refreshing) return;
    setRefreshing(true);
    setError(undefined);
    refreshUsage(provider)
      .then((next) => {
        if (!mounted.current) return;
        setUsage(next);
        setUpdatedAt(Date.now());
      })
      .catch((e) => {
        if (mounted.current) setError(usageErrorKind(e));
      })
      .finally(() => {
        if (mounted.current) setRefreshing(false);
      });
  };

  const loadViewUsage = (profile: ProfileView) => {
    const request = ++viewRequest.current;
    setViewLoading(true);
    setViewError(null);
    refreshProfileUsage(provider, profile.id)
      .then((next) => {
        if (!mounted.current || viewRequest.current !== request) return;
        setViewUsage(next);
        setViewUpdatedAt(Date.now());
      })
      .catch((e) => {
        if (mounted.current && viewRequest.current === request) setViewError(usageErrorKind(e));
      })
      .finally(() => {
        if (mounted.current && viewRequest.current === request) setViewLoading(false);
      });
  };

  const view = (profile: ProfileView) => {
    // 作废进行中的查询：快速连点两个账号时，先发的结果不许盖住后点的那个。
    viewRequest.current += 1;
    setViewUsage(null);
    setViewError(null);
    setViewLoading(false);
    setViewUpdatedAt(null);
    if (profile.active) {
      setViewing(null);
      return;
    }
    setViewing(profile);
    // 未登录的账号没有用量可查，不白发一次请求。
    if (profile.account) loadViewUsage(profile);
  };

  /**
   * 切换活跃账号：与桌面 ProfileList.switchTo 同一套——先切设置，再问后端有几个运行中的
   * 会话不在新账号上（搬不了会话的家后端恒回 0，于是不问），点头才重启。重启失败不连累
   * 切换：设置已写进去，照常整页重载，错误交给父级保管、就地显示。
   */
  const switchTo = async (profile: ProfileView) => {
    if (switching || profile.active) return;
    setSwitching(true);
    setLocalSwitchError(null);
    onSwitchStart();
    try {
      await setActiveProfile(provider, profile.id);
    } catch (e) {
      // 没切成：账号原样，留在本节说明。
      if (mounted.current) {
        setLocalSwitchError(formatBackendError(e, t.locale));
        setSwitching(false);
      }
      return;
    }
    let restartError: string | null = null;
    try {
      // 必须切完再问：判据是「和当前活跃账号不同」。
      const running = await sessionsOffActiveProfileCount(provider);
      if (running > 0) {
        const ok = await appConfirm(t.account.switchRestartConfirm(running), {
          title: t.account.switchProfile,
          // 会杀进程（正在生成的那一轮回答会丢），按危险操作办。
          danger: true,
          confirmLabel: t.account.switchRestartConfirmLabel,
        });
        if (ok) await applyActiveProfileToSessions(provider);
      }
    } catch (e) {
      restartError = formatBackendError(e, t.locale);
    }
    onSwitched(restartError);
  };

  const multi = profiles != null && profiles.length > 1;
  const viewingKey = viewing ? profileKey(viewing) : null;
  const activeProfile = profiles?.find((p) => p.active) ?? null;
  const canRefresh = viewing
    ? viewing.account != null && !account.relay_enabled
    : account.account != null && account.usage_supported && !account.relay_enabled;
  const isRefreshing = viewing ? viewLoading : refreshing;
  const refreshDisabled = viewing ? viewError === "unsupported" : error === "unsupported";
  const shownUpdatedAt = viewing ? viewUpdatedAt : updatedAt;
  const shownSwitchError = localSwitchError ?? switchError;

  let body: ReactNode;
  if (viewing) {
    body = !viewing.account ? (
      <div className="usage-stale">{t.account.notLoggedIn}</div>
    ) : account.relay_enabled ? (
      <div className="usage-stale">{t.account.relayActive}</div>
    ) : viewUsage ? (
      <UsageLanes usage={viewUsage} />
    ) : viewError === "unsupported" ? (
      <div className="usage-stale">{t.account.usageUnsupported}</div>
    ) : viewError === "error" ? (
      <div className="usage-stale" role="alert">{t.account.usageUnavailable}</div>
    ) : (
      <div className="usage-stale">{t.account.loading}</div>
    );
  } else {
    body = account.account == null ? (
      <div className="usage-stale">{t.account.notLoggedIn}</div>
    ) : account.relay_enabled ? (
      <div className="usage-stale">{t.account.relayActive}</div>
    ) : usage ? (
      <>
        <UsageLanes usage={usage} />
        {error === "error" && <div className="usage-stale" role="alert">{t.account.refreshFailed}</div>}
      </>
    ) : !account.usage_supported || error === "unsupported" ? (
      <div className="usage-stale">{t.account.usageUnsupported}</div>
    ) : error === "error" ? (
      <div className="usage-stale" role="alert">{t.account.usageUnavailable}</div>
    ) : (
      <div className="usage-stale">{t.remote.usageNoCache}</div>
    );
  }

  return (
    <section className="remote-usage-provider" aria-label={name}>
      <div className="remote-usage-head">
        <span className="remote-usage-logo" style={tintStyle(provider)}><ProviderIcon /></span>
        <span className="remote-usage-name">{name}</span>
        {shownUpdatedAt != null && (
          <span className="remote-usage-updated">
            {t.account.usageUpdatedAt(new Intl.DateTimeFormat(t.locale, { hour: "numeric", minute: "2-digit" }).format(shownUpdatedAt))}
          </span>
        )}
        {canRefresh && (
          <button
            type="button"
            className="remote-usage-refresh"
            aria-label={t.remote.usageRefreshProvider(name)}
            title={t.remote.usageRefreshProvider(name)}
            disabled={refreshDisabled}
            {...busyProps(isRefreshing, () => (viewing ? loadViewUsage(viewing) : refreshActive()))}
          >
            <Icon glyph={icRefreshCw} size={14} className={isRefreshing ? "spin" : undefined} />
          </button>
        )}
      </div>
      {multi && (
        <div className="remote-profile-list" role="group" aria-label={t.remote.profilesOf(name)}>
          {profiles.map((p) => {
            const key = profileKey(p);
            const label = p.name || t.account.defaultProfile;
            // 描述行说「这是哪个账号」，套餐是徽章（同桌面 ProfileList）。
            const desc = p.account?.email ?? p.account?.display_name ?? p.account?.login_label ?? t.account.notLoggedIn;
            const selected = viewingKey ? viewingKey === key : p.active;
            return (
              <div key={key} className={"remote-profile-row" + (selected ? " is-selected" : "")}>
                {/* 点行＝看这个账号的用量（无副作用，大热区）；切换是行尾独立按钮——触屏滚动时
                    误触行的概率高，切换会改全局账号还可能引出重启确认（decisions.md 2026-11-15）。 */}
                <button
                  type="button"
                  className="remote-profile-main"
                  aria-pressed={selected}
                  aria-label={t.account.viewProfileUsage(label)}
                  onClick={() => view(p)}
                >
                  <span className="remote-profile-name-row">
                    <span className="remote-profile-name">{label}</span>
                    {p.account?.plan && <span className="profile-badge profile-badge-plan">{p.account.plan}</span>}
                  </span>
                  <span className="remote-profile-desc">{desc}</span>
                </button>
                {p.active ? (
                  <span className="profile-badge">{t.account.activeProfile}</span>
                ) : (
                  <button
                    type="button"
                    className="provider-card-action provider-card-action-sm remote-profile-switch"
                    aria-label={t.remote.switchToProfile(label)}
                    {...busyProps(switching, () => void switchTo(p))}
                  >
                    {t.remote.switchProfile}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}
      {profilesFailed && <div className="usage-stale" role="alert">{t.remote.profilesLoadFailed}</div>}
      {shownSwitchError && <div className="remote-settings-error" role="alert">{shownSwitchError}</div>}
      {multi && (
        <div className="remote-usage-for">
          {t.account.usageFor(viewing ? viewing.name || t.account.defaultProfile : activeProfile?.name || t.account.defaultProfile)}
        </div>
      )}
      <div className="remote-usage-body">{body}</div>
    </section>
  );
}
