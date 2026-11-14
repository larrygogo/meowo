/// 要在终端里现场打字的作答：屏幕通道的「说明并拒绝」、提问表单的自定义答案。
///
/// 三步，每一步都不盲按（流程由插件声明，见后端 chat_ui::TypedInput）：
/// 1. 写 `openInput`，让那一项进入等输入的状态；
/// 2. 读仿真后的整屏，认出 `readyPattern` 才往下走；
/// 3. 写正文，再写 `submitInput`。
///
/// 为什么不能一次写完：审批框里数字键就是选项（kimi 面板里 `1` 是 Approve once，claude 的权限框
/// 同样认数字）。第一步的键没生效（框已经换了、焦点不在预想的位置）时，后面的正文会被当成按键，
/// 说明里带一个 `1` 就成了放行。所以第二步认不出就停：不打字，由调用方告诉用户没有送达。
/// 真机流程见 tests/probe_deny_feedback.rs（claude 2.1.325、kimi 2.6.1，2026-11-14）。

import { managedTerminalScreen, writeManagedTerminal } from "../../api";
import type { TypedAnswer } from "../../terminalAttention";

/// 等输入状态最多等多久。真机上按键后 0.2 秒内出现（两家都是），留足余量给慢机器与远程。
export const TYPED_READY_TIMEOUT_MS = 4_000;
const POLL_MS = 150;
/// 正文与提交键之间的间隔：合成一次写会被一些 TUI 当成粘贴，回车落进正文里（见 terminalSubmit）。
const SUBMIT_GAP_MS = 300;

/// `sent`：三步走完。`not_ready`：第一步之后屏幕上没出现等输入的标志，正文没有写。
export type TypedAnswerOutcome = "sent" | "not_ready";

type Deps = {
  write: (sessionId: number, data: string) => Promise<void>;
  screen: (sessionId: number) => Promise<string[] | null>;
  sleep: (ms: number) => Promise<void>;
  now: () => number;
};

const realDeps: Deps = {
  write: writeManagedTerminal,
  screen: managedTerminalScreen,
  sleep: (ms) => new Promise((resolve) => window.setTimeout(resolve, ms)),
  now: () => Date.now(),
};

/// 屏幕上有没有等输入的标志。规则来自后端，逐行、不分大小写；坏正则当作认不出。
export function typedAnswerReady(lines: readonly string[], flow: Pick<TypedAnswer, "readyPattern">): boolean {
  try {
    return new RegExp(flow.readyPattern, "im").test(lines.join("\n"));
  } catch {
    return false;
  }
}

/// 正文只能是一行：换行在这些输入框里就是提交，多出来的行会落到提交之后的画面上。
export function typedAnswerText(text: string): string {
  return text.replace(/\s*[\r\n]+\s*/g, " ").trim();
}

export async function typeAnswerIntoTerminal(
  sessionId: number,
  flow: TypedAnswer,
  text: string,
  deps: Deps = realDeps,
): Promise<TypedAnswerOutcome> {
  const body = typedAnswerText(text);
  // 空正文不该走到这里（卡片在理由为空时不让提交）；真来了也不按任何键。
  if (!body) return "not_ready";
  await deps.write(sessionId, flow.openInput);
  const deadline = deps.now() + TYPED_READY_TIMEOUT_MS;
  let ready = false;
  while (!ready && deps.now() < deadline) {
    await deps.sleep(POLL_MS);
    const lines = await deps.screen(sessionId);
    ready = lines !== null && typedAnswerReady(lines, flow);
  }
  if (!ready) return "not_ready";
  await deps.write(sessionId, body);
  await deps.sleep(SUBMIT_GAP_MS);
  await deps.write(sessionId, flow.submitInput);
  return "sent";
}
