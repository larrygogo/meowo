import { createContext, useContext } from "react";
import type { ChatItem, SessionTone } from "../../api";
import type { ToolResultItem } from "./shared";

/// 工具调用与子任务的运行态，以及它依据的「这一回合此刻还在不在进行」。
///
/// 此前运行态是从「缺什么」反推的：没有回执 = 在跑。可没有回执的原因不止一种——回合被
/// 打断、进程被杀、CLI 根本不落回执、正卡在审批上——于是几天前的调用还在「运行中… (3d)」
/// 地计时，等审批时过程块自动收起并写成「已运行 N 条命令」（走查 A1、A4）。
///
/// 现在状态是显式的五种，由两件事实定：这次调用有没有回执（以及回执说了什么），和它所在
/// 的回合此刻的活动态。后者只有对话窗知道（会话在跑、在等人、还是停了），经
/// [`TurnLiveContext`] 注入到时间线的每一段。

/** 一段时间线所属回合此刻的活动态：在跑 / 在等人（审批、提问）/ 已不在进行。 */
export type TurnActivity = "running" | "waiting" | "idle";

/** 工具调用的运行态。`interrupted` 是终态：没跑完，也不会再有结果。 */
export type ToolRunState = "running" | "waiting" | "done" | "error" | "interrupted";

/// 当前这段时间线所属回合的活动态。没有注入（独立渲染一条工具行）按「已不在进行」算：
/// 默认说「在跑」正是原来那个错。
export const TurnLiveContext = createContext<TurnActivity>("idle");

/// 会话进程此刻是否还活着。后台子任务派出后主回合可以先停，它们的「进行中」只在进程
/// 还活着时成立。没有注入时按活着算（子任务面板的独立测试、演示页）。
export const SessionAliveContext = createContext<boolean>(true);

/** 时间线之外（任务面板）判断「还在不在跑」要的两件事实。 */
export type SessionLive = { activity: TurnActivity; alive: boolean };

/// 不知道会话状态时的缺省：按在跑算。只给「有没有任务」这类不关心状态的调用和独立测试用；
/// 界面上的调用点都传真实状态。
export const ASSUME_LIVE: SessionLive = { activity: "running", alive: true };

/** 会话状态（与侧栏状态点同一个 sessionTone）→ 时间线用的活动态。 */
export function turnActivityOf(tone: SessionTone): TurnActivity {
  if (tone === "running") return "running";
  // 待处理 = 回合停在审批、提问或终端里的阻塞界面上：回合还在进行，只是在等人。
  if (tone === "pending") return "waiting";
  return "idle";
}

export function toolRunState(result: ToolResultItem | undefined, live: TurnActivity): ToolRunState {
  if (result) return result.interrupted ? "interrupted" : result.is_error ? "error" : "done";
  if (live === "running") return "running";
  if (live === "waiting") return "waiting";
  return "interrupted";
}

export function useToolRunState(result: ToolResultItem | undefined): ToolRunState {
  return toolRunState(result, useContext(TurnLiveContext));
}

/// 当前回合从哪条之后算起。回合边界只认记录里的显式信号，不靠「后面又出现了一条用户消息」反推：
/// - 中断分隔（meta interrupted）：被打断的那一轮到此为止，之后即使会话又在跑，也是新的一轮。
/// - provider 的记录说明「这条开启了新回合」的用户消息。各家记录怎么写（2026-11-14 用本机真实会话核对）：
///   - claude、kimi：回合中途送进去的话有专门的记录形态（queued_command、origin.inTurn），后端标成
///     interjected；不带这个标记的用户消息只会写在回合开头。
///   - codex：回合中途的话不另起回合，记录形态也和回合开头那条一样，只是 turn_id 与本回合第一条相同
///     （本机 98 份 rollout 里 15 条都是）。所以带 turn_id 的用户消息，前面还有同一个 turn_id 的，
///     就不是边界。
///   - gemini、opencode：时间线里不会出现没有回执的调用（gemini 的调用连同结果一起落盘，opencode 的
///     导出器只导出已收场的调用），这条边界对它们的运行态没有影响。
/// 发给子代理的话（to_agent）不算边界。没有边界返回 -1（整段都属于当前回合）。
///
/// 此前 codex 的用户消息不带 turn_id，回合中途再发一条，它之前还在跑的调用就被显示成「已中断」
/// （做空 ③，相对 main 的回退）。
export function liveTurnStart(items: ChatItem[]): number {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.type === "meta" && item.kind === "interrupted") return index;
    if (item.type !== "user_text" || item.interjected || item.to_agent) continue;
    if (item.turn_id && continuesTurn(items, index, item.turn_id)) continue;
    return index;
  }
  return -1;
}

/// index 处的用户消息之前，紧挨着的那条用户消息是不是同一个回合的（同一个 turn_id）。
/// 中间隔着中断分隔就不必再看：外层会先遇到那条分隔。
function continuesTurn(items: ChatItem[], index: number, turnId: string): boolean {
  for (let before = index - 1; before >= 0; before -= 1) {
    const item = items[before];
    if (item.type === "meta" && item.kind === "interrupted") return false;
    if (item.type === "user_text" && !item.to_agent) return item.turn_id === turnId;
  }
  return false;
}
