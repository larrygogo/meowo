// 名单行展开后的「最近几步」：不钻进去也能看到它刚才做了什么、卡在哪一步。
// 数据走对话窗「加载更早」的那条读法（get_chat_history 带 before）：before 给一个比文件还大的数，
// 后端就取「文件末尾之前的最后一屏」。不用首读（offset=0）那条路，两个原因（做空审查第二轮）：
// - 首读除了解析尾部一屏，还要把前面整个文件扫一遍找模式与计划标记，100MB 的会话上实测 120–180ms，
//   速览两秒一拍用不起；带 before 的这条只读尾部（发布构建实测见 transcript.rs 的计时测试）。
// - 首读会动后端按会话记的读取进度（offset / mtime），那是对话窗轮询用的；带 before 的不动。
import { useCallback, useEffect, useRef, useState } from "react";
import { getChatHistory } from "../../api";
import type { ChatItem } from "../../generated/contracts/ChatItem";
import { useVisibleInterval } from "../../hooks/useVisibleInterval";
import { remoteUi } from "../../remoteMode";
import { prefetchPending, recallHistory } from "../chat/historyCache";
import { useT } from "../../i18n";
import { toolKind, toolShortName } from "../chat/ToolIcon";

/** 摆几步。原型定的是五步：再多就不是速览了。 */
const STEPS = 5;
/** 展开期间多久刷新一次。它在跑的时候步骤在往前走，两秒一拍够跟上，又不至于每行都在抢后端。 */
const REFRESH_MS = 2000;
/** 手机端每次读都要过远程桥，放慢。 */
const REMOTE_REFRESH_MS = 5000;

type Step = { id: string; time: string; name: string; summary: string; failed: boolean };

/** 从尾部一屏里取最后几次工具调用；配上各自的结果，报错的那一步标出来。纯函数，测试直接喂数据。 */
export function lastSteps(items: readonly ChatItem[], count: number, label: (name: string) => string, clock: (iso: string | null) => string): Step[] {
  const failed = new Set<string>();
  for (const item of items) {
    if (item.type === "tool_result" && item.is_error && item.tool_use_id) failed.add(item.tool_use_id);
  }
  const steps: Step[] = [];
  for (let i = items.length - 1; i >= 0 && steps.length < count; i -= 1) {
    const item = items[i];
    if (item.type !== "tool_use") continue;
    steps.push({ id: item.id, time: clock(item.timestamp), name: label(item.name), summary: item.summary, failed: failed.has(item.id) });
  }
  return steps.reverse();
}

export function RosterGlance({ sessionId }: { sessionId: number }) {
  const t = useT();
  // 这条会话的首屏已经在缓存里（刚看过，或手机上点开时预取到了）就直接用，不再读一遍。
  const [items, setItems] = useState<readonly ChatItem[] | null>(() => (recallHistory(sessionId)?.history.items as ChatItem[] | undefined) ?? null);
  const aliveRef = useRef(true);
  const firstRef = useRef(true);
  useEffect(() => () => { aliveRef.current = false; }, []);
  const load = useCallback(() => {
    // 预取正在路上：等它，不并发再读一份同样的东西。它回来后落进缓存，下一拍从缓存取。
    if (prefetchPending(sessionId)) return;
    const cached = recallHistory(sessionId);
    if (cached && firstRef.current) {
      firstRef.current = false;
      setItems(cached.history.items as ChatItem[]);
      return;
    }
    firstRef.current = false;
    getChatHistory(sessionId, 0, undefined, Number.MAX_SAFE_INTEGER)
      .then((history) => { if (aliveRef.current) setItems((history.items ?? []) as ChatItem[]); })
      // 读不到就不摆这一块：下面还有它最后说的话和「进入现场」，不为这个报错。
      .catch(() => {});
  }, [sessionId]);
  useEffect(load, [load]);
  useVisibleInterval(load, remoteUi() ? REMOTE_REFRESH_MS : REFRESH_MS);

  if (!items) return null;
  const clock = (iso: string | null) => {
    if (!iso) return "";
    const at = new Date(iso);
    return Number.isNaN(at.getTime()) ? "" : new Intl.DateTimeFormat(t.locale, { hour: "2-digit", minute: "2-digit", hour12: false }).format(at);
  };
  const steps = lastSteps(items, STEPS, (name) => toolShortName(name, toolKind(name), t), clock);
  if (steps.length === 0) return null;
  return (
    <ol className="roster-steps" aria-label={t.roster.recentSteps}>
      {steps.map((step) => (
        <li key={step.id} className={step.failed ? "is-failed" : undefined}>
          <span className="roster-step-time">{step.time}</span>
          <span className="roster-step-name">{step.name}</span>
          <span className="roster-step-what">{step.summary}</span>
        </li>
      ))}
    </ol>
  );
}
