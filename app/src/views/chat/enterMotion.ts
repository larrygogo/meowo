import type { ChatItem } from "../../api";

/**
 * 实时新增条目的渐显（U6，2026-11-18）：只给「这一刻刚追加到时间线末尾」的回复段落、思考、工具行、
 * 子任务、回合报错加一次约 200ms 的 blur→清晰 + 淡入（ChatGPT、豆包的新段落同类做法），
 * 历史首屏、滚动加载更早的消息、切换会话一律不动。
 *
 * 判法：按会话记住已见过的条目 id 与上一轮末尾那条的 id。
 * - 第一次见到这个会话（含切会话、Transcript 重挂载、还没有任何条目）：全部记为已见，不动。
 * - 之后新出现的 id，只有排在「上一轮末尾那条」之后才算实时追加；排在它前面的是加载更早插到头部的。
 * - 一轮里追加超过 BULK 条按整段补读（断线重连、后台久了一次补齐）处理，不动：一屏同时糊一下更像出错。
 * 用户自己发出的消息与分隔条（meta）不在此列：用户消息是本人刚打的字，不需要「出现」的过程。
 */
export const ENTER_BULK = 6;

/** 会参与渐显的条目类型；DOM 上对应元素带 data-enter-id（Message / ToolDisclosure / SubagentBlock）。 */
const ENTER_TYPES = new Set<ChatItem["type"]>(["assistant_text", "assistant_delta", "reasoning", "reasoning_delta", "turn_error", "tool_use"]);

export type EnterTrack = { sessionId: number; known: Set<string>; lastId: string | null };

export function trackEntering(prev: EnterTrack | null, sessionId: number, items: readonly ChatItem[]): { track: EnterTrack; entering: string[] } {
  const lastId = items.length > 0 ? items[items.length - 1].id : null;
  if (!prev || prev.sessionId !== sessionId || prev.known.size === 0) {
    return { track: { sessionId, known: new Set(items.map((item) => item.id)), lastId }, entering: [] };
  }
  const known = prev.known;
  // 上一轮末尾那条现在排在哪：它之后的新 id 才是追加。找不到（整段被替换）就不认任何追加。
  const anchor = prev.lastId === null ? -1 : items.findIndex((item) => item.id === prev.lastId);
  const appended: string[] = [];
  items.forEach((item, index) => {
    if (known.has(item.id)) return;
    known.add(item.id);
    if (anchor >= 0 && index > anchor && ENTER_TYPES.has(item.type)) appended.push(item.id);
  });
  return { track: { sessionId, known, lastId }, entering: appended.length > ENTER_BULK ? [] : appended };
}

export const ENTER_CLASS = "is-entering";

const escapeId = (id: string) => (typeof CSS !== "undefined" && CSS.escape ? CSS.escape(id) : id.replace(/["\\]/g, "\\$&"));

/** 给刚追加的条目挂上渐显类，动画播完摘掉（不留着：折叠块收起再展开时重挂载，不该再播一遍）。
 *  动画本身、时长与减少动效的关闭都在 styles.css 的 .is-entering。 */
export function markEntering(root: ParentNode, ids: readonly string[]): void {
  for (const id of ids) {
    const el = root.querySelector<HTMLElement>(`[data-enter-id="${escapeId(id)}"]`);
    if (!el) continue; // 落在收起的折叠块里，看不见就不播
    el.classList.add(ENTER_CLASS);
    const done = (event: Event) => {
      // 子元素自己的动画（转圈、呼吸点）也会冒泡上来，只认这条元素自己的渐显。
      if (event.target !== el) return;
      el.classList.remove(ENTER_CLASS);
      el.removeEventListener("animationend", done);
      el.removeEventListener("animationcancel", done);
    };
    el.addEventListener("animationend", done);
    el.addEventListener("animationcancel", done);
  }
}
