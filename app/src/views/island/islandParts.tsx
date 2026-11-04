// 灵动岛的零件：眼睛、带进度环的 agent 图标、额度环、时钟。收起胶囊、提醒、展开面板共用。
// 视觉照用户给的参考图（2026-11-04）：纯黑底、白字、环形进度、状态用颜色而不是文字堆叠。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { agentAssets, tintStyle } from "../../providers";
import { cardTone } from "../sticker/helpers";
import type { Item } from "../sticker/types";

export type IslandTone = "error" | "pending" | "running" | "waiting" | "idle";

/// 两只眼睛（Meowo 标志）。用户要求「做得有趣些」（2026-11-04）：
/// - 在跑 / 闲着：白眼珠黑瞳孔，瞳孔跟着屏幕上的鼠标转，不定时眨眼（偶尔连眨两下）；在跑时旁边冒小「+」
/// - 要处理：睁大、轻轻弹跳，旁边一个琥珀「!」（眼白始终是白的，用户要求）
/// - 出错：红色「× ×」
/// - 跑完等输入：眯成「^ ^」
/// - 闲着超过 10 分钟：睡着，眼睛合成两道线，冒「z」
export function Eyes({ tone, small = false }: { tone: IslandTone; small?: boolean }) {
  const sleepy = useSleepy(tone === "idle");
  const mode: EyeMode = tone === "error" ? "dead"
    : tone === "waiting" ? "happy"
    : sleepy ? "sleep"
    : tone === "pending" ? "wide"
    : "open";
  const looking = mode === "open" || mode === "wide";
  const look = useCursorLook(looking);
  const blink = useBlink(mode === "open");
  return (
    <span className={`isl-eyes${small ? " is-small" : ""}`} data-tone={tone} data-mode={mode} aria-hidden="true">
      {!small && tone === "running" && <span className="isl-spark is-l">+</span>}
      {!small && mode === "wide" && <span className="isl-spark is-l is-warn">!</span>}
      <svg className="isl-eye-pair" width={small ? 20 : 26} height={small ? 13 : 16} viewBox="0 0 26 16">
        {mode === "dead" ? (
          <g fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round">
            <path d="M3 4.5l7 7M10 4.5l-7 7" />
            <path d="M16 4.5l7 7M23 4.5l-7 7" />
          </g>
        ) : mode === "happy" ? (
          <g fill="none" stroke="currentColor" strokeWidth={2.6} strokeLinecap="round">
            <path d="M2.5 10.5 Q6.5 3.5 10.5 10.5" />
            <path d="M15.5 10.5 Q19.5 3.5 23.5 10.5" />
          </g>
        ) : mode === "sleep" ? (
          <g fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round">
            <path d="M2.5 9 Q6.5 11.5 10.5 9" />
            <path d="M15.5 9 Q19.5 11.5 23.5 9" />
          </g>
        ) : (
          <g className={`isl-eye-open${blink ? " is-blink" : ""}`}>
            {[6.5, 19.5].map((cx) => (
              <g key={cx}>
                <ellipse className="isl-sclera" cx={cx} cy={8} rx={mode === "wide" ? 5.6 : 5} ry={mode === "wide" ? 7.2 : 6.6} />
                <circle
                  className="isl-pupil"
                  cx={cx}
                  cy={8}
                  r={mode === "wide" ? 1.9 : 2.5}
                  style={{ transform: `translate(${look.x * 2.2}px, ${look.y * 2.6}px)` }}
                />
              </g>
            ))}
          </g>
        )}
      </svg>
      {!small && tone === "running" && <span className="isl-spark is-r">+</span>}
      {!small && mode === "sleep" && <span className="isl-zz">z</span>}
    </span>
  );
}

type EyeMode = "open" | "wide" | "happy" | "dead" | "sleep";

/// 鼠标相对岛中心的方向，归一到 -1..1。只在需要「看」的时候轮询（110ms 一次，瞳孔有 CSS 过渡，
/// 看着是连续转的）。页面不可见时暂停。island_cursor 只读光标与窗口位置，很便宜。
function useCursorLook(active: boolean): { x: number; y: number } {
  const [look, setLook] = useState({ x: 0, y: 0 });
  useEffect(() => {
    if (!active) {
      setLook({ x: 0, y: 0 });
      return;
    }
    let cancelled = false;
    const tick = () => {
      if (document.visibilityState !== "visible") return;
      invoke<[number, number] | null>("island_cursor")
        .then((pos) => {
          if (cancelled || !pos) return;
          const [dx, dy] = pos;
          // 横向在 150px 内由中间转到最边，纵向 60px：鼠标在屏幕下方远处时眼睛朝下看到底。
          const x = Math.max(-1, Math.min(1, dx / 150));
          const y = Math.max(-1, Math.min(1, dy / 60));
          setLook((prev) => (Math.abs(prev.x - x) < 0.05 && Math.abs(prev.y - y) < 0.05 ? prev : { x, y }));
        })
        .catch(() => {});
    };
    tick();
    const id = window.setInterval(tick, 110);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [active]);
  return look;
}

/// 不定时眨眼：2.5~6 秒一次，两成概率连眨两下。固定节奏的眨眼看两遍就假了。
function useBlink(active: boolean): boolean {
  const [blink, setBlink] = useState(false);
  useEffect(() => {
    if (!active) return;
    let timer = 0;
    const schedule = () => {
      timer = window.setTimeout(() => {
        const twice = Math.random() < 0.2;
        setBlink(true);
        timer = window.setTimeout(() => {
          setBlink(false);
          if (twice) {
            timer = window.setTimeout(() => {
              setBlink(true);
              timer = window.setTimeout(() => { setBlink(false); schedule(); }, 130);
            }, 160);
          } else {
            schedule();
          }
        }, 130);
      }, 2_500 + Math.random() * 3_500);
    };
    schedule();
    return () => {
      window.clearTimeout(timer);
      setBlink(false);
    };
  }, [active]);
  return blink;
}

/// 闲着超过 10 分钟就「睡着」。一有会话（不再闲着）立刻醒。
export const EYES_SLEEP_AFTER_MS = 10 * 60_000;
function useSleepy(idle: boolean): boolean {
  const [sleepy, setSleepy] = useState(false);
  useEffect(() => {
    setSleepy(false);
    if (!idle) return;
    const id = window.setTimeout(() => setSleepy(true), EYES_SLEEP_AFTER_MS);
    return () => window.clearTimeout(id);
  }, [idle]);
  return sleepy;
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

/// 亮弧只在有含义时出现（用户嫌整圈亮色环又重又看不出状态）：
/// 要处理 / 出错整圈状态色；在跑有待办按完成比例、没待办转一小段；跑完不画亮弧（只剩淡淡的底轨）。
/// 返回 undefined = 不画亮弧。
function arcOf(item: Item, tone: string): number | null | undefined {
  if (tone === "pending" || tone === "error") return 1;
  if (tone === "running") return item.todo_total > 0 ? item.todo_done / item.todo_total : null;
  return undefined;
}

/// 会话图标：深灰圆底座里放 agent logo，外面一圈很淡的品牌色底轨，亮弧按 arcOf 画。
/// 角标：要处理「!」、出错「×」、跑完但你还没看过的亮一个小绿点。
export function AgentRing({ item, size = 32 }: { item: Item; size?: number }) {
  const tone = cardTone(item);
  const color = ringColor(item, tone);
  const track = `color-mix(in srgb, ${color} 22%, #000)`;
  const arc = arcOf(item, tone);
  const { Icon } = agentAssets(item.provider);
  const badge = tone === "pending" ? "!" : tone === "error" ? "×" : null;
  const unread = tone === "waiting" && item.unread === true;
  return (
    <span className="isl-agent" data-tone={tone}>
      <Ring size={size} stroke={2.5} value={arc ?? 0} color={arc === undefined ? "transparent" : color} track={track}>
        <span className="isl-agent-logo" style={tintStyle(item.provider)}><Icon /></span>
      </Ring>
      {badge && <span className={`isl-agent-badge is-${tone}`}>{badge}</span>}
      {unread && <span className="isl-agent-badge is-unread" aria-hidden="true" />}
    </span>
  );
}

/// 额度环中间翻一次的间隔。
export const USAGE_FLIP_MS = 3_500;

/// 额度一律显示「剩余」，像电量：环满 = 还宽裕。此前显示已用，用了 4% 时环几乎是空的，
/// 一眼看去像「快没了」（用户嫌怪）。颜色也按剩余：宽裕绿、不到四成琥珀、不到一成五红。
export function remainingOf(usedPct: number): number {
  return Math.round(Math.min(100, Math.max(0, 100 - usedPct)));
}

export function remainingColor(remaining: number): string {
  if (remaining <= 15) return "var(--isl-err)";
  if (remaining <= 40) return "var(--isl-warn)";
  return "var(--isl-ok)";
}

/// 额度环（收起胶囊上的小环）：中间是剩余百分比，右下角是哪家的 logo（与右边会话图标的角标同一种
/// 做法）。几家都有额度时每隔几秒淡入淡出换下一家。尺寸、粗细与会话图标的环一致，底轨中性灰。
/// 没有数据时不渲染（调用方判断）。
export function UsageRing({ entries, size = 32 }: {
  entries: readonly { provider: string; pct: number }[];
  size?: number;
}) {
  const [index, setIndex] = useState(0);
  useEffect(() => {
    setIndex(0);
    if (entries.length < 2) return;
    const id = window.setInterval(() => setIndex((i) => i + 1), USAGE_FLIP_MS);
    return () => window.clearInterval(id);
  }, [entries.length]);
  const entry = entries[index % Math.max(1, entries.length)] ?? entries[0];
  const provider = entry?.provider ?? "";
  const remaining = remainingOf(entry?.pct ?? 0);
  const { Icon } = agentAssets(provider);
  return (
    <span className="isl-usage-wrap" data-provider={provider}>
      {/* key 随家换：换家时整块重挂，淡入动画重播。 */}
      <span key={provider} className="isl-usage-swap">
        <Ring size={size} stroke={3} value={remaining / 100} color={remainingColor(remaining)} track="var(--isl-track)" className="isl-usage">
          <span className="isl-usage-val">{remaining}</span>
        </Ring>
        {provider && <span className="isl-usage-badge" style={tintStyle(provider)}><Icon /></span>}
      </span>
    </span>
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
