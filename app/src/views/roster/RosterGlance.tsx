// 名单行展开后的「最近几步」：不钻进去也能看到它刚才做了什么、卡在哪一步。
// 数据走对话窗首读的同一个接口（get_chat_history，offset=0）：后端只解析文件尾部够一屏的那段，
// 不整文件读，长会话也是同一个量级。
import { useCallback, useEffect, useRef, useState } from "react";
import { getChatHistory } from "../../api";
import type { ChatItem } from "../../generated/contracts/ChatItem";
import { useVisibleInterval } from "../../hooks/useVisibleInterval";
import { useT } from "../../i18n";
import { toolKind, toolShortName } from "../chat/ToolIcon";

/** 摆几步。原型定的是五步：再多就不是速览了。 */
const STEPS = 5;
/** 展开期间多久刷新一次。它在跑的时候步骤在往前走，两秒一拍够跟上，又不至于每行都在抢后端。 */
const REFRESH_MS = 2000;

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
  const [items, setItems] = useState<readonly ChatItem[] | null>(null);
  const aliveRef = useRef(true);
  useEffect(() => () => { aliveRef.current = false; }, []);
  const load = useCallback(() => {
    getChatHistory(sessionId, 0)
      .then((history) => { if (aliveRef.current) setItems((history.items ?? []) as ChatItem[]); })
      // 读不到就不摆这一块：下面还有它最后说的话和「进入现场」，不为这个报错。
      .catch(() => {});
  }, [sessionId]);
  useEffect(load, [load]);
  useVisibleInterval(load, REFRESH_MS);

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
