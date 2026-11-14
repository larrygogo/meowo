import { useEffect, useRef, useState } from "react";
import {
  getAccounts,
  refreshUsage,
  type ProviderAccountPayload,
  type ProviderUsage,
  type UsageLane,
} from "../api";
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

/// 远程设置里的只读账号用量。打开时只读桌面缓存，不逐家联网；用户点某家的刷新才走
/// refresh_usage（后端仍有 60s 限频），避免每次切换设置页都扇出一组 API 请求。
export function RemoteUsage() {
  const t = useT();
  const mounted = useRef(true);
  const [accounts, setAccounts] = useState<ProviderAccountPayload[] | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [usage, setUsage] = useState<Record<string, ProviderUsage>>({});
  const [errors, setErrors] = useState<Record<string, UsageError | undefined>>({});
  const [refreshing, setRefreshing] = useState<Record<string, boolean>>({});
  const [updatedAt, setUpdatedAt] = useState<Record<string, number>>({});

  useEffect(() => {
    mounted.current = true;
    getAccounts()
      .then((list) => {
        if (!mounted.current) return;
        setLoadFailed(false);
        setAccounts(list);
        const cached: Record<string, ProviderUsage> = {};
        list.forEach((account) => {
          if (account.usage) cached[account.provider] = account.usage;
        });
        setUsage(cached);
      })
      .catch(() => {
        if (mounted.current) setLoadFailed(true);
      });
    return () => { mounted.current = false; };
  }, []);

  const refresh = (provider: string) => {
    if (refreshing[provider]) return;
    setRefreshing((current) => ({ ...current, [provider]: true }));
    setErrors((current) => ({ ...current, [provider]: undefined }));
    refreshUsage(provider)
      .then((next) => {
        if (!mounted.current) return;
        setUsage((current) => ({ ...current, [provider]: next }));
        setUpdatedAt((current) => ({ ...current, [provider]: Date.now() }));
      })
      .catch((error) => {
        if (!mounted.current) return;
        const kind = String(error).includes("USAGE_UNSUPPORTED") ? "unsupported" : "error";
        setErrors((current) => ({ ...current, [provider]: kind }));
      })
      .finally(() => {
        if (mounted.current) setRefreshing((current) => ({ ...current, [provider]: false }));
      });
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
        ) : accounts.map((account) => {
          const name = providerLabel(account.provider);
          const { Icon: ProviderIcon } = agentAssets(account.provider);
          const currentUsage = usage[account.provider] ?? null;
          const error = errors[account.provider];
          const canRefresh = account.account != null && account.usage_supported && !account.relay_enabled;
          return (
            <section className="remote-usage-provider" aria-label={name} key={account.provider}>
              <div className="remote-usage-head">
                <span className="remote-usage-logo" style={tintStyle(account.provider)}><ProviderIcon /></span>
                <span className="remote-usage-name">{name}</span>
                {updatedAt[account.provider] != null && (
                  <span className="remote-usage-updated">
                    {t.account.usageUpdatedAt(new Intl.DateTimeFormat(t.locale, { hour: "numeric", minute: "2-digit" }).format(updatedAt[account.provider]))}
                  </span>
                )}
                {canRefresh && (
                  <button
                    type="button"
                    className="remote-usage-refresh"
                    aria-label={t.remote.usageRefreshProvider(name)}
                    title={t.remote.usageRefreshProvider(name)}
                    disabled={error === "unsupported"}
                    {...busyProps(!!refreshing[account.provider], () => refresh(account.provider))}
                  >
                    <Icon glyph={icRefreshCw} size={14} className={refreshing[account.provider] ? "spin" : undefined} />
                  </button>
                )}
              </div>
              <div className="remote-usage-body">
                {account.account == null ? (
                  <div className="usage-stale">{t.account.notLoggedIn}</div>
                ) : account.relay_enabled ? (
                  <div className="usage-stale">{t.account.relayActive}</div>
                ) : currentUsage ? (
                  <>
                    {currentUsage.lanes.map((lane, index) => (
                      <UsageBar key={`${lane.kind}-${index}`} lane={lane} label={laneLabel(lane, t)} />
                    ))}
                    {currentUsage.note && <div className="usage-extra">{renderNote(currentUsage.note, t)}</div>}
                    {error === "error" && <div className="usage-stale" role="alert">{t.account.refreshFailed}</div>}
                  </>
                ) : !account.usage_supported || error === "unsupported" ? (
                  <div className="usage-stale">{t.account.usageUnsupported}</div>
                ) : error === "error" ? (
                  <div className="usage-stale" role="alert">{t.account.usageUnavailable}</div>
                ) : (
                  <div className="usage-stale">{t.remote.usageNoCache}</div>
                )}
              </div>
            </section>
          );
        })}
      </div>
    </>
  );
}
