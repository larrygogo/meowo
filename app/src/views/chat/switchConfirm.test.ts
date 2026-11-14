/// 切换确认框的判据：前后端读同一份样例（test/fixtures/switch-confirm.json，真机帧来自
/// tests/probe_claude_model.rs；后端 registry::tests::switch_confirm_frames_agree_with_declared_spec）。
import { describe, expect, it } from "vitest";

import type { ChatUi } from "../../api";
import fixture from "../../test/fixtures/switch-confirm.json";
import { chatUi } from "../../test/agents";
import { terminalAttention, type TerminalAttention } from "../../terminalAttention";
import { grammar } from "../../test/agents";
import { beginSwitchHold, endSwitchHold, holdSwitchDialog, releaseSwitchHold, switchConfirmInput, switchConfirmVerdict } from "./switchConfirm";

type Spec = NonNullable<ChatUi["switch_confirm"]>;
const SPECS = fixture.spec as Record<string, Spec>;

describe("对话中途切换的二次确认框", () => {
  it.each(fixture.frames.map((frame) => [frame.name, frame] as const))("%s", (_name, frame) => {
    expect(switchConfirmInput(frame.lines, SPECS[frame.provider], frame.target)).toBe(frame.expect);
  });

  it("真机帧前面带着别的输出、框被重绘了两遍，照样认最后一个", () => {
    const real = fixture.frames.find((frame) => frame.name === "claude-2.1.325-model-haiku")!;
    const lines = ["❯ /model haiku", "▔▔▔▔▔▔▔▔", ...real.lines, "▔▔▔▔▔▔▔▔", ...real.lines];
    expect(switchConfirmInput(lines, SPECS.claude, "haiku")).toBe("\r");
  });

  it("带方括号变体的模型按家族名核对（opus[1m] → opus）", () => {
    const lines = ["   Switch model?", "   Your next response will be slower and use more tokens", "   ❯ 1. Yes, switch to Opus 5.5 (1M context)", "     2. No, go back"];
    expect(switchConfirmInput(lines, SPECS.claude, "opus")).toBe("\r");
  });

  it("没声明这种框的家、目标为空：一律不按", () => {
    const real = fixture.frames[0];
    expect(switchConfirmInput(real.lines, null, "haiku")).toBeNull();
    expect(switchConfirmInput(real.lines, SPECS.claude, "")).toBeNull();
  });

  it("只有 claude 声明了这种框", () => {
    expect(chatUi("claude")?.switch_confirm).toEqual(SPECS.claude);
    for (const provider of ["codex", "kimi", "gemini", "opencode"]) {
      expect(chatUi(provider)?.switch_confirm ?? null).toBeNull();
    }
  });
});

/// 做空审查 2026-11-14 ⑬：同一个确认组件还用于「PreModelSwitch hook 要求你确认」，此前会被一起代按。
describe("只代按「切到刚选的那一个」这一种确认", () => {
  it.each(fixture.frames.map((frame) => [frame.name, frame] as const))("%s：处置与样例一致", (_name, frame) => {
    expect(switchConfirmVerdict(frame.lines, SPECS[frame.provider], frame.target).kind).toBe(frame.verdict);
  });

  it("hook 要求确认的框：不给确认键", () => {
    const hook = fixture.frames.find((frame) => frame.name === "claude-hook-asked")!;
    expect(switchConfirmInput(hook.lines, SPECS.claude, "haiku")).toBeNull();
    expect(switchConfirmVerdict(hook.lines, SPECS.claude, "haiku")).toEqual({ kind: "manual" });
  });

  it.each(fixture.frames.filter((frame) => frame.verdict !== "none").map((frame) => [frame.name, frame] as const))(
    "%s：这个框被屏幕识别认成一张选择卡，两个选项照原文、按键从焦点算", (_name, frame) => {
      const attention = terminalAttention(frame.lines.join("\n"), [], false, false, grammar("claude"));
      expect(attention?.kind).toBe("choice");
      expect(attention?.options).toHaveLength(2);
      const focusOnCancel = frame.lines.some((line) => /^\s*❯.*go back/i.test(line));
      // 焦点在取消项上时，确认项要先上移一格再回车；不盲按回车。
      expect(attention?.options?.map((option) => option.input)).toEqual(focusOnCancel ? ["\x1b[A\r", "\r"] : ["\r", "\x1b[B\r"]);
    });

  it("正文里引用了标题与确认项（不是整行的标题）：不出卡", () => {
    expect(terminalAttention("● 它会问 Switch model? 然后\n   ❯ 1. Yes, switch to Haiku 5.5", [], false, false, grammar("claude"))).toBeNull();
  });
});

describe("切换进行中确认框的卡片先压着", () => {
  const card = (kind: TerminalAttention["kind"]): TerminalAttention => ({ id: "x", kind, text: "Switch model?" });

  it("没有切换在进行：不压，照常上屏", () => {
    expect(holdSwitchDialog(1, card("choice"), () => {})).toBe(false);
  });

  it("切换进行中认出的选择卡被压住；决定代按后丢掉，不上屏", () => {
    const shown: TerminalAttention[] = [];
    beginSwitchHold(2);
    expect(holdSwitchDialog(2, card("choice"), (attention) => shown.push(attention))).toBe(true);
    endSwitchHold(2);
    expect(shown).toEqual([]);
    expect(holdSwitchDialog(2, card("choice"), () => {})).toBe(false);
  });

  it("决定不代按：压着的卡交出去，之后的照常上屏", () => {
    const shown: TerminalAttention[] = [];
    beginSwitchHold(3);
    // 屏幕识别还没认出来：交不出去，继续压着。
    expect(releaseSwitchHold(3)).toBe(false);
    holdSwitchDialog(3, card("choice"), (attention) => shown.push(attention));
    expect(releaseSwitchHold(3)).toBe(true);
    expect(shown).toEqual([card("choice")]);
    expect(holdSwitchDialog(3, card("choice"), () => {})).toBe(false);
  });

  it("只压选择卡：切换期间来的审批与提问照常上屏；别的会话不受影响", () => {
    beginSwitchHold(4);
    expect(holdSwitchDialog(4, card("approval"), () => {})).toBe(false);
    expect(holdSwitchDialog(4, card("question"), () => {})).toBe(false);
    expect(holdSwitchDialog(5, card("choice"), () => {})).toBe(false);
    endSwitchHold(4);
  });
});
