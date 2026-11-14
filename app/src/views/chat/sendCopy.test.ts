import { describe, expect, it } from "vitest";
import { chatUi } from "../../test/agents";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";
import {
  runningPlaceholder, runningReceipt, runningSendMode, runningSendVerb, sendBlockedMessage, unrecognizedPromptNotice,
} from "./sendCopy";

const PROVIDERS = ["claude", "codex", "kimi", "gemini", "opencode"] as const;
const LANGS = [["zh", zh], ["en", en]] as const;

/// 验收 2：文案与发送结果对应。占位是发之前的许诺，回执是发之后的说法，两处必须是同一个去向。
describe("回合在跑时发消息的去向", () => {
  it("去向只看能力声明：Claude 插入，Kimi 不许诺时机，其余三家排队", () => {
    expect(Object.fromEntries(PROVIDERS.map((provider) => [provider, runningSendMode(chatUi(provider))]))).toEqual({
      claude: "insert",
      codex: "queue",
      kimi: "handoff",
      gemini: "queue",
      opencode: "queue",
    });
    expect(runningSendMode(null)).toBe("unavailable");
    expect(runningSendMode({ queue_input: null, queue_delivery: "turn_end" })).toBe("unavailable");
  });

  describe.each(LANGS)("%s", (_, t) => {
    it.each(PROVIDERS)("%s：占位（桌面带中断、桌面不带中断、手机）与回执用同一个动词", (provider) => {
      const mode = runningSendMode(chatUi(provider));
      const verb = runningSendVerb(t, mode);
      expect(verb).not.toBe("");
      const texts = [
        runningPlaceholder(t, mode, { canInterrupt: true, remote: false }),
        runningPlaceholder(t, mode, { canInterrupt: false, remote: false }),
        runningPlaceholder(t, mode, { canInterrupt: false, remote: true }),
        runningReceipt(t, mode, 1),
        runningReceipt(t, mode, 3),
      ];
      for (const text of texts) expect(text.toLowerCase(), text).toContain(verb.toLowerCase());
      // 别的去向的动词不许混进来（Kimi 的占位此前写着「排队」）。
      const others = (["insert", "queue", "handoff"] as const).filter((other) => other !== mode).map((other) => runningSendVerb(t, other));
      for (const text of texts) for (const other of others) expect(text.toLowerCase(), text).not.toContain(other.toLowerCase());
    });

    it("手机占位不讲键盘，桌面带中断时写出 Ctrl+Enter", () => {
      for (const mode of ["insert", "queue", "handoff"] as const) {
        expect(runningPlaceholder(t, mode, { canInterrupt: true, remote: true })).not.toMatch(/Enter/);
        expect(runningPlaceholder(t, mode, { canInterrupt: true, remote: false })).toContain("Ctrl+Enter");
        expect(runningPlaceholder(t, mode, { canInterrupt: false, remote: false })).not.toContain("Ctrl+Enter");
      }
    });

    it("没有声明运行中提交键：占位说回合结束后才能发，不出回执", () => {
      expect(runningPlaceholder(t, "unavailable", { canInterrupt: true, remote: false })).toBe(t.chat.inputPlaceholderBusy);
      expect(runningReceipt(t, "unavailable", 2)).toBe("");
      expect(runningReceipt(t, "queue", 0)).toBe("");
    });
  });
});

describe("消息被拦下时的文案", () => {
  it("没认出终端提示的横幅如实说没有发出，不说已发出", () => {
    expect(unrecognizedPromptNotice(zh)).toContain("没有发出");
    expect(unrecognizedPromptNotice(zh)).not.toMatch(/已发出|已发送/);
    expect(unrecognizedPromptNotice(en)).toMatch(/was not sent/i);
    expect(unrecognizedPromptNotice(en)).not.toMatch(/was sent|been sent/i);
  });

  it("错误条按拦下的原因说要先做什么", () => {
    expect(sendBlockedMessage(zh, "question")).toBe(zh.chat.questionNeedsAnswer);
    expect(sendBlockedMessage(zh, "interaction")).toBe(zh.chat.terminalNeedsAttention);
    expect(sendBlockedMessage(en, "question")).toBe(en.chat.questionNeedsAnswer);
  });
});
