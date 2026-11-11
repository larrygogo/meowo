import { useEffect, useState } from "react";

/// 审批卡 / 提问卡出现时的读屏播报（不抢焦点）。
///
/// 起因：首焦点改给 composer（防开窗直接打字误拒，focus 车道），卡片挂载时不再获焦，
/// alertdialog 不获焦读屏就不会主动念它，读屏用户不知道有请求在等。
/// 做法：卡片挂载时把标题（「Agent 请求权限」「Agent 正在等待你的回答」）发到一个常驻的
/// polite live region，焦点留在原处。常驻是因为随卡片一起挂载的 live region 读屏不播。
type Listener = (text: string) => void;
const listeners = new Set<Listener>();

export function announceCard(text: string): void {
  listeners.forEach((listener) => listener(text));
}

export function CardAnnouncer() {
  const [text, setText] = useState("");
  useEffect(() => {
    let timer = 0;
    const listener: Listener = (next) => {
      // 先清空再写：连着两张同标题的卡（例如连续两次权限请求），文本不变读屏不会重播。
      setText("");
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setText(next), 50);
    };
    listeners.add(listener);
    return () => { listeners.delete(listener); window.clearTimeout(timer); };
  }, []);
  return <div className="chat-sronly" role="status" aria-live="polite" data-testid="card-announcer">{text}</div>;
}
