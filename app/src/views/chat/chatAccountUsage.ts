import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { useT } from "../../i18n";
import { listProfiles, refreshProfileUsage, type AgentId, type ProfileView, type ProviderUsage } from "../../api";
import { profileLabel } from "../../profileLabel";
import { usageFailure, type UsageFailure } from "../usage/usageView";

/**
 * 对话窗底部状态条读的账号额度（照 Orca 的 status bar usage + Claude 账号面板）。
 *
 * 每个账号一份，按 `provider|profile` 分键，所有对话共用：
 * - 这个对话所用的账号常驻读：挂上时、窗口回到前台时、前台每 5 分钟各一次，5 分钟内不重复请求；
 * - 其余账号**只在打开面板时**读，一个接一个（上一个回来再问下一个），整轮 60 秒内不再来一轮——
 *   照 Orca 的 fetchInactiveClaudeAccountsOnOpen（顺序 await、INACTIVE_FETCH_DEBOUNCE_MS = 60s），
 *   多个账号一起拉也不会同时打到官方接口上触发 429。
 * 后端 refresh_profile_usage 再兜一层：每个账号按自己的 CLAUDE_CONFIG_DIR 先看 statusline，
 * 失败按 30 秒翻倍退避、429 照 Retry-After 等，退避期内直接回上次的错误码，不联网。
 */
export type UsageEntry = { usage: ProviderUsage | null; failure: UsageFailure | null; at: number; fetchedAt: number | null };

const cache = new Map<string, UsageEntry>();
const inflight = new Map<string, Promise<void>>();
const listeners = new Set<() => void>();
let version = 0;
function emit() { version += 1; listeners.forEach((l) => l()); }
function subscribe(cb: () => void) { listeners.add(cb); return () => { listeners.delete(cb); }; }
function useCacheVersion() { return useSyncExternalStore(subscribe, () => version); }

const REUSE_MS = 5 * 60_000;
/** 其余账号整轮读取的去抖（照 Orca：INACTIVE_FETCH_DEBOUNCE_MS）。 */
const OTHERS_DEBOUNCE_MS = 60_000;
const lastOthersCycle = new Map<string, number>();

export const usageKey = (provider: string, profile: string | null) => `${provider}|${profile ?? ""}`;
export function getUsageEntry(provider: string, profile: string | null): UsageEntry | undefined {
  return cache.get(usageKey(provider, profile));
}
export function isUsageFetching(provider: string, profile: string | null): boolean {
  return inflight.has(usageKey(provider, profile));
}

/** 测试用：模块级缓存跨用例会串。 */
export function resetChatUsageCache() { cache.clear(); inflight.clear(); lastOthersCycle.clear(); emit(); }

/** 读一个账号；5 分钟内读过且不强制就直接用缓存。同一账号同时只有一个请求。 */
export function loadUsage(provider: string, profile: string | null, force: boolean): Promise<void> {
  const key = usageKey(provider, profile);
  const running = inflight.get(key);
  if (running) return running;
  const hit = cache.get(key);
  if (!force && hit && Date.now() - hit.at < REUSE_MS) return Promise.resolve();
  const p = refreshProfileUsage(provider as AgentId, profile)
    .then((u) => { cache.set(key, { usage: u, failure: null, at: Date.now(), fetchedAt: Date.now() }); })
    // 出错保留上次的数字（照 Orca），只换上失败原因。
    .catch((e) => { cache.set(key, { usage: hit?.usage ?? null, failure: usageFailure(e), at: Date.now(), fetchedAt: hit?.fetchedAt ?? null }); })
    .finally(() => { inflight.delete(key); emit(); });
  inflight.set(key, p);
  emit();
  return p;
}

/**
 * 面板打开时读其余账号：一个接一个，跳过未登录的；`force`（点刷新）不受 60 秒去抖限制，
 * 但每个账号仍走后端的退避。返回的函数用来中止（面板关了就不再往下问）。
 */
export function loadOtherAccounts(provider: string, profiles: (string | null)[], force: boolean): () => void {
  let stopped = false;
  const last = lastOthersCycle.get(provider) ?? 0;
  if (!force && Date.now() - last < OTHERS_DEBOUNCE_MS) return () => {};
  lastOthersCycle.set(provider, Date.now());
  void (async () => {
    for (const profile of profiles) {
      if (stopped) return;
      await loadUsage(provider, profile, force);
    }
  })();
  return () => { stopped = true; };
}

export type UsageState = {
  label: string | null;
  usage: ProviderUsage | null;
  failure: UsageFailure | null;
  loading: boolean;
  fetching: boolean;
  fetchedAt: number | null;
  retry: () => void;
};

/** 这个对话所用账号的额度：常驻读（见文件头）。 */
export function useChatAccountUsage(provider: string | null, profile: string | null, active: boolean): UsageState {
  useCacheVersion();
  const [label, setLabel] = useState<string | null>(null);
  const t = useT();
  const load = useCallback((force: boolean) => {
    if (provider) void loadUsage(provider, profile, force);
  }, [provider, profile]);

  useEffect(() => {
    if (!active || !provider) return;
    load(false);
    listProfiles(provider as AgentId)
      .then((rows) => {
        const row = rows.find((p) => p.id === profile);
        setLabel(row ? [profileLabel(row, t), row.account?.plan].filter(Boolean).join(" · ") : null);
      })
      .catch(() => setLabel(null));
    const onFocus = () => load(false);
    window.addEventListener("focus", onFocus);
    const tick = window.setInterval(() => { if (document.visibilityState === "visible") load(false); }, REUSE_MS);
    return () => { window.removeEventListener("focus", onFocus); window.clearInterval(tick); };
  }, [active, provider, profile, load, t]);

  const hit = provider ? getUsageEntry(provider, profile) : undefined;
  const fetching = provider ? isUsageFetching(provider, profile) : false;
  return {
    label,
    usage: hit?.usage ?? null,
    failure: hit?.failure ?? null,
    loading: fetching && !hit?.usage,
    fetching,
    fetchedAt: hit?.fetchedAt ?? null,
    retry: () => load(true),
  };
}

/** 面板里的账号列表：系统默认 + 所有已添加的账号；打开面板时按上面的规则读其余账号。 */
export function useAccountRoster(provider: string, conversationProfile: string | null, open: boolean) {
  useCacheVersion();
  const [rows, setRows] = useState<ProfileView[] | null>(null);
  const [refreshTick, setRefreshTick] = useState(0);
  useEffect(() => {
    if (!open) return;
    let stop = () => {};
    let cancelled = false;
    listProfiles(provider as AgentId)
      .then((list) => {
        if (cancelled) return;
        setRows(list);
        // 未登录的不问（问了也只会回「需要登录」），这个对话的账号由状态条那条常驻读管。
        const others = list.filter((p) => p.account && p.id !== conversationProfile).map((p) => p.id);
        stop = loadOtherAccounts(provider, others, refreshTick > 0);
      })
      .catch(() => { if (!cancelled) setRows([]); });
    return () => { cancelled = true; stop(); };
  }, [provider, conversationProfile, open, refreshTick]);
  return { rows, refreshAll: () => setRefreshTick((n) => n + 1) };
}

/** 显示「已用」还是「剩余」（照 Orca 的 Used/Left 切换）。只是看法偏好，存本机 localStorage。 */
export type UsageDisplay = "used" | "remaining";
/** 状态条与账号行的详略（照 Orca 的 Detailed/Compact）。 */
export type UsageMode = "detailed" | "compact";

function prefStore<T extends string>(key: string, fallback: T, values: readonly T[]) {
  let mem: T | null = null;
  const ls = new Set<() => void>();
  const read = (): T => {
    if (mem) return mem;
    try { const v = localStorage.getItem(key) as T | null; return v && values.includes(v) ? v : fallback; } catch { return fallback; }
  };
  return {
    use: () => useSyncExternalStore((cb) => { ls.add(cb); return () => { ls.delete(cb); }; }, read),
    set: (v: T) => {
      try { localStorage.setItem(key, v); } catch { /* 隐私模式禁写：本次会话内仍按内存切 */ }
      mem = v;
      ls.forEach((l) => l());
    },
    reset: () => { mem = null; try { localStorage.removeItem(key); } catch { /* 无 */ } },
  };
}
const displayPref = prefStore<UsageDisplay>("meowo-usage-display", "used", ["used", "remaining"]);
const modePref = prefStore<UsageMode>("meowo-usage-mode", "detailed", ["detailed", "compact"]);
export const useUsageDisplay = displayPref.use;
export const setUsageDisplay = displayPref.set;
export const useUsageMode = modePref.use;
export const setUsageMode = modePref.set;
/** 测试用。 */
export function resetUsageDisplay() { displayPref.reset(); modePref.reset(); }

/** 按「已用/剩余」换算显示的百分比；色阶始终按已用算（照 Orca）。 */
export function shownPct(usedPct: number, d: UsageDisplay): number {
  const u = Math.max(0, Math.min(100, usedPct));
  return Math.round(d === "remaining" ? 100 - u : u);
}

/** 状态条上的短倒计时：3h 39m / 3d 13h / 12m（照 Orca 状态条，中文界面也用这种紧凑写法）。 */
export function fmtResetShort(iso: string, now = Date.now()): string {
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return "";
  const min = Math.max(0, Math.round((ts - now) / 60_000));
  if (min < 60) return `${min}m`;
  const h = Math.floor(min / 60);
  if (h < 24) return min % 60 ? `${h}h ${min % 60}m` : `${h}h`;
  const d = Math.floor(h / 24);
  return h % 24 ? `${d}d ${h % 24}h` : `${d}d`;
}
