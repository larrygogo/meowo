import { describe, expect, it } from "vitest";
import { fmtElapsed, parseCliStatus } from "./cliStatus";
import { CLAUDE_STATUS_LINE_PATTERN } from "../../test/agents";

/** 运行条照搬 claude 的状态行（2026-10 用户要求「改成终端这样的」）。 */
describe("parseCliStatus", () => {
  it("认出输入框上方的状态行：动词 + 括号统计", () => {
    const screen = [
      "● 我来看一下",
      "",
      "✻ Composing… (41s · ↓ 427 tokens · thought for 4s)",
      "  ⎿  Tip: Use /btw to ask a quick side question",
      "────────────",
      "❯ ",
    ].join("\n");
    expect(parseCliStatus(screen, CLAUDE_STATUS_LINE_PATTERN)).toEqual({ verb: "Composing…", detail: "41s · ↓ 427 tokens · thought for 4s" });
  });

  it("刚开跑还没有统计时只给动词；esc to interrupt 是终端键位提示，不搬", () => {
    expect(parseCliStatus("✶ Thinking…", CLAUDE_STATUS_LINE_PATTERN)).toEqual({ verb: "Thinking…", detail: null });
    expect(parseCliStatus("· Clauding… (3s · esc to interrupt)", CLAUDE_STATUS_LINE_PATTERN)).toEqual({ verb: "Clauding…", detail: "3s" });
  });

  it("正文里的普通括号与列表不误认", () => {
    expect(parseCliStatus("* 这是列表项…", CLAUDE_STATUS_LINE_PATTERN)).toBeNull();
    // 括号不以耗时数字开头 = 不是状态行的统计，整行不认（宁可少报）。
    expect(parseCliStatus("✻ Loading… (see docs)", CLAUDE_STATUS_LINE_PATTERN)).toBeNull();
    expect(parseCliStatus("plain text (41s)", CLAUDE_STATUS_LINE_PATTERN)).toBeNull();
  });

  it("多条长得像的行取最靠下那条", () => {
    expect(parseCliStatus("✻ Old… (9s)\n正文\n✻ Composing… (2s)", CLAUDE_STATUS_LINE_PATTERN)?.verb).toBe("Composing…");
  });
});

describe("fmtElapsed", () => {
  it("与终端同一副写法", () => {
    expect(fmtElapsed(41_000)).toBe("41s");
    expect(fmtElapsed(125_000)).toBe("2m 5s");
    expect(fmtElapsed(3_780_000)).toBe("1h 3m");
  });
});
