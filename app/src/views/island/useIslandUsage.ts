// 灵动岛的额度数据。取法与贴纸底栏一致（Sticker.tsx 的用量 effect）：先用账号快照里的缓存
// 预填，再对有账号、支持用量、没走中转的 provider 每 5 分钟刷新一次。岛常驻，贴纸只在展开
// 完整列表时挂载，所以这里单独取，不依赖贴纸。
import { useCallback, useEffect, useRef, useState } from "react";
import { getAccounts, refreshUsage, type ProviderUsage } from "../../api";
import { useTauriEvent } from "../../hooks/useTauriEvent";

export type IslandUsage = {
  provider: string;
  /** 主泳道（5 小时窗口优先）的已用百分比。 */
  pct: number;
  kind: string;
  /** 这家的数字不是刚取到的（刷新失败，显示的是缓存）。 */
  stale?: boolean;
  /** 上次成功取到的时刻（ms）；只有账号快照里的缓存、从没刷新成功过时为 null。 */
  fetchedAt?: number | null;
};

/** 每家额度的新鲜度（backlog 仍存在 #1，2026-11-12 核实：贴纸当年为 7B-8 做的 usageMeta 没搬到岛上，
 *  `refreshUsage().catch(() => {})` 吞掉错误，网络一抖或 token 过期，岛上显示的就是几小时前的值，
 *  且不会有任何提示）。failed = 最近一次刷新失败；fetchedAt = 上次成功刷新的时刻。 */
export type IslandUsageMeta = { fetchedAt: number | null; failed: boolean };

/** 账号可能变化的事件：登录完成、安装完成。岛常驻，原来只在挂载时读一次账号，之后新登录的
 *  provider 进不了刷新列表，要重启 Meowo 才出额度（backlog 仍存在 #1）。 */
export const USAGE_ACCOUNT_EVENTS = ["login-done", "install-done"] as const;
/** 这家当前用的账号变了（切换、删除、并回默认），payload 是 provider id。后端此刻已清掉它的用量缓存。
 *  岛上那份数字属于上一个账号：立刻丢掉并重取，不等五分钟的定时刷新（用户实拍 2026-11-15：
 *  「切换账号之后灵动岛上的用量没有刷新成新的账号的」）。 */
export const ACTIVE_PROFILE_CHANGED = "active-profile-changed";

export function useIslandUsage(): { usage: Record<string, ProviderUsage>; meta: Record<string, IslandUsageMeta> } {
  const [usage, setUsage] = useState<Record<string, ProviderUsage>>({});
  const [meta, setMeta] = useState<Record<string, IslandUsageMeta>>({});
  // 账号代数：登录/安装完成时加一，effect 重读账号、按新名单重排刷新定时器。
  const [generation, setGeneration] = useState(0);
  const bump = useCallback(() => setGeneration((g) => g + 1), []);
  useTauriEvent(USAGE_ACCOUNT_EVENTS[0], bump);
  useTauriEvent(USAGE_ACCOUNT_EVENTS[1], bump);
  // 刚换过账号的那几家：账号快照里即使还带着缓存也不拿来预填。后端按理已清缓存，但这里不赌
  // 它每条路径都清了（删除账号那条就漏过，独立审查 2026-11-15）——预填回去就是旧账号的数字。
  const switched = useRef(new Set<string>());
  useTauriEvent<string>(ACTIVE_PROFILE_CHANGED, ({ payload: provider }) => {
    switched.current.add(provider);
    // 先丢旧值再重取：留着它，新账号就顶着旧账号的额度，取数失败时还会被标成「缓存」继续显示。
    const drop = <T,>(m: Record<string, T>) => {
      if (!(provider in m)) return m;
      const next = { ...m };
      delete next[provider];
      return next;
    };
    setUsage(drop);
    setMeta(drop);
    bump();
  });
  useEffect(() => {
    let cancelled = false;
    const timers: number[] = [];
    getAccounts()
      .then((accounts) => {
        if (cancelled || !Array.isArray(accounts)) return;
        setUsage((cur) => {
          const next = { ...cur };
          accounts.forEach((a) => {
            if (a.relay_enabled) delete next[a.provider];
            else if (!next[a.provider] && a.usage && !switched.current.has(a.provider)) next[a.provider] = a.usage;
          });
          return next;
        });
        accounts
          .filter((a) => a.account != null && a.usage_supported && !a.relay_enabled)
          .forEach(({ provider }) => {
            const refresh = () => {
              if (cancelled) return;
              refreshUsage(provider)
                .then((u) => {
                  if (cancelled) return;
                  switched.current.delete(provider);
                  setUsage((m) => ({ ...m, [provider]: u }));
                  setMeta((m) => ({ ...m, [provider]: { fetchedAt: Date.now(), failed: false } }));
                })
                .catch(() => {
                  if (cancelled) return;
                  // 旧值留着（总比空着强），但标成缓存：展示层降饱和并写上次更新时间。
                  setMeta((m) => ({ ...m, [provider]: { fetchedAt: m[provider]?.fetchedAt ?? null, failed: true } }));
                });
            };
            refresh();
            timers.push(window.setInterval(refresh, 5 * 60_000));
          });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      timers.forEach((id) => window.clearInterval(id));
    };
  }, [generation]);
  return { usage, meta };
}

/// 主泳道：5 小时窗口优先，其次周/7 天，再次第一条有百分比的。
export function primaryLane(usage: ProviderUsage | undefined): { pct: number; kind: string } | null {
  if (!usage) return null;
  const order = ["five_hour", "seven_day", "weekly"];
  for (const kind of order) {
    const lane = usage.lanes.find((l) => l.kind === kind && l.used_pct != null);
    if (lane) return { pct: lane.used_pct as number, kind };
  }
  const any = usage.lanes.find((l) => l.used_pct != null);
  return any ? { pct: any.used_pct as number, kind: any.kind } : null;
}

/// 岛上额度的排序：先按「正在用的」（activeProviders：在跑/要处理的会话的 agent，按要紧程度），
/// 再按「贴纸底栏显示配额」的设置顺序，最后是其余有数据的。只留有数据的。
/// 此前只按设置顺序取第一个：设置里 kimi 排第一，会话全是 claude 时环上也固定是 kimi（用户反馈）。
export function orderIslandUsage(
  usage: Record<string, ProviderUsage>,
  providers: readonly string[],
  activeProviders: readonly string[] = [],
  meta: Record<string, IslandUsageMeta> = {},
): IslandUsage[] {
  const ordered = [...activeProviders, ...providers, ...Object.keys(usage)]
    .filter((p, i, all) => all.indexOf(p) === i);
  const out: IslandUsage[] = [];
  for (const provider of ordered) {
    const lane = primaryLane(usage[provider]);
    if (!lane) continue;
    const m = meta[provider];
    out.push(m?.failed ? { provider, ...lane, stale: true, fetchedAt: m.fetchedAt } : { provider, ...lane });
  }
  return out;
}

/** 额度陈旧时的提示：知道上次成功时刻就写时间，否则只说显示的是缓存。 */
export function usageStaleTip(
  fetchedAt: number | null | undefined,
  t: { locale: string; sticker: { usageStale: (time: string) => string; staleNotice: string } },
): string {
  if (fetchedAt == null) return t.sticker.staleNotice;
  const time = new Date(fetchedAt).toLocaleTimeString(t.locale, { hour: "2-digit", minute: "2-digit", hour12: false });
  return t.sticker.usageStale(time);
}

/// 收起胶囊上额度环轮换的那几家：只取岛上正有会话的 agent（用户要求，2026-11-04：岛上两个
/// claude 会话时环上还在轮 kimi 的额度，不相干）。都没数据就是空，额度环不画。
export function liveIslandUsage(
  usage: Record<string, ProviderUsage>,
  providers: readonly string[],
  liveProviders: readonly string[],
  meta: Record<string, IslandUsageMeta> = {},
): IslandUsage[] {
  return orderIslandUsage(usage, providers, liveProviders, meta).filter((u) => liveProviders.includes(u.provider));
}
