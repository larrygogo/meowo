import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useT } from "../../i18n";
import { listProfiles, refreshProfileUsage, type AgentId, type ProviderUsage } from "../../api";
import { profileLabel } from "../../profileLabel";
import { usageFailure, type UsageFailure } from "../usage/usageView";

/**
 * 对话所用账号的额度：底部状态条常驻读它（照 Orca 的 status bar usage）。
 * 原先藏在输入框圆环的悬停卡片里（ContextMeter），主人反馈「只有悬停圆环才看得到额度」，
 * 抽出来给状态条用；圆环照 Orca 的分工只管上下文。
 */
export type UsageState = {
  label: string | null;
  usage: ProviderUsage | null;
  failure: UsageFailure | null;
  loading: boolean;
  fetching: boolean;
  fetchedAt: number | null;
  retry: () => void;
};

/** 账号额度在各对话之间共用一份：同一账号 5 分钟内不重复请求（照 Orca 的去重窗口）。 */
const usageCache = new Map<string, { usage: ProviderUsage | null; failure: UsageFailure | null; at: number; fetchedAt: number | null }>();
const REUSE_MS = 5 * 60_000;
/** 测试用：模块级缓存跨用例会串。 */
export function resetChatUsageCache() { usageCache.clear(); }

export function useChatAccountUsage(provider: string | null, profile: string | null, active: boolean): UsageState {
  const key = provider ? `${provider}|${profile ?? ""}` : "";
  const [, bump] = useState(0);
  const [fetching, setFetching] = useState(false);
  const [label, setLabel] = useState<string | null>(null);
  const inflight = useRef(false);
  const t = useT();

  const load = useCallback((force: boolean) => {
    if (!provider || inflight.current) return;
    const hit = usageCache.get(key);
    if (!force && hit && Date.now() - hit.at < REUSE_MS) { bump((n) => n + 1); return; }
    inflight.current = true;
    setFetching(true);
    refreshProfileUsage(provider as AgentId, profile)
      .then((u) => usageCache.set(key, { usage: u, failure: null, at: Date.now(), fetchedAt: Date.now() }))
      // 出错保留上次的数字（照 Orca），只换上失败原因。
      .catch((e) => usageCache.set(key, { usage: hit?.usage ?? null, failure: usageFailure(e), at: Date.now(), fetchedAt: hit?.fetchedAt ?? null }))
      .finally(() => { inflight.current = false; setFetching(false); bump((n) => n + 1); });
  }, [key, provider, profile]);

  useEffect(() => {
    if (!active || !provider) return;
    load(false);
    // 账号名（起过名的显示名字，否则邮箱）：本地读，不联网。
    listProfiles(provider as AgentId)
      .then((rows) => {
        const row = rows.find((p) => p.id === profile);
        setLabel(row ? [profileLabel(row, t), row.account?.plan].filter(Boolean).join(" · ") : null);
      })
      .catch(() => setLabel(null));
    // 常驻显示：窗口重新聚焦时、以及每 5 分钟各刷一次（照 Orca：focus 时刷新、窗口在前台才轮询），
    // 都受 5 分钟去重约束，不会比设置页更勤。
    const onFocus = () => load(false);
    window.addEventListener("focus", onFocus);
    const tick = window.setInterval(() => { if (document.visibilityState === "visible") load(false); }, REUSE_MS);
    return () => { window.removeEventListener("focus", onFocus); window.clearInterval(tick); };
  }, [active, provider, profile, load, t]);

  const hit = usageCache.get(key);
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

/** 显示「已用」还是「剩余」（照 Orca 的 Used/Left 切换）。只是看法偏好，存本机 localStorage。 */
export type UsageDisplay = "used" | "remaining";
const DISPLAY_KEY = "meowo-usage-display";
const listeners = new Set<() => void>();
function readDisplay(): UsageDisplay {
  try { return localStorage.getItem(DISPLAY_KEY) === "remaining" ? "remaining" : "used"; } catch { return "used"; }
}
export function setUsageDisplay(d: UsageDisplay) {
  try { localStorage.setItem(DISPLAY_KEY, d); } catch { /* 隐私模式禁写：本次会话内仍按内存切 */ }
  memDisplay = d;
  listeners.forEach((l) => l());
}
let memDisplay: UsageDisplay | null = null;
export function useUsageDisplay(): UsageDisplay {
  return useSyncExternalStore(
    (cb) => { listeners.add(cb); return () => { listeners.delete(cb); }; },
    () => memDisplay ?? readDisplay(),
  );
}
/** 测试用。 */
export function resetUsageDisplay() { memDisplay = null; try { localStorage.removeItem(DISPLAY_KEY); } catch { /* 无 */ } }

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
