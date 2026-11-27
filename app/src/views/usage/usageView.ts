// 用量显示的共享工具：设置页的配额区、对话页的上下文卡片都用（之前在设置页里私有）。
import type { UsageLane } from "../../api";
import type { Dict } from "../../i18n/zh";

export function fmtResetIn(iso: string, t: Dict): string {
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return "";
  const now = Date.now();
  const diffMs = ts - now;
  if (diffMs <= 0) return t.account.resetSoon;
  // 按自然日差判断：今天显示剩余小时/分钟，跨天则用相对词/日期并精确到钟点。
  const startOf = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const dayDiff = Math.round((startOf(ts) - startOf(now)) / 86_400_000);
  if (dayDiff <= 0) {
    const min = Math.round(diffMs / 60000);
    if (min < 1) return t.account.resetSoon; // 剩余不足半分钟时 round 得 0，归入「即将重置」
    if (min < 60) return t.account.resetInMin(min);
    const h = Math.floor(min / 60);
    const m = min % 60;
    return m > 0 ? t.account.resetInHourMin(h, m) : t.account.resetInHour(h);
  }
  // 时刻/日期走 Intl：手拼 `pad(getHours())` 是硬编码 24 小时制，英文用户看到的是
  // "Resets tomorrow 14:30" 而非 "2:30 PM"；月/日顺序同理随 locale。
  const r = new Date(ts);
  const clock = new Intl.DateTimeFormat(t.locale, { hour: "numeric", minute: "2-digit" }).format(r);
  if (dayDiff === 1) return t.account.resetTomorrow(clock);
  if (dayDiff === 2) return t.account.resetDayAfter(clock);
  const date = new Intl.DateTimeFormat(t.locale, { month: "short", day: "numeric" }).format(r);
  return t.account.resetOnDate(date, clock);
}

export function laneLabel(lane: UsageLane, t: Dict): string {
  switch (lane.kind) {
    case "five_hour": return lane.label ? t.account.laneGroupFiveHour(lane.label) : t.account.laneFiveHour;
    case "seven_day": return t.account.laneSevenDay;
    case "opus": return t.account.laneOpus;
    case "weekly": return lane.label ? t.account.laneModelWeekly(lane.label) : t.account.laneWeekly;
    // 模型专属周限(claude 的 Fable/Opus 周配额):模型名由 API 下发,缺失时退回通用周配额文案。
    case "model_weekly": return lane.label ? t.account.laneModelWeekly(lane.label) : t.account.laneWeekly;
    case "balance": return t.account.laneBalance;
    default: return lane.kind;
  }
}

// note 是后端机器哨兵串（claude 发 "extra_usage_enabled"、codex 发 "credits:45.5"），
// 映射为本地化文案；未知格式原样显示以向后兼容。
export function renderNote(note: string, t: Dict): string {
  if (note === "extra_usage_enabled") return t.account.extraUsage;
  if (note.startsWith("credits:")) return t.account.credits(note.slice("credits:".length));
  return note;
}

/** 用量请求失败的分类（后端 refresh_usage 的错误码，照 Orca 的错误分类）。 */
export type UsageFailure =
  | { kind: "unsupported" }
  | { kind: "expired" }
  | { kind: "rateLimited"; retryAfterSec: number | null }
  | { kind: "server" }
  | { kind: "network" }
  | { kind: "unknown" };

export function usageFailure(e: unknown): UsageFailure {
  const s = String(e);
  if (s.includes("USAGE_UNSUPPORTED")) return { kind: "unsupported" };
  if (s.includes("USAGE_EXPIRED")) return { kind: "expired" };
  const limited = /USAGE_RATE_LIMITED(?::(\d+))?/.exec(s);
  if (limited) return { kind: "rateLimited", retryAfterSec: limited[1] ? Number(limited[1]) : null };
  if (s.includes("USAGE_SERVER")) return { kind: "server" };
  if (s.includes("USAGE_NETWORK")) return { kind: "network" };
  return { kind: "unknown" };
}

/** 失败原因的一句话：说清发生了什么、要不要做点什么。`agent` 是展示名（「Claude Code」）。 */
export function usageFailureText(f: UsageFailure, t: Dict, agent: string): string {
  switch (f.kind) {
    case "unsupported": return t.account.usageUnsupported;
    case "expired": return t.account.usageExpired(agent);
    case "rateLimited": return t.account.usageRateLimited(f.retryAfterSec != null ? Math.max(1, Math.ceil(f.retryAfterSec / 60)) : null);
    case "server": return t.account.usageServer;
    case "network": return t.account.usageNetwork;
    default: return t.account.usageUnavailable;
  }
}

/** 进度条色阶（照 Orca：≥80% 提醒色、≥90% 警示色）。 */
export function usageTone(pct: number): "" | " is-warn" | " is-full" {
  return pct >= 90 ? " is-full" : pct >= 80 ? " is-warn" : "";
}
