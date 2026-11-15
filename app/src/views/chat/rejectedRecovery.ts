import type { ChatItem } from "../../api";

/// 后端给「接口拒收了上下文本身」打的标签（kimi telemetry::failure：不可重试的 400/413）。
export const REJECTED_LABEL = "请求被拒";

/// 「请求被拒」出错卡的撤回目标：会话末尾连着几轮都是请求被拒时，毒内容是最早那一轮读进来的
///（实拍：读图那一轮的下一步就 400，之后每轮的「继续」都 400）。撤到那一轮的用户消息之前，
/// 上下文就干净了，原话放回输入框。kimi 的 /undo 只在本地收回上下文，不请求接口，
/// 会话被毒时照样能用。
///
/// 只认最后一条是请求被拒的情况；最早那条用户消息此刻不可撤（不在当前段、会话在跑）时返回 null，
/// 出错卡退回「新开会话」。
export function rejectedUndoTarget(items: readonly ChatItem[], undoableIds: ReadonlySet<string> | undefined): { id: string; text: string } | null {
  if (!undoableIds) return null;
  const last = items[items.length - 1];
  if (last?.type !== "turn_error" || last.label !== REJECTED_LABEL) return null;
  let target: { id: string; text: string } | null = null;
  let rejectedSinceUser = false;
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item.type === "turn_error" && item.label === REJECTED_LABEL) rejectedSinceUser = true;
    if (item.type !== "user_text" || item.interjected || item.to_agent) continue;
    // 这一轮没有以请求被拒收场：再往前的轮次是好的，停在这里。
    if (!rejectedSinceUser) break;
    target = { id: item.id, text: item.text };
    rejectedSinceUser = false;
  }
  return target && undoableIds.has(target.id) ? target : null;
}
