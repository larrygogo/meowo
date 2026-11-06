// 灵动岛的零件：眼睛、带进度环的 agent 图标、额度环、时钟。收起胶囊、提醒、展开面板共用。
// 视觉照用户给的参考图（2026-11-04）：纯黑底、白字、环形进度、状态用颜色而不是文字堆叠。
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { agentAssets, tintStyle } from "../../providers";
import { cardTone } from "../sticker/helpers";
import type { Item } from "../sticker/types";

export type IslandTone = "error" | "pending" | "running" | "waiting" | "idle";

/// 两只眼睛（Meowo 标志）。用户要求「做得有趣些」（2026-11-04），又要「更多可能性的表情」（2026-11-06）。
/// 常驻的神态跟着整体状态走：
/// - 在跑 / 闲着：白眼珠黑瞳孔，瞳孔跟着屏幕上的鼠标转，不定时眨眼（偶尔连眨两下）；在跑时旁边冒小「+」。
///   隔一阵会自己做个小动作：眨一只眼，或左右瞟一眼
/// - 要处理：睁大、轻轻弹跳，旁边一个琥珀「!」（眼白始终是白的，用户要求）
/// - 出错：红色「× ×」
/// - 跑完等输入：眯成「^ ^」
/// - 额度用到八成五以上：眉头往下耷拉，旁边挂一滴汗；用光了：眼睛转成蚊香圈
/// - 同时跑三个以上：眼皮压低、眼角往里收，一副专注的样子
/// - 闲着超过 6 分钟（或后半夜闲着）：眼皮半耷拉犯困；超过 10 分钟睡着，眼睛合成两道线，冒「z」
/// - 同时两个以上出错：眼睛挤成「> <」，抓狂地抖
/// - 待处理放了一分钟没人管：水汪汪的大眼睛往上望，可怜巴巴
/// 一闪而过的表情由 cue 给（见 useEyeCue）：有会话刚跑完冒星星眼，新开了会话吓一跳，鼠标移上岛眨一只眼，
/// 在岛上停一会儿就害羞脸红。从睡着里醒过来也吓一跳，睡着前打个哈欠。
/// 睁眼时的随机小动作还有：一只眼睁大（好奇）、在跑时眼珠往上看（想事情）。
export function Eyes({ tone, small = false, cue = null, quotaPct = null, busy = false, errors = 0 }: {
  tone: IslandTone;
  small?: boolean;
  cue?: EyeCue | null;
  /** 正在用的那家额度已用百分比；null = 没有数据。 */
  quotaPct?: number | null;
  /** 同时在跑的会话多（≥ EYES_BUSY_AT）。 */
  busy?: boolean;
  /** 出错的会话数。 */
  errors?: number;
}) {
  const mode = useEyeMode({ tone, cue, quotaPct, busy, errors });
  const fidget = useFidget(mode === "open", tone === "running");
  const winking = mode === "open" && (cue === "wink" || fidget === "wink");
  const curious = mode === "open" && fidget === "curious";
  const looking = mode === "open" || mode === "wide" || mode === "worried" || mode === "focus" || mode === "drowsy";
  const scripted = fidget === "glance" ? GLANCE_LOOK : fidget === "ponder" ? PONDER_LOOK : null;
  const cursor = useCursorLook(looking && !scripted);
  const look = scripted ?? cursor;
  const blink = useBlink(mode === "open" || mode === "worried" || mode === "focus" || mode === "drowsy");
  const clip = useId().replace(/[^a-zA-Z0-9]/g, "");
  const lids = LIDS[mode];
  // 在跑时冒的小「+」只配睁眼和专注：发愁、蚊香圈旁边再冒「+」就乱了。
  const sparks = !small && tone === "running" && (mode === "open" || mode === "focus");
  return (
    <span className={`isl-eyes${small ? " is-small" : ""}`} data-tone={tone} data-mode={mode} aria-hidden="true">
      {sparks && <span className="isl-spark is-l">+</span>}
      {!small && (mode === "wide" || mode === "plead") && <span className="isl-spark is-l is-warn">!</span>}
      <svg className="isl-eye-pair" width={small ? 20 : 26} height={small ? 13 : 16} viewBox="0 0 26 16">
        {mode === "dead" ? (
          <g fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round">
            <path d="M3 4.5l7 7M10 4.5l-7 7" />
            <path d="M16 4.5l7 7M23 4.5l-7 7" />
          </g>
        ) : mode === "frustrated" || mode === "yawn" ? (
          <g className="isl-eye-squeeze" fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round">
            <path d="M3.5 4.5 L9.5 8 L3.5 11.5" />
            <path d="M22.5 4.5 L16.5 8 L22.5 11.5" />
          </g>
        ) : mode === "plead" ? (
          <g className="isl-eye-plead">
            {[6.5, 19.5].map((cx) => (
              <g key={cx}>
                <ellipse className="isl-sclera" cx={cx} cy={8} rx={5.8} ry={7} />
                <circle className="isl-pupil" cx={cx} cy={7} r={3.7} />
                <circle className="isl-glint" cx={cx + 1.3} cy={5.4} r={1.25} />
                <circle className="isl-glint" cx={cx - 1.2} cy={8.6} r={0.6} />
              </g>
            ))}
          </g>
        ) : mode === "happy" || mode === "blush" ? (
          <g fill="none" stroke="currentColor" strokeWidth={2.6} strokeLinecap="round">
            <path d={HAPPY_ARC[0]} />
            <path d={HAPPY_ARC[1]} />
            {mode === "blush" && (
              <g className="isl-cheeks" stroke="none">
                <ellipse cx={4} cy={13.6} rx={2.8} ry={1.4} />
                <ellipse cx={22} cy={13.6} rx={2.8} ry={1.4} />
              </g>
            )}
          </g>
        ) : mode === "sleep" ? (
          <g fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round">
            <path d="M2.5 9 Q6.5 11.5 10.5 9" />
            <path d="M15.5 9 Q19.5 11.5 23.5 9" />
          </g>
        ) : mode === "star" ? (
          <g className="isl-eye-pop" fill="currentColor">
            <path d={starPath(6.5, 8)} />
            <path d={starPath(19.5, 8)} />
          </g>
        ) : mode === "dizzy" ? (
          <g fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round">
            <path className="isl-spiral" d={spiralPath(6.5, 8)} />
            <path className="isl-spiral" d={spiralPath(19.5, 8)} />
          </g>
        ) : mode === "surprised" ? (
          <g className="isl-eye-pop">
            {[6.5, 19.5].map((cx) => (
              <g key={cx}>
                <circle className="isl-sclera" cx={cx} cy={8} r={6.4} />
                <circle className="isl-pupil" cx={cx} cy={8} r={1.4} />
              </g>
            ))}
          </g>
        ) : (
          <g className={`isl-eye-open${blink ? " is-blink" : ""}`}>
            {lids && (
              <defs>
                {lids.map((points, i) => (
                  <clipPath key={i} id={`${clip}-${i}`}><polygon points={points} /></clipPath>
                ))}
              </defs>
            )}
            {[6.5, 19.5].map((cx, i) => winking && i === 1 ? (
              <path key={cx} d={HAPPY_ARC[1]} fill="none" stroke="currentColor" strokeWidth={2.6} strokeLinecap="round" />
            ) : (
              <g key={cx} clipPath={lids ? `url(#${clip}-${i})` : undefined}>
                {/* 好奇：左眼睁大一圈，像歪头打量。 */}
                <ellipse className="isl-sclera" cx={cx} cy={8} rx={mode === "wide" ? 5.6 : curious && i === 0 ? 6 : 5} ry={mode === "wide" ? 7.2 : curious && i === 0 ? 7.6 : 6.6} />
                <circle
                  className="isl-pupil"
                  cx={cx}
                  cy={8}
                  r={mode === "wide" ? 1.9 : curious && i === 0 ? 2 : 2.5}
                  style={{ transform: `translate(${look.x * 2.2}px, ${look.y * 2.6}px)` }}
                />
              </g>
            ))}
          </g>
        )}
      </svg>
      {sparks && <span className="isl-spark is-r">+</span>}
      {!small && mode === "star" && <span className="isl-spark is-r is-star">✦</span>}
      {!small && mode === "worried" && <span className="isl-sweat" />}
      {!small && mode === "sleep" && <span className="isl-zz">z</span>}
      {!small && mode === "yawn" && <span className="isl-tear" />}
      {!small && mode === "frustrated" && <span className="isl-spark is-r is-err">#</span>}
    </span>
  );
}

/**
 * 眼睛此刻的表情。灵动岛的 Eyes 和 macOS 菜单栏的眼睛（App.tsx 推给 tray_anim.rs）共用这一套，
 * 两边同一个神态。
 */
export function useEyeMode({ tone, cue = null, quotaPct = null, busy = false, errors = 0 }: {
  tone: IslandTone;
  cue?: EyeCue | null;
  quotaPct?: number | null;
  busy?: boolean;
  errors?: number;
}): EyeMode {
  const sleep = useSleepStage(tone === "idle");
  const woke = useRiseFlash(sleep !== "asleep", 900);
  const yawning = useRiseFlash(sleep === "asleep", EYES_YAWN_MS);
  const pleading = useAfter(tone === "pending", EYES_PLEAD_AFTER_MS);
  const exhausted = quotaPct != null && quotaPct >= 100;
  const worried = quotaPct != null && quotaPct >= EYES_WORRY_AT;
  // 要人处理的（出错、待处理）和额度用光压过一切；一闪而过的表情压过常驻的神态。
  return tone === "error" ? (errors >= 2 ? "frustrated" : "dead")
    : tone === "pending" ? (pleading ? "plead" : "wide")
    : exhausted ? "dizzy"
    : cue === "star" || cue === "surprised" || cue === "blush" ? cue
    : woke ? "surprised"
    : yawning ? "yawn"
    : tone === "waiting" ? "happy"
    : worried ? "worried"
    : busy && tone === "running" ? "focus"
    : sleep === "asleep" ? "sleep"
    : sleep === "drowsy" ? "drowsy"
    : "open";
}

export type EyeMode = "open" | "wide" | "happy" | "dead" | "sleep" | "drowsy" | "star" | "surprised" | "worried" | "focus" | "dizzy"
  | "frustrated" | "plead" | "blush" | "yawn";
/** 一闪而过的表情：会话刚跑完、新开了会话、鼠标移上岛、在岛上停了一会儿。 */
export type EyeCue = "star" | "surprised" | "wink" | "blush";

/// 待处理放多久没人管就开始可怜巴巴。
export const EYES_PLEAD_AFTER_MS = 60_000;
/// 鼠标在岛上停多久开始害羞。
export const EYES_BLUSH_AFTER_MS = 1_500;
/// 睡着前那个哈欠多长。
export const EYES_YAWN_MS = 1_800;

/// 额度用到多少开始发愁（与额度环变红同一档，见 usedColor）。
export const EYES_WORRY_AT = 85;
/// 同时跑几个会话算忙。
export const EYES_BUSY_AT = 3;

const HAPPY_ARC = ["M2.5 10.5 Q6.5 3.5 10.5 10.5", "M15.5 10.5 Q19.5 3.5 23.5 10.5"] as const;
const GLANCE_LOOK = { x: -1, y: 0.2 };
/** 想事情：眼珠往右上方看。 */
const PONDER_LOOK = { x: 0.7, y: -1 };

/// 眼皮：每只眼一个可见区域（多边形以上被眼皮盖住）。发愁是外眼角往下耷拉，专注是内眼角往下压，
/// 犯困是眼皮平着盖住上半截。用裁剪而不是画黑色眼皮：菜单栏弹出的提醒底是半透明的，黑块会露馅。
const LIDS: Partial<Record<EyeMode, [string, string]>> = {
  worried: ["-1,6.5 13,0.6 13,18 -1,18", "13,0.6 27,6.5 27,18 13,18"],
  focus: ["-1,1.6 13,5.2 13,18 -1,18", "13,5.2 27,1.6 27,18 13,18"],
  drowsy: ["-1,7.4 13,7.4 13,18 -1,18", "13,7.4 27,7.4 27,18 13,18"],
};

/** 五角星，外径 6.2。 */
function starPath(cx: number, cy: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 10; i += 1) {
    const r = i % 2 === 0 ? 6.2 : 2.6;
    const a = -Math.PI / 2 + (i * Math.PI) / 5;
    pts.push(`${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`);
  }
  return `M${pts.join("L")}Z`;
}

/** 蚊香圈：从中心往外绕两圈半。 */
function spiralPath(cx: number, cy: number): string {
  const pts: string[] = [];
  for (let i = 0; i <= 40; i += 1) {
    const a = (i / 40) * Math.PI * 5;
    const r = 0.4 + (i / 40) * 5.4;
    pts.push(`${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`);
  }
  return `M${pts.join("L")}`;
}

/// 岛上一闪而过的表情：盯着会话列表，有会话从「在跑」变成「跑完」就冒一下星星眼，冒出新会话就吓
/// 一跳。第一次拿到列表不算（启动时一下子出现一堆会话不是「新开的」）。poke 给鼠标移上岛眨眼用。
export const EYE_CUE_MS: Record<EyeCue, number> = { star: 2_400, surprised: 1_200, wink: 600, blush: 2_400 };
export function useEyeCue(sessions: readonly Item[]): { cue: EyeCue | null; poke: (cue: EyeCue) => void } {
  const [cue, setCue] = useState<EyeCue | null>(null);
  const timer = useRef(0);
  const prev = useRef<Map<number, string> | null>(null);
  const poke = useCallback((next: EyeCue) => {
    window.clearTimeout(timer.current);
    setCue(next);
    timer.current = window.setTimeout(() => setCue(null), EYE_CUE_MS[next]);
  }, []);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    const next = new Map<number, string>();
    for (const item of sessions) {
      if (!item.archived && item.connected) next.set(item.session.id, cardTone(item));
    }
    // 空列表不当基线、也不覆盖基线：App 的 stripSessions 初值是 []，拿它当基线会让第一次真实拉取
    // 里的会话全算「新开的」，启动就吓一跳；拉取偶尔回空时同理，覆盖掉旧基线下一次又全体吓一跳。
    // 代价是真从零个会话开出第一个时不吓，可以接受。
    if (next.size === 0) return;
    const before = prev.current;
    prev.current = next;
    if (!before) return;
    let fired: EyeCue | null = null;
    for (const [id, tone] of next) {
      const was = before.get(id);
      if (was === "running" && tone === "waiting") fired = "star";
      else if (was === undefined && !fired) fired = "surprised";
    }
    if (fired) poke(fired);
  }, [sessions, poke]);
  return { cue, poke };
}

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

/// 睁着眼时隔 18~40 秒自己做个小动作：眨一只眼、往旁边瞟一眼、一只眼睁大（好奇）；在跑时还会
/// 往上看（想事情）。做动作时眼珠不跟鼠标。
/// 只盯着鼠标转太像监控摄像头，偶尔走个神才像活的。
type Fidget = "wink" | "glance" | "curious" | "ponder";
const FIDGET_MS: Record<Fidget, number> = { wink: 500, glance: 900, curious: 1_400, ponder: 1_600 };
function useFidget(active: boolean, running: boolean): Fidget | null {
  const [fidget, setFidget] = useState<Fidget | null>(null);
  useEffect(() => {
    if (!active) return;
    let timer = 0;
    const schedule = () => {
      timer = window.setTimeout(() => {
        if (document.visibilityState !== "visible") { schedule(); return; }
        const pool: Fidget[] = running ? ["wink", "glance", "curious", "ponder", "ponder"] : ["wink", "glance", "curious"];
        const next = pool[Math.floor(Math.random() * pool.length)];
        setFidget(next);
        timer = window.setTimeout(() => { setFidget(null); schedule(); }, FIDGET_MS[next]);
      }, 18_000 + Math.random() * 22_000);
    };
    schedule();
    return () => {
      window.clearTimeout(timer);
      setFidget(null);
    };
  }, [active, running]);
  return fidget;
}

/// 闲着超过 6 分钟犯困，超过 10 分钟睡着。后半夜（0~5 点）一闲下来就犯困。一有会话（不再闲着）立刻醒。
export const EYES_DROWSY_AFTER_MS = 6 * 60_000;
export const EYES_SLEEP_AFTER_MS = 10 * 60_000;
type SleepStage = "awake" | "drowsy" | "asleep";
function useSleepStage(idle: boolean): SleepStage {
  const [stage, setStage] = useState<SleepStage>("awake");
  useEffect(() => {
    if (!idle) {
      setStage("awake");
      return;
    }
    setStage(new Date().getHours() < 5 ? "drowsy" : "awake");
    const drowsy = window.setTimeout(() => setStage("drowsy"), EYES_DROWSY_AFTER_MS);
    const asleep = window.setTimeout(() => setStage("asleep"), EYES_SLEEP_AFTER_MS);
    return () => {
      window.clearTimeout(drowsy);
      window.clearTimeout(asleep);
    };
  }, [idle]);
  return stage;
}

/// 某个条件从假变真的那一下，亮 ms 毫秒（醒来吓一跳、睡着前打哈欠）。挂载时本来就是真的不算。
function useRiseFlash(active: boolean, ms: number): boolean {
  const [flash, setFlash] = useState(false);
  const was = useRef(active);
  useEffect(() => {
    const before = was.current;
    was.current = active;
    // 条件在闪现期间变回假时，上一轮 cleanup 已经把收回计时器清了，这里不收就永远亮着
    // （刚睡着打哈欠时开新会话 → 一直打哈欠；刚醒又睡着 → 一直吓一跳）。
    if (!active) {
      setFlash(false);
      return;
    }
    if (before) return;
    setFlash(true);
    const id = window.setTimeout(() => setFlash(false), ms);
    return () => window.clearTimeout(id);
  }, [active, ms]);
  return flash;
}

/// 条件连续成立满 ms 毫秒后变真；一断就复位（待处理放久了没人管）。
function useAfter(active: boolean, ms: number): boolean {
  const [done, setDone] = useState(false);
  useEffect(() => {
    setDone(false);
    if (!active) return;
    const id = window.setTimeout(() => setDone(true), ms);
    return () => window.clearTimeout(id);
  }, [active, ms]);
  return done;
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
/// 死气沉沉的灰。要人处理（琥珀）、出错（红）、在跑（绿）换成状态色，一眼能从一排图标里挑出来。
/// 在跑用绿是用户要求（2026-11-05）：品牌色的弧看不出「正在跑」。绿用对话窗侧栏状态点同一个
/// --cc-ok-vivid，岛上和侧栏同一个会话颜色要一样（用户要求）。
const BRAND_COLORS: Record<string, string> = {
  claude: "#d97757",
  codex: "#a970ff",
  kimi: "#4f8cff",
  gemini: "#7b8cff",
  opencode: "#c7c7cc",
};
const STATE_COLORS: Record<string, string> = {
  running: "var(--cc-ok-vivid)",
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
/// 角标：要处理「!」、出错「×」、跑完在等你的亮一个琥珀小点（与对话窗侧栏的等待点同色同规则）。
export function AgentRing({ item, size = 32, tip }: { item: Item; size?: number; tip?: string }) {
  const tone = cardTone(item);
  const color = ringColor(item, tone);
  const track = `color-mix(in srgb, ${color} 22%, #000)`;
  const arc = arcOf(item, tone);
  const { Icon } = agentAssets(item.provider);
  const badge = tone === "pending" ? "!" : tone === "error" ? "×" : null;
  const waiting = tone === "waiting";
  return (
    // data-session-id：收起胶囊上点图标直接打开这个会话（App 的按下处理按它分流）。
    <span className="isl-agent" data-tone={tone} data-session-id={item.session.id} title={tip}>
      <Ring size={size} stroke={2.5} value={arc ?? 0} color={arc === undefined ? "transparent" : color} track={track}>
        <span className="isl-agent-logo" style={tintStyle(item.provider)}><Icon /></span>
      </Ring>
      {badge && <span className={`isl-agent-badge is-${tone}`}>{badge}</span>}
      {waiting && <span className="isl-agent-badge is-waiting" aria-hidden="true" />}
    </span>
  );
}

/// 额度环中间翻一次的间隔。
export const USAGE_FLIP_MS = 3_500;

/// 额度显示「已用」：环越满越接近上限。曾改成显示剩余（像电量），用户又要回已用（2026-11-05），
/// 别再改回剩余。颜色按已用：用到六成琥珀、八成五红，其余绿。
export function usedOf(usedPct: number): number {
  return Math.round(Math.min(100, Math.max(0, usedPct)));
}

export function usedColor(used: number): string {
  if (used >= 85) return "var(--isl-err)";
  if (used >= 60) return "var(--isl-warn)";
  return "var(--isl-ok)";
}

/// 额度环（收起胶囊上的小环）：中间是已用百分比，右下角是哪家的 logo（与右边会话图标的角标同一种
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
  const used = usedOf(entry?.pct ?? 0);
  const { Icon } = agentAssets(provider);
  return (
    <span className="isl-usage-wrap" data-provider={provider}>
      {/* key 随家换：换家时整块重挂，淡入动画重播。 */}
      <span key={provider} className="isl-usage-swap">
        <Ring size={size} stroke={3} value={used / 100} color={usedColor(used)} track="var(--isl-track)" className="isl-usage">
          <span className="isl-usage-val">{used}</span>
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
