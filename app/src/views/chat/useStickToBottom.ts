/// 对话滚动区的「跟着最新消息」：吸底判定、各种内容变化时钉回底部、触摸/内层滚动让路、
/// 切去终端再回来的阅读位置恢复、「回到最新」悬浮钮与它的未读数。
///
/// 为什么单独成 hook（对照 Kimi 的 ChatPane 把这件事收在一个 composable 里，返回
/// following / showJump / onScroll / pinScroll / jumpToBottom）：此前这些规则以十来处 effect
/// 和 ref 散在 ChatWindow 各段，改一处常漏另一处——手机上的滚动抽搐就是「内容自己长高」
/// 这条路没人管。规则全在这里；ChatWindow 只在自己也要动滚动位置的地方（加载更早的保位、
/// 审批卡的底部留白）读写返回的 ref。

import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import type { ChatItem } from "../../api";

/** 吸底判定：距底 ≤64px 算「跟着最新消息」（取自 Kimi 桌面版 ChatPane 的同名阈值）。 */
export const FOLLOW_PX = 64;
/** 前插「更早历史」落位后的重算用这个宽窗：补 scrollHeight 差有一两像素的取整误差，
 *  按 FOLLOW_PX 判会把刚落好位的人误判成脱离底部，之后新消息再也不自动跟随。 */
export const ANCHOR_PX = 160;

/** 这一层自己有没有纵向滚动条（可滚 = overflow-y 允许 + 确有内容余量）。
 *  Kimi 的嵌套滚动判据同款：scrollHeight<=clientHeight+1 的盒子根本不接管滚动。 */
function scrollsOwnY(node: Element): boolean {
  if (node.scrollHeight <= node.clientHeight + 1) return false;
  return /(auto|scroll)/.test(getComputedStyle(node).overflowY);
}

/** 滚轮这一下该不该由内层容器消化：从落点往上找第一个能滚的盒子，它在自己方向的
 *  边缘之前都归它管（长日志、diff 表读到一半被主区拽走，是最常见的一类打断）。 */
function innerScrollOwns(root: HTMLElement, target: Element, deltaY: number): boolean {
  for (let node: Element | null = target; node && node !== root; node = node.parentElement) {
    if (!scrollsOwnY(node)) continue;
    const max = node.scrollHeight - node.clientHeight;
    return deltaY > 0 ? node.scrollTop < max - 1 : node.scrollTop > 0;
  }
  return false;
}

// 程序化吸底一律瞬移（临时关掉 smooth）：`.chat-scroll` 的平滑滚动是动画，动画途中
// 浏览器持续派发 scroll 事件——一条长消息到达时，从旧位置滚向新底部的中途距离远大于
// onScroll 的 FOLLOW_PX（64px）阈值，followRef 被误置 false，吸底从此断掉（此坑首帧定位早就堵过，
// 但只堵了首帧；后续每一批 delta 的吸底都在裸奔）。smooth 留给用户主动的滚动。
export function stickToBottom(el: HTMLElement) {
  const behavior = el.style.scrollBehavior;
  el.style.scrollBehavior = "auto";
  el.scrollTop = el.scrollHeight;
  el.style.scrollBehavior = behavior;
}

/// 「回到最新」未读数的计数口径：只数人能认出来的一条消息——用户发言、agent 正文、回合报错。
/// 工具调用/结果、思考块在屏幕上被折成一行「已运行 N 条命令」，按条数算会让徽章一跳几十，
/// 和眼前看到的对不上（用户反馈数字不准）；流式 delta 是同一条正文的碎片，也不算。
export function countsAsUnseen(item: ChatItem): boolean {
  return item.type === "user_text" || item.type === "assistant_text" || item.type === "turn_error";
}

export function useStickToBottom({ scrollRef, visible, timelineItems, contentCount }: {
  scrollRef: RefObject<HTMLElement | null>;
  /** 对话视图在显示（终端视图时滚动区是 display:none）。 */
  visible: boolean;
  timelineItems: readonly ChatItem[];
  /** 时间线里按 countsAsUnseen 计的消息数，未读数的口径。 */
  contentCount: number;
}) {
  const followRef = useRef(true);
  // 手指按在滚动区上（含松手后的惯性滑动）期间为 true：自动吸底一律让路。人在滚的时候
  // 程序再写 scrollTop，就是跟手指抢位置——手机上表现为滚动条一下下抽（用户实拍）。
  const touchingRef = useRef(false);
  // 是否贴在底部（followRef 的渲染镜像）：驱动「回到最新」悬浮钮显隐。
  const [atBottom, setAtBottom] = useState(true);
  // 悬浮钮未读徽章的计数快照：脱离底部那一刻的时间线长度（null = 正贴底/无须计数）。
  const awayCountRef = useRef<number | null>(null);
  // 7C-7：切去终端再切回来，阅读位置此前一律丢失——真正的原因是 hidden 的 display:none
  // 让浏览器把 scrollTop 清零。容器还是同一个，离开前把位置和 follow 态记下来，回来原样
  // 放回去；本来就吸着底的照旧吸底。
  const savedScrollRef = useRef<{ top: number; follow: boolean } | null>(null);
  // 最后一次「可见状态下」的滚动位置，由 onScroll 持续写入（见那里的理由）。
  const lastScrollTopRef = useRef(0);

  // compose 进入文档流后不再浮在滚动区上：textarea 变高、审批卡出现/消失都会改变
  // .chat-scroll 的视口高度。用户停在底部时 scrollTop 不变、视口变矮，尾条消息会
  // 视觉上移——这里在滚动容器尺寸变化时把已吸底的用户重新钉回底部（反向变矮时
  // 浏览器会自动钳住 scrollTop，无需处理）；用户已上翻则不打扰。
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      // hidden（终端视图）时 display:none，scrollHeight 恒 0，不能拿它改写滚动位置。
      if (el.hidden || !followRef.current || touchingRef.current) return;
      el.scrollTop = el.scrollHeight;
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [scrollRef]);

  // 内容自己长高（折叠块开合、代码高亮、图片加载、运行条换行）不经过 items 变化，
  // 下方 [timelineItems] 的吸底接不到：要等下一次轮询才一把拽到底，看上去就是抽搐。
  // 监听滚动区子树变化与图片 load，在下一帧绘制前钉回底部（rAF 合并同一帧的多次变化）。
  // 触摸期间不动；松手后等惯性滑动停稳（一段时间没有 scroll）再按真实位置重算是否吸底。
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    let frame = 0;
    let release = 0;
    const pin = () => {
      frame = 0;
      if (el.hidden || !followRef.current || touchingRef.current) return;
      stickToBottom(el);
    };
    // 顶层条目自己的尺寸变化也要接：条目是 content-visibility:auto，屏外先按 60px 占位，
    // 滚到底后底部几条才真正排版、长回实高——这不改 DOM、也不改滚动区自身尺寸，上面两个
    // 观察者都收不到，进会话就停在半截（用户实拍「每次打开不在最新」）。对照 Kimi 的
    // useStickToBottom：ResizeObserver 盯内容本身，跟随中长高就钉回去。回调在布局后、
    // 绘制前执行，直接钉，不等下一帧，免得先闪一帧半截。
    const sizes = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      if (el.hidden || !followRef.current || touchingRef.current) return;
      stickToBottom(el);
    });
    const observed = new Set<Element>();
    const syncObserved = () => {
      if (!sizes) return;
      const now = new Set(Array.from(el.children));
      for (const child of observed) if (!now.has(child)) { sizes.unobserve(child); observed.delete(child); }
      for (const child of now) if (!observed.has(child)) { sizes.observe(child); observed.add(child); }
    };
    syncObserved();
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(() => { syncObserved(); pin(); });
    };
    const mutations = typeof MutationObserver === "undefined" ? null : new MutationObserver(schedule);
    mutations?.observe(el, { childList: true, subtree: true, characterData: true });
    const settle = () => {
      window.clearTimeout(release);
      release = window.setTimeout(() => {
        touchingRef.current = false;
        followRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= FOLLOW_PX;
        setAtBottom(followRef.current);
      }, 180);
    };
    const onTouchStart = () => { window.clearTimeout(release); touchingRef.current = true; };
    const onScrollWhileTouching = () => { if (touchingRef.current && release) settle(); };
    const onTouchEnd = () => settle();
    el.addEventListener("load", schedule, true);
    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("touchend", onTouchEnd, { passive: true });
    el.addEventListener("touchcancel", onTouchEnd, { passive: true });
    el.addEventListener("scroll", onScrollWhileTouching, { passive: true });
    return () => {
      mutations?.disconnect();
      sizes?.disconnect();
      cancelAnimationFrame(frame);
      window.clearTimeout(release);
      el.removeEventListener("load", schedule, true);
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchend", onTouchEnd);
      el.removeEventListener("touchcancel", onTouchEnd);
      el.removeEventListener("scroll", onScrollWhileTouching);
    };
  }, [scrollRef]);

  // 新消息到达 / 视图切换：离开对话视图时记下位置，回来时放回；平时吸着底就钉回底部。
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!visible) {
      // 这里已经是 hidden 之后，el.scrollTop 恒 0——取 onScroll 记下的那一份。
      savedScrollRef.current = { top: lastScrollTopRef.current, follow: followRef.current };
      return;
    }
    if (!el) return;
    const saved = savedScrollRef.current;
    if (saved) {
      savedScrollRef.current = null;
      followRef.current = saved.follow;
      setAtBottom(saved.follow);
      if (!saved.follow) {
        const behavior = el.style.scrollBehavior;
        el.style.scrollBehavior = "auto";
        el.scrollTop = saved.top;
        el.style.scrollBehavior = behavior;
        lastScrollTopRef.current = saved.top;
        return;
      }
    }
    if (!followRef.current || touchingRef.current) return;
    if (timelineItems.length === 0) return;
    stickToBottom(el);
  }, [timelineItems, visible, scrollRef]);

  // 滚轮落在能自己滚的子容器上（长日志的 pre、diff 表）时关掉自动跟随：这一发是子容器在
  // 读它自己的内容，下一批 delta 到达时的吸底会把人正在看的那几行拽走。只改 followRef，
  // atBottom 交给下一次真实的主区滚动去同步——子容器里滚不需要「回到最新」这颗钮冒出来。
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onWheelCapture = (event: WheelEvent) => {
      const target = event.target;
      if (!(target instanceof Element) || target === el) return;
      if (innerScrollOwns(el, target, event.deltaY)) followRef.current = false;
    };
    el.addEventListener("wheel", onWheelCapture, { passive: true });
    return () => el.removeEventListener("wheel", onWheelCapture);
  }, [scrollRef]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const at = el.scrollHeight - el.scrollTop - el.clientHeight <= FOLLOW_PX;
    followRef.current = at;
    // 7C-7：切去终端页时 main 变 display:none，scrollTop 被浏览器清零，等 layout effect
    // 跑到时已经量不到了。唯一可靠的时机是「还看得见的时候」——每次滚动都记一份，切回时直接用。
    lastScrollTopRef.current = el.scrollTop;
    // 脱离底部的瞬间记下时间线内容项数快照，贴回底部即清：悬浮钮的未读徽章由差值派生。
    if (at) awayCountRef.current = null;
    else if (awayCountRef.current === null) awayCountRef.current = contentCount;
    // 驱动「回到最新」悬浮钮的显隐；同值短路避免滚动过程反复重渲染。
    setAtBottom((prev) => (prev === at ? prev : at));
  };
  // 上翻后回到底部的唯一出口（此前只能手动滚回去，新消息静默追加在视口外）。
  const jumpToLatest = () => {
    const el = scrollRef.current;
    if (!el) return;
    followRef.current = true;
    awayCountRef.current = null; // 跳底 = 已读，未读计数清零
    setAtBottom(true);
    stickToBottom(el);
  };
  /// 换会话：旧会话的阅读位置、未读快照对新 transcript 无意义（7C-7 的恢复只跨视图，不跨会话）。
  const resetForSession = () => {
    awayCountRef.current = null;
    followRef.current = true;
    savedScrollRef.current = null;
    lastScrollTopRef.current = 0;
  };
  // 「回到最新」悬浮钮的未读数：脱离底部时的消息数为快照（口径见 countsAsUnseen），
  // 之后追加的算未读（前插的「加载更早」与接续段旧历史不算——调用方同步抬高 awayCountRef）。
  const unseenCount = !atBottom && awayCountRef.current !== null
    ? Math.max(0, contentCount - awayCountRef.current)
    : 0;

  return { followRef, touchingRef, awayCountRef, atBottom, setAtBottom, onScroll, jumpToLatest, resetForSession, unseenCount };
}
