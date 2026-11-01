import { describe, expect, it } from "vitest";
import { fmtElapsed, parseCliRetry, parseCliStatus } from "./cliStatus";
import { CLAUDE_RETRY_LINE_PATTERN, CLAUDE_STATUS_LINE_PATTERN } from "../../test/agents";

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

/// 自动重试行：样本照 claude 2.1.312 包内渲染源码拼出（重试不落 transcript，屏幕是唯一证据）。
describe("parseCliRetry", () => {
  const screen = (line: string) => ["● 我来看一下", "", line, "────────────", "❯ "].join("\n");

  it("认出三种带上限的重试行", () => {
    expect(parseCliRetry(screen("✗ API Error (529 Overloaded) · Retrying in 5s · attempt 2/10"), CLAUDE_RETRY_LINE_PATTERN)).toEqual({ attempt: 2, max: 10 });
    expect(parseCliRetry(screen("✗ No response from the API after 30s · retrying, waiting up to 1m · attempt 3/10"), CLAUDE_RETRY_LINE_PATTERN)).toEqual({ attempt: 3, max: 10 });
    expect(parseCliRetry(screen("✗ Auto mode check unavailable · next try in 5s · attempt 1/3 · esc to interrupt"), CLAUDE_RETRY_LINE_PATTERN)).toEqual({ attempt: 1, max: 3 });
  });

  it("带限额重置时刻的说法也认", () => {
    expect(parseCliRetry(screen("✗ Rate limited · Retrying in 2m (3:15pm) · attempt 4/10"), CLAUDE_RETRY_LINE_PATTERN)).toEqual({ attempt: 4, max: 10 });
  });

  /// 容量排队（无上限）是在等空位，不是失败；正文里引用的同款半句也不能当成正在重试。
  it("容量排队与正文引用不误认", () => {
    expect(parseCliRetry(screen("⚠ Waiting for capacity · next try in 30s · attempt 3 · esc to interrupt"), CLAUDE_RETRY_LINE_PATTERN)).toBeNull();
    expect(parseCliRetry(screen("日志里写着 Retrying in 5s · attempt 2/10，后来就好了"), CLAUDE_RETRY_LINE_PATTERN)).toBeNull();
  });

  it("只看屏幕底部一截：更上面的旧重试行不算", () => {
    const old = ["✗ API Error · Retrying in 5s · attempt 2/10", ...Array.from({ length: 20 }, (_, i) => `正文第 ${i} 行`), "❯ "].join("\n");
    expect(parseCliRetry(old, CLAUDE_RETRY_LINE_PATTERN)).toBeNull();
  });
});
