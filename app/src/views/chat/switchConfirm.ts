/// 带参切换命令（`/model sonnet`、`/effort high`）在对话中途弹的二次确认框：认出来、核对是这次要切的
/// 那一个，才给出确认键。规格由插件声明（ChatUi.switch_confirm，见后端 chat_ui::SwitchConfirm）。
///
/// 为什么需要：claude 2.1.325 在会话里已经有过回合时不直接切，先问 `Switch model?`
/// （真机取证 2026-11-14，tests/probe_claude_model.rs）。不认这个框，命令发出去后等不到确认行，
/// 只能报「未确认」，框留在终端里把之后发的消息吞掉；回合结束后自动执行的切换同样撞上。
///
/// 只代按一种：框里写着已知的理由（缓存，下一条回复更慢）、要切到的正是刚选的那一个、焦点在确认项上。
/// 同一个组件还用于「PreModelSwitch hook 要求你确认」，那是用户自己配的 hook 要人来点头，不能代按
/// （做空审查 2026-11-14）。不代按的框也不留在终端：它作为一张普通选择卡交给用户。

import type { ChatUi } from "../../api";
import type { TerminalAttention } from "../../terminalAttention";

type SwitchConfirm = NonNullable<ChatUi["switch_confirm"]>;

const squash = (text: string) => text.toLowerCase().replace(/[^a-z0-9]/g, "");

/// 这一屏上的确认框怎么处置：
/// - `none`：没有确认框；
/// - `confirm`：可以代按，`input` 是确认键；
/// - `manual`：有确认框，但不该代按（hook 要求确认、理由不认识、焦点在取消项上、要切到的是别的目标）。
export type SwitchConfirmVerdict = { kind: "none" } | { kind: "confirm"; input: string } | { kind: "manual" };

export function switchConfirmVerdict(lines: string[], spec: SwitchConfirm | null | undefined, target: string): SwitchConfirmVerdict {
  if (!spec) return { kind: "none" };
  const title = new RegExp(spec.title_pattern, "i");
  // 取最后一个标题：重绘会把同一个框画好几遍。
  let titleAt = -1;
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    if (title.test(lines[index])) { titleAt = index; break; }
  }
  if (titleAt < 0) return { kind: "none" };
  const wanted = squash(target);
  const accept = new RegExp(spec.accept_pattern, "i");
  const auto = new RegExp(spec.auto_pattern, "i");
  const manual = spec.manual_pattern ? new RegExp(spec.manual_pattern, "i") : null;
  const body = lines.slice(titleAt + 1);
  const acceptAt = body.findIndex((line) => accept.test(line));
  // 带焦点的确认项没找到：焦点在取消项上，或框还没画完。都不按。
  if (acceptAt < 0 || !wanted) return { kind: "manual" };
  const between = body.slice(0, acceptAt);
  if (!between.some((line) => auto.test(line))) return { kind: "manual" };
  if (manual && between.some((line) => manual.test(line))) return { kind: "manual" };
  const shown = squash(accept.exec(body[acceptAt])?.[1] ?? "");
  return shown.includes(wanted) ? { kind: "confirm", input: spec.accept_input } : { kind: "manual" };
}

/// 可以代按时返回确认键，否则 null。
export function switchConfirmInput(lines: string[], spec: SwitchConfirm | null | undefined, target: string): string | null {
  const verdict = switchConfirmVerdict(lines, spec, target);
  return verdict.kind === "confirm" ? verdict.input : null;
}

// ── 切换进行中，确认框的卡片先压着 ──
//
// 屏幕识别每 150ms 扫一次，确认框一出现就会认成选择卡；而代按要等下一次轮询（200ms）。不压着的话，
// 每次切模型对话页都会闪出一张卡又消失。所以切换命令发出后，这个会话新认出的「等你选择」卡先存着：
// 决定代按就丢掉（框随即消失）；决定不代按就原样交出去，由用户在卡上选。
// 只压「等你选择」这一种：审批、提问照常出卡。
type Hold = { stash: TerminalAttention | null; deliver: ((attention: TerminalAttention) => void) | null };
const holds = new Map<number, Hold>();

/// 切换命令即将发出：开始压卡。
export function beginSwitchHold(sessionId: number): void {
  holds.set(sessionId, { stash: null, deliver: null });
}

/// 对话窗认出一张卡时问一句：压住了就返回 true（这一帧不上屏）。
export function holdSwitchDialog(sessionId: number, attention: TerminalAttention, deliver: (attention: TerminalAttention) => void): boolean {
  const hold = holds.get(sessionId);
  if (!hold || attention.kind !== "choice") return false;
  hold.stash = attention;
  hold.deliver = deliver;
  return true;
}

/// 决定不代按：把压着的卡交出去，之后认出的照常上屏。返回是否真的交出了一张卡。
/// 还没有压着的卡（屏幕识别比这边的轮询慢一拍）时返回 false 并继续压着，调用方下一轮再问。
export function releaseSwitchHold(sessionId: number): boolean {
  const hold = holds.get(sessionId);
  if (!hold?.stash || !hold.deliver) return false;
  holds.delete(sessionId);
  hold.deliver(hold.stash);
  return true;
}

/// 切换了结（代按成功、被拒、超时）：停止压卡，压着的那张不再需要。
export function endSwitchHold(sessionId: number): void {
  holds.delete(sessionId);
}
