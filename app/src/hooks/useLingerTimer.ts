import { useEffect, useRef, useState } from "react";

/**
 * 带按钮的提示条「读完之前不消失」（走查 S-10，2026-11-12）：会话管理页的「撤销」原先 6 秒
 * 一到就没，焦点停在按钮上也照样消失，键盘用户 Tab 过去时它已经走了。
 *
 * 规则：token 非空即起计时 ms（token 换了一条就重新计）（不短于 8 秒，设计准则「含按钮或含原因的提示至少 8 秒」）；
 * 鼠标悬停或焦点在条内时暂停，移开/移出后重新计满。返回值铺到条的容器上。
 * ms = null 表示不计时，只等用户关闭（失败原因类的提示用它）。
 */
export function useLingerTimer(token: unknown, ms: number | null, onExpire: () => void) {
  const active = token != null;
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const expireRef = useRef(onExpire);
  expireRef.current = onExpire;
  const held = hovered || focused;
  useEffect(() => {
    if (!active || held || ms == null) return;
    const timer = window.setTimeout(() => expireRef.current(), Math.max(ms, 8_000));
    return () => window.clearTimeout(timer);
  }, [active, token, held, ms]);
  // 条收起后清掉悬停/焦点记录：下一条提示不该继承上一条的「暂停」。
  useEffect(() => {
    if (active) return;
    setHovered(false);
    setFocused(false);
  }, [active]);
  return {
    onMouseEnter: () => setHovered(true),
    onMouseLeave: () => setHovered(false),
    onFocus: () => setFocused(true),
    onBlur: (event: { currentTarget: HTMLElement; relatedTarget: EventTarget | null }) => {
      if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setFocused(false);
    },
  };
}
