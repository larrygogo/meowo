// 手机键盘是在输入框聚焦之后才弹起来的：聚焦那一刻浏览器按原高度把框滚进视野，键盘一顶，
// 对话里表单的框就落到了键盘后面（用户 2026-11-24 反馈）。可视高度变完再把它挪到对话区中间：
// 底边钉着「提交」那一排（mobile.css），贴边放会被那一排盖住。底部输入框本来就在可视区底边，不管。
// 只滚对话区：scrollIntoView 会连带滚动 html/body，iOS 上正是 --app-height 那段要压住的整页上顶。
const FOOT_ROOM = 72;

export function keepFormFieldInView(): void {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !active.closest(".chat-form")) return;
  const scroller = active.closest<HTMLElement>(".chat-scroll");
  if (!scroller) return;
  requestAnimationFrame(() => {
    const box = scroller.getBoundingClientRect();
    const field = active.getBoundingClientRect();
    if (field.top >= box.top && field.bottom <= box.bottom - FOOT_ROOM) return;
    scroller.scrollTo({ top: scroller.scrollTop + field.top + field.height / 2 - (box.top + box.height / 2), behavior: "instant" });
  });
}
