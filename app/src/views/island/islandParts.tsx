// 灵动岛的零件：眼睛、带进度环的 agent 图标、额度环、时钟。收起胶囊、提醒、展开面板共用。
// 视觉照用户给的参考图（2026-11-04）：纯黑底、白字、环形进度、状态用颜色而不是文字堆叠。
import { useEffect, useState } from "react";
import { agentAssets, tintStyle } from "../../providers";
import { cardTone } from "../sticker/helpers";
import type { Item } from "../sticker/types";

export type IslandTone = "error" | "pending" | "running" | "waiting" | "idle";

/// 两只眼睛（Meowo 标志）。在跑：睁眼左右张望，两侧冒小「+」；要处理：琥珀色、一闪一闪；
/// 出错：红；跑完/闲着：眯成两道弯弯的弧（照参考图，实心椭圆一直睁着显得呆）。动作全在 CSS。
export function Eyes({ tone, small = false }: { tone: IslandTone; small?: boolean }) {
  const closed = tone === "waiting" || tone === "idle";
  return (
    <span className={`isl-eyes${small ? " is-small" : ""}`} data-tone={tone} aria-hidden="true">
      {!small && tone === "running" && <span className="isl-spark is-l">+</span>}
      <svg className="isl-eye-pair" width={small ? 18 : 22} height={small ? 11 : 14} viewBox="0 0 22 14">
        {closed ? (
          <g fill="none" stroke="currentColor" strokeWidth={2.6} strokeLinecap="round">
            <path d="M1.8 9.5 Q5.5 3 9.2 9.5" />
            <path d="M12.8 9.5 Q16.5 3 20.2 9.5" />
          </g>
        ) : (
          <g className="isl-eye-balls" fill="currentColor">
            <ellipse cx="5.5" cy="7" rx="4.2" ry="5.6" />
            <ellipse cx="16.5" cy="7" rx="4.2" ry="5.6" />
          </g>
        )}
      </svg>
      {!small && tone === "running" && <span className="isl-spark is-r">+</span>}
    </span>
  );
}

/// 环形进度。value 为 0..1；null = 不定进度（转圈的一段弧）。
export function Ring({ size, stroke, value, color, track, children, className }: {
  size: number;
  stroke: number;
  value: number | null;
  color: string;
  track: string;
  children?: React.ReactNode;
  className?: string;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  // 定量进度用 dashoffset 画：挂载时从 0 扫到位（CSS 动画从 --isl-c 起），数值变了平滑滑过去
  // （dashoffset 上的 transition）。不定进度是一段固定长度的弧整体转圈。
  const filled = value == null ? c * 0.28 : c * Math.min(1, Math.max(0, value));
  const arc = value == null
    ? { strokeDasharray: `${filled} ${c}` }
    : { strokeDasharray: `${c} ${c}`, strokeDashoffset: c - filled, style: { "--isl-c": `${c}` } as React.CSSProperties };
  return (
    <span className={`isl-ring${value == null ? " is-spin" : ""}${className ? ` ${className}` : ""}`} style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke={track} strokeWidth={stroke} />
        <circle
          className={value == null ? undefined : "isl-ring-arc"}
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke={color}
          strokeWidth={stroke}
          strokeLinecap="round"
          {...arc}
        />
      </svg>
      {children != null && <span className="isl-ring-body">{children}</span>}
    </span>
  );
}

/// 环色：照参考图用各家品牌色（Claude 橙、Codex 紫……），跑完的会话也是一整圈亮色，不再是
/// 死气沉沉的灰。只有要人处理（琥珀）和出错（红）换成状态色，一眼能从一排图标里挑出来。
const BRAND_COLORS: Record<string, string> = {
  claude: "#d97757",
  codex: "#a970ff",
  kimi: "#4f8cff",
  gemini: "#7b8cff",
  opencode: "#c7c7cc",
};
const STATE_COLORS: Record<string, string> = {
  pending: "var(--isl-warn)",
  error: "var(--isl-err)",
};

function ringColor(item: Item, tone: string): string {
  return STATE_COLORS[tone] ?? BRAND_COLORS[item.provider] ?? "var(--isl-text-2)";
}

/// 会话进度：有待办按完成比例；在跑但没待办 = 不定进度（转圈）；其余整圈。
function progressOf(item: Item, tone: string): number | null {
  if (item.todo_total > 0) return item.todo_done / item.todo_total;
  return tone === "running" ? null : 1;
}

/// 带进度环的 agent 图标：环色 = 品牌色（要处理琥珀、出错红），环长 = 待办进度，在跑没待办时转圈。
/// 要处理的会话角上带「!」，出错带「×」。
export function AgentRing({ item, size = 32 }: { item: Item; size?: number }) {
  const tone = cardTone(item);
  const color = ringColor(item, tone);
  const track = `color-mix(in srgb, ${color} 24%, #000)`;
  const { Icon } = agentAssets(item.provider);
  const badge = tone === "pending" ? "!" : tone === "error" ? "×" : null;
  return (
    <span className="isl-agent" data-tone={tone}>
      <Ring size={size} stroke={3} value={progressOf(item, tone)} color={color} track={track}>
        <span className="isl-agent-logo" style={tintStyle(item.provider)}><Icon /></span>
      </Ring>
      {badge && <span className={`isl-agent-badge is-${tone}`}>{badge}</span>}
    </span>
  );
}

/// 额度环中间翻一次的间隔。
export const USAGE_FLIP_MS = 3_500;

/// 额度的颜色：还宽裕是绿，过六成转琥珀，过八成五转红。一眼看出离用完还有多远，
/// 不用读数字（此前一圈淡灰，用户嫌不灵动）。
export function usageColor(pct: number): string {
  if (pct >= 85) return "var(--isl-err)";
  if (pct >= 60) return "var(--isl-warn)";
  return "var(--isl-ok)";
}

export function usageTrack(pct: number): string {
  return `color-mix(in srgb, ${usageColor(pct)} 22%, #000)`;
}

/// 额度环（收起胶囊上的小环）。中间每隔几秒在「百分比」和「是哪家的额度」（agent logo）之间
/// 上下翻一次：光一个数字读不出是谁的额度，翻动本身也让它活起来。有几家额度时轮流显示：
/// A 的数字 → A 的 logo → B 的数字 → B 的 logo……没有数据时不渲染（调用方判断）。
export function UsageRing({ entries, size = 30 }: {
  entries: readonly { provider: string; pct: number }[];
  size?: number;
}) {
  // 相位：偶数 = 某家的数字，奇数 = 它的 logo；每 USAGE_FLIP_MS 前进一步，跨家循环。
  const [phase, setPhase] = useState(0);
  useEffect(() => {
    setPhase(0);
    const id = window.setInterval(() => setPhase((p) => p + 1), USAGE_FLIP_MS);
    return () => window.clearInterval(id);
  }, [entries.length]);
  const entry = entries[Math.floor(phase / 2) % Math.max(1, entries.length)] ?? entries[0];
  const showLogo = phase % 2 === 1;
  const provider = entry?.provider;
  const value = Math.round(Math.min(100, Math.max(0, entry?.pct ?? 0)));
  const { Icon } = agentAssets(provider ?? "");
  return (
    <Ring size={size} stroke={3.5} value={value / 100} color={usageColor(value)} track={usageTrack(value)} className="isl-usage">
      <span className={`isl-usage-flip${showLogo && provider ? " is-logo" : ""}`}>
        <span className="isl-usage-val">{value}</span>
        {provider && <span className="isl-usage-logo" style={tintStyle(provider)}><Icon /></span>}
      </span>
    </Ring>
  );
}

/// 每 15 秒刷新一次的「现在」，时钟和日期用。
export function useNow(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 15_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

export function Clock({ now, locale }: { now: Date; locale: string }) {
  return (
    <span className="isl-clock">
      {now.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hour12: false })}
    </span>
  );
}
