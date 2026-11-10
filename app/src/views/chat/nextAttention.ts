import type { SessionTone } from "../../api";

/// 「等你做决定或已出错」的状态：待批准、提问、计划确认、屏幕上挂着交互（都归 pending），以及出错。
/// 运行中、跑完等待输入都不算（二期 #5：Alt+Shift+↓ 只跳真正卡住的会话）。
export function attentionTone(tone: SessionTone): boolean {
  return tone === "pending" || tone === "error";
}

/// 从当前会话往后找下一个需要处理的会话：按给定顺序（侧栏显示顺序）走，到尾从头循环，
/// 当前会话自己不算。当前会话不在列表里（被筛掉、刚归档）时从头找。没有就返回 null。
export function nextAttentionSession(
  order: readonly number[],
  needs: (id: number) => boolean,
  current: number,
): number | null {
  const at = order.indexOf(current);
  for (let step = 1; step <= order.length; step += 1) {
    const id = order[(at + step) % order.length];
    if (id !== current && needs(id)) return id;
  }
  return null;
}
