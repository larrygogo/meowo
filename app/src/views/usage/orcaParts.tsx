import type { UsageLane } from "../../api";
import type { Dict } from "../../i18n/zh";
import { AgentAppIcon } from "../chat/AgentAppIcon";

/**
 * 从 Orca 源码移植的用量小部件（主人第三次打回：「同样的数据、同样的主题，放在一起看分不出是两个产品」）。
 * 结构、尺寸、颜色都照 orca-src 的原文件抄，出处写在每个函数上；样式在 styles.css 的 .ou 块，
 * 用 Orca 自己的设计变量值，不套 Meowo 的 token。
 */

/** 一个用量窗口（Orca 的 UsageSection：session / weekly / fableWeekly …）。 */
export type OSection = {
  lane: UsageLane;
  used: number;
  resetsAt: number | null;
  /** 只限一个模型的窗口（Fable / Opus），不代表普通请求的额度（Orca：isScopedUsageWindow）。 */
  scoped: boolean;
};

const clamp = (n: number) => Math.max(0, Math.min(100, n));

/** Orca usedSections：有百分比的窗口，顺序照后端给的（5 小时、7 天、模型周限）。余额不是窗口。 */
export function usedSections(lanes: UsageLane[] | undefined): OSection[] {
  return (lanes ?? [])
    .filter((l) => l.used_pct != null && l.kind !== "balance")
    .map((l) => {
      const ts = l.resets_at ? Date.parse(l.resets_at) : NaN;
      return { lane: l, used: clamp(l.used_pct as number), resetsAt: Number.isNaN(ts) ? null : ts, scoped: l.kind === "model_weekly" || l.kind === "opus" };
    });
}

/** Orca getTightestUsageSection：scope 'primary' 跳过 Fable 这类模型窗口，'all'（状态条）不跳。 */
export function tightest(sections: OSection[], scope: "primary" | "all"): OSection | null {
  const pool = scope === "all" ? sections : sections.filter((s) => !s.scoped);
  return pool.reduce<OSection | null>((a, s) => (!a || s.used > a.used ? s : a), null);
}

/** 显示的百分比：已用或剩余（Orca getDisplayedUsagePercentage）。 */
export function shown(used: number, display: "used" | "remaining"): number {
  return Math.round(display === "remaining" ? 100 - clamp(used) : clamp(used));
}

/** Orca usage-health-tone：80% 起黄、90% 起红，按已用算，与显示已用还是剩余无关。 */
export function healthTone(used: number): "healthy" | "warning" | "urgent" {
  return used >= 90 ? "urgent" : used >= 80 ? "warning" : "healthy";
}

/** 窗口名（Orca getWindowSectionLabel）：5h / 每周 / Fable。 */
export function panelLabel(s: OSection, t: Dict): string {
  if (s.lane.kind === "five_hour") return "5h";
  if (s.scoped) return s.lane.label ?? (s.lane.kind === "opus" ? "Opus" : "Fable");
  return t.chat.usageWeekly;
}
/** 短窗口名（Orca shortUsageLabel）：5h / 周 / Fable。 */
export function shortLabel(s: OSection, t: Dict): string {
  if (s.lane.kind === "five_hour") return "5h";
  if (s.scoped) return s.lane.label ?? (s.lane.kind === "opus" ? "Opus" : "Fable");
  return t.chat.usageWeekShort;
}

/** Orca formatResetDuration：向下取整到分钟，47m / 3h 54m / 6d 7h。fmt 决定单位怎么写。 */
export function fmtDuration(ms: number, fmt: (v: number, u: "minute" | "hour" | "day") => string): string | null {
  if (ms <= 0) return null;
  const total = Math.floor(ms / 60_000);
  if (total < 60) return fmt(total, "minute");
  const h = Math.floor(total / 60);
  const m = total % 60;
  if (h >= 24) {
    const d = Math.floor(h / 24);
    return h % 24 > 0 ? `${fmt(d, "day")} ${fmt(h % 24, "hour")}` : fmt(d, "day");
  }
  return m > 0 ? `${fmt(h, "hour")} ${fmt(m, "minute")}` : fmt(h, "hour");
}
/** 状态条上的倒计时恒为 2h 43m（Orca 状态条不本地化单位）。 */
export const chipUnit = (v: number, u: "minute" | "hour" | "day") => `${v}${u === "minute" ? "m" : u === "hour" ? "h" : "d"}`;

/** 状态条那一格的窗口名：时间窗口写剩余时长，模型窗口写模型名（Orca formatRateLimitWindowChipLabel）。 */
export function chipLabel(s: OSection, now: number): string {
  if (s.scoped) return s.lane.label ?? "Fable";
  if (s.resetsAt != null) return fmtDuration(s.resetsAt - now, chipUnit) ?? "now";
  return s.lane.kind === "five_hour" ? "5h" : "wk";
}

/** Orca formatTimeAgo。 */
export function timeAgo(ts: number, t: Dict, now = Date.now()): string {
  const diff = now - ts;
  if (diff < 60_000) return t.chat.usageJustNow;
  const mins = Math.floor(diff / 60_000);
  if (mins < 60) return t.chat.usageMinutesAgo(mins);
  return t.chat.usageHoursAgo(Math.floor(mins / 60));
}

/** Orca UsageBar：中性底轨，填充是显示的百分比；tone="health" 只在账号详情卡里按用量上色，别处灰色。 */
export function UsageBar({ used, display, tone = "plain", className }: {
  used: number; display: "used" | "remaining"; tone?: "plain" | "health"; className: string;
}) {
  return (
    <span className={"ou-bar-track " + className} data-usage-bar="">
      <span className={"ou-bar-fill is-" + (tone === "plain" ? "plain" : healthTone(used))} style={{ width: `${shown(used, display)}%` }} />
    </span>
  );
}

/** lucide 图标（Orca 用的同一套路径，24 视框、2px 描边）。 */
const LUCIDE: Record<string, string[]> = {
  refresh: ["M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"],
  updown: ["m21 16-4 4-4-4", "M17 20V4", "m3 8 4-4 4 4", "M7 4v16"],
  chevron: ["m9 18 6-6-6-6"],
  check: ["M20 6 9 17l-5-5"],
  alert: ["m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4", "M12 17h.01"],
};
export function Lucide({ name, size, className, strokeWidth = 2 }: { name: keyof typeof LUCIDE; size: number; className?: string; strokeWidth?: number }) {
  return (
    <svg className={className} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {LUCIDE[name].map((d) => <path key={d} d={d} />)}
    </svg>
  );
}

/** Orca ProviderIcon：13px。图形用 Meowo 的应用图标（Claude 的 logomark 和 Orca provider-icons 的 ClaudeIcon 同形同色）。 */
export function ProviderIcon({ provider }: { provider: string }) {
  return <AgentAppIcon provider={provider} className="ou-provider-icon ou-provider-icon-app" />;
}
