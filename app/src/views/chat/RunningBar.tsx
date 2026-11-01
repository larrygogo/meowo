/// 对话末尾的运行行（对照 Kimi 的 WorkingIndicator）与它的数据：CLI 状态行/重试行的
/// 小仓库、最近命令的展示清洗、贪吃蛇 spinner。从 ChatWindow 搬出：这是一块独立的展示
/// 领域，状态行每秒刷新，放在五千行组件里只会让人误以为它和会话状态纠缠在一起。

import { useEffect, useMemo, useState } from "react";
import { COMPACTING_ACTIVITY } from "../../activity";
import { useT } from "../../i18n";
import { fmtElapsed, type CliRetry, type CliStatus } from "./cliStatus";

// 运行条活动文本的展示清洗：剥掉「cd <目录> && 」前缀（保留可能的 ›/> 提示符）。
// 状态条截断成单行后，冗长的项目路径会把真正在跑的命令挤出可视区；完整原文在 title 里。
const trimActivityCdPrefix = (activity: string): string =>
  activity.replace(/^([›>]\s*)?cd\s+("[^"]*"|'[^']*'|[^\s&]+)\s+&&\s*/, "$1");

// current_activity 还可能是压缩进行中的哨兵 COMPACTING_ACTIVITY（见 ../activity）：状态
// 记号不是活动文本——映射成本地化「正在压缩上下文…」，原值不上屏、不进 tooltip，也不过
// trimActivityCdPrefix（哨兵不是命令，没有 cd 前缀可剥）。
export function runningActivityView(
  activity: string | null | undefined,
  compactingLabel: string,
  runningLabel: string,
): { text: string; tip: string | undefined } {
  if (activity === COMPACTING_ACTIVITY) return { text: compactingLabel, tip: undefined };
  if (!activity) return { text: runningLabel, tip: undefined };
  return { text: trimActivityCdPrefix(activity), tip: activity };
}

// 运行指示的「贪吃蛇」（用户指定形态，kimi TUI 同款）：2×4 盲文格内 3 个亮点沿
// 顺时针环路爬行（左列往上、右列往下），8 帧一轮。比原地呼吸的圆点更「在前进」。
// 帧字符按盲文点位预生成（dot1..8 = bit0..7，左列 1,2,3,7 / 右列 4,5,6,8）。
const SNAKE_PATH = [1, 4, 5, 6, 8, 7, 3, 2]; // 顺时针环路（盲文点编号）
const SNAKE_FRAMES = SNAKE_PATH.map((_, i) =>
  String.fromCharCode(0x2800 + [0, 1, 2].reduce((m, k) => m | (1 << (SNAKE_PATH[(i + k) % 8] - 1)), 0)),
);
export function SnakeSpinner() {
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    // reduced-motion：不启动循环，定格首帧（仍有「蛇」形，不晃）。
    if (typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = window.setInterval(() => setFrame((f) => (f + 1) % SNAKE_FRAMES.length), 120);
    return () => window.clearInterval(timer);
  }, []);
  return <i className="chat-running-snake" aria-hidden="true">{SNAKE_FRAMES[frame]}</i>;
}

/// 屏幕上解析到的 CLI 状态行，带解析时刻。放在 ref 式小仓库里而不是 ChatWindow 的
/// state：状态行每秒刷新（计时在跳），进 state 会让这个五千行的组件每秒整体重渲染。
type CliStatusSnap = CliStatus & { at: number };
export function useCliStatusStore() {
  return useMemo(() => {
    let snap: CliStatusSnap | null = null;
    // 自动重试行单独一路：重试期间 spinner 状态行未必还在，两者各自保鲜。
    let retry: (CliRetry & { at: number }) | null = null;
    const listeners = new Set<() => void>();
    return {
      get: () => snap,
      set: (next: CliStatus | null) => {
        snap = next && { ...next, at: Date.now() };
        listeners.forEach((listener) => listener());
      },
      getRetry: () => retry,
      setRetry: (next: CliRetry | null) => {
        retry = next && { ...next, at: Date.now() };
        listeners.forEach((listener) => listener());
      },
      subscribe: (listener: () => void) => {
        listeners.add(listener);
        return () => { listeners.delete(listener); };
      },
    };
  }, []);
}
export type CliStatusStore = ReturnType<typeof useCliStatusStore>;
/// 屏幕解析结果的保鲜期：运行中状态行每秒重画，超过这个时长没再认到 = 屏幕上已经
/// 没有它（回合刚结束/刚换回合），不能拿上一轮的旧耗时冒充当前。
const CLI_STATUS_FRESH_MS = 3000;

/// 运行条（2026-10 用户要求「内容改成终端这样的」）：首行照搬 CLI 状态行的动词与统计
/// （`Composing… (41s · ↓ 427 tokens · thought for 4s)`），读不到屏幕（外部终端会话/
/// 非 claude）时退回「正在运行… (本地计时)」；次行是 hook 报的最近命令，`⎿` 起头，
/// 对应终端里状态行下方那一行。spinner 仍是贪吃蛇（用户指定形态，只改内容不改字符）。
export function RunningBar({ store, turnStartedAt, override, fallbackVerb, activity }: {
  store: CliStatusStore;
  turnStartedAt: number | null;
  /** 压过 CLI 状态行的说法（插话去向、压缩中）：用户此刻最关心的那件事。 */
  override: string | null;
  fallbackVerb: string;
  activity: { text: string; tip: string | undefined } | null;
}) {
  const t = useT();
  const [, setTick] = useState(0);
  useEffect(() => {
    const bump = () => setTick((value) => value + 1);
    const unsubscribe = store.subscribe(bump);
    // 本地计时与保鲜判定都要按秒推进，即便屏幕没有新输出。
    const timer = window.setInterval(bump, 1000);
    return () => { unsubscribe(); window.clearInterval(timer); };
  }, [store]);
  const now = Date.now();
  const snap = store.get();
  const fresh = snap && now - snap.at < CLI_STATUS_FRESH_MS ? snap : null;
  const retrySnap = store.getRetry();
  const retry = retrySnap && now - retrySnap.at < CLI_STATUS_FRESH_MS ? retrySnap : null;
  // 自动重试压过一切说法（对照 Kimi 的 turnRetry 优先）：这是此刻「为什么没动静」的真正答案。
  const verb = retry ? t.chat.workingRetry(retry.attempt, retry.max) : override ?? fresh?.verb ?? fallbackVerb;
  const detail = fresh ? fresh.detail : turnStartedAt !== null ? fmtElapsed(now - turnStartedAt) : null;
  return (
    <div className="chat-running" role="status" aria-live="polite">
      <div className="chat-running-line">
        <SnakeSpinner />
        <span className="chat-running-verb">{verb}</span>
        {/* 统计每秒都在变：不进读屏播报，否则活区每秒念一遍。 */}
        {detail && <span className="chat-running-detail" aria-hidden="true">({detail})</span>}
      </div>
      {activity && (
        <div className="chat-running-sub">
          <span className="chat-running-elbow" aria-hidden="true">⎿</span>
          {/* 长命令截断成单行，全文进 data-tip（见 runningActivityView）。 */}
          <span className="chat-running-activity" data-tip={activity.tip}>{activity.text}</span>
        </div>
      )}
    </div>
  );
}
