/// 切换确认框的判据：前后端读同一份样例（test/fixtures/switch-confirm.json，真机帧来自
/// tests/probe_claude_model.rs；后端 registry::tests::switch_confirm_frames_agree_with_declared_spec）。
import { describe, expect, it } from "vitest";

import type { ChatUi } from "../../api";
import fixture from "../../test/fixtures/switch-confirm.json";
import { chatUi } from "../../test/agents";
import { switchConfirmInput } from "./switchConfirm";

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
    const lines = ["   Switch model?", "   ❯ 1. Yes, switch to Opus 5.5 (1M context)", "     2. No, go back"];
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
