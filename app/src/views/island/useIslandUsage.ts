// 灵动岛的额度数据。取法与贴纸底栏一致（Sticker.tsx 的用量 effect）：先用账号快照里的缓存
// 预填，再对有账号、支持用量、没走中转的 provider 每 5 分钟刷新一次。岛常驻，贴纸只在展开
// 完整列表时挂载，所以这里单独取，不依赖贴纸。
import { useEffect, useState } from "react";
import { getAccounts, refreshUsage, type ProviderUsage } from "../../api";

export type IslandUsage = {
  provider: string;
  /** 主泳道（5 小时窗口优先）的已用百分比。 */
  pct: number;
  kind: string;
};

/// enabled=false 时不取（完整列表开着，贴纸底栏自己在取，两边同时刷是重复请求）。已取到的保留。
export function useIslandUsage(enabled = true): Record<string, ProviderUsage> {
  const [usage, setUsage] = useState<Record<string, ProviderUsage>>({});
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const timers: number[] = [];
    getAccounts()
      .then((accounts) => {
        if (cancelled || !Array.isArray(accounts)) return;
        setUsage((cur) => {
          const next = { ...cur };
          accounts.forEach((a) => {
            if (a.relay_enabled) delete next[a.provider];
            else if (!next[a.provider] && a.usage) next[a.provider] = a.usage;
          });
          return next;
        });
        accounts
          .filter((a) => a.account != null && a.usage_supported && !a.relay_enabled)
          .forEach(({ provider }) => {
            const refresh = () => {
              if (cancelled) return;
              refreshUsage(provider)
                .then((u) => { if (!cancelled) setUsage((m) => ({ ...m, [provider]: u })); })
                .catch(() => {});
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
  }, [enabled]);
  return usage;
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
): IslandUsage[] {
  const ordered = [...activeProviders, ...providers, ...Object.keys(usage)]
    .filter((p, i, all) => all.indexOf(p) === i);
  const out: IslandUsage[] = [];
  for (const provider of ordered) {
    const lane = primaryLane(usage[provider]);
    if (lane) out.push({ provider, ...lane });
  }
  return out;
}

/// 收起胶囊上额度环轮换的那几家：只取岛上正有会话的 agent（用户要求，2026-11-04：岛上两个
/// claude 会话时环上还在轮 kimi 的额度，不相干）。都没数据就是空，额度环不画。
export function liveIslandUsage(
  usage: Record<string, ProviderUsage>,
  providers: readonly string[],
  liveProviders: readonly string[],
): IslandUsage[] {
  return orderIslandUsage(usage, providers, liveProviders).filter((u) => liveProviders.includes(u.provider));
}
