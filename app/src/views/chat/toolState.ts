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

/// 当前回合从哪条之后算起：最后一条「真正开启回合」的用户消息，或最后一条中断分隔。
/// 回合中插话（interjected）和发给子代理的话（to_agent）不算边界——agent 读到插话后接着
/// 干同一回合。中断分隔算：被打断的那一轮到此为止，之后即使会话又在跑，也是新的一轮。
/// 没有边界返回 -1（整段都属于当前回合）。
export function liveTurnStart(items: ChatItem[]): number {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.type === "user_text" && !item.interjected && !item.to_agent) return index;
    if (item.type === "meta" && item.kind === "interrupted") return index;
  }
  return -1;
}
