/**
 * 发送相关的文案由发送结果决定（验收 2）。
 *
 * 两类此前各写各的、与实际对不上的文案集中到这里：
 * 1. 回合在跑时发消息会怎样：输入框占位（发之前的许诺）与排队栈回执（发之后的说法）必须是
 *    同一个去向。去向只看后端声明的能力（queue_input、queue_delivery），不按 Provider 名字。
 *    走查输入 B5：Kimi 没声明送达时机，回执说「已交给 Agent」，占位却许诺「排队」。
 * 2. 消息被拦下没发时的提示：不能说「消息已发出」（走查输入 A2）。
 */
import type { ChatUi } from "../../api";
import type { Dict } from "../../i18n/zh";

/**
 * 回合在跑时从对话页发消息的去向：
 * - insert：Agent 下一步就读到，插进当前回合。
 * - queue：排到当前回合结束后处理。
 * - handoff：消息交给了 Agent，何时处理没有取证，不许诺时机。
 * - unavailable：这家没有声明运行中的提交键，回合结束后才能发。
 */
export type RunningSendMode = "insert" | "queue" | "handoff" | "unavailable";

export function runningSendMode(chatUi: Pick<ChatUi, "queue_input" | "queue_delivery"> | null | undefined): RunningSendMode {
  if (!chatUi?.queue_input) return "unavailable";
  if (chatUi.queue_delivery === "next_step") return "insert";
  if (chatUi.queue_delivery === "turn_end") return "queue";
  return "handoff";
}

/** 回合在跑时输入框的占位：说清发出去会怎样。手机上没有 Enter / Ctrl+Enter 可讲，只说结果。 */
export function runningPlaceholder(
  t: Dict,
  mode: RunningSendMode,
  options: { canInterrupt: boolean; remote: boolean },
): string {
  if (mode === "unavailable") return t.chat.inputPlaceholderBusy;
  const copy = t.chat.runningSend[mode];
  if (options.remote) return copy.placeholderRemote;
  return options.canInterrupt ? copy.placeholder : copy.placeholderNoInterrupt;
}

/** 排队栈顶上的回执：n 条已发出、还没被读到的消息现在在哪。与占位同一个去向。 */
export function runningReceipt(t: Dict, mode: RunningSendMode, count: number): string {
  if (mode === "unavailable" || count <= 0) return "";
  return t.chat.runningSend[mode].receipt(count);
}

/** 占位与回执共用的动词（插入 / 排队 / 交给 Agent）。测试据此断言两处说的是同一件事。 */
export function runningSendVerb(t: Dict, mode: RunningSendMode): string {
  return mode === "unavailable" ? "" : t.chat.runningSend[mode].verb;
}

/**
 * 终端像是在等选择、但屏幕上没认出是哪一种时的横幅。此时消息被拦下、没有发出
 * （正文后的 Enter 可能落到原生审批的默认项上），横幅如实说「没有发出」。
 */
export function unrecognizedPromptNotice(t: Dict): string {
  return t.chat.unrecognizedPromptNotice;
}
