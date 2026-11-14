import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { zh } from "../../i18n/zh";
import { markRemoteUi } from "../../remoteMode";
import { terminalAttention, type TerminalAttention } from "../../terminalAttention";
import { grammar } from "../../test/agents";
import { OPENCODE_ALWAYS_CONFIRM_SCREEN } from "../../test/requestSamples";
import { promptContext, TerminalPromptCard, TerminalQuestionCard } from "./TerminalRequestCards";

afterEach(() => {
  cleanup();
  (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
  document.body.classList.remove("remote-ui");
});

const noop = () => {};
const focus = { current: null };

/// kimi 2.4.1 的提问表单（真机取证 2026-11-05，与 terminalAttention.test.ts 的样本同一份）。
const KIMI_RULE = " " + "─".repeat(60);
const kimiForm = (...body: string[]) => [KIMI_RULE, "  question", ...body, KIMI_RULE, " Ask When Needed  K3 thinking: high"].join("\n");
const KIMI_MULTI = kimiForm(
  "  (✓) 颜色   水果    Submit",
  "  ? 喜欢哪些水果？",
  "   [ ] 苹果",
  "   [✓] 香蕉",
  "   [ ] 橙子",
  "   [ ] Other",
  "   ↑↓ select  1-4 / ↵ toggle  ←/→/tab switch  esc cancel",
);

/// gemini 0.64 的额度耗尽框（真机，与 terminalAttention.test.ts 的样本同一份）。
const GEMINI_USAGE_LIMIT = [
  " > say hi",
  "╭──────────────────────────────────────────────╮",
  "│                                              │",
  "│ Usage limit reached for gemini-3.8-flash.    │",
  "│ /stats model for usage details               │",
  "│ /model to switch models.                     │",
  "│                                              │",
  "│ ● 1. Keep trying                             │",
  "│   2. Stop                                    │",
  "╰──────────────────────────────────────────────╯",
].join("\n");

function recognized(provider: string, screenText: string, markers: string[] = [], interactive = false): TerminalAttention {
  const attention = terminalAttention(screenText, markers, interactive, false, grammar(provider, provider === "claude"
    ? [{ marker: "type something", kind: "input" }, { marker: "chat about this", kind: "chat" }] : []));
  if (!attention) throw new Error(`${provider} 的样本没认出来`);
  return attention;
}

describe("提问卡（屏幕通道）：三家的表单同一张卡", () => {
  const cases: [string, TerminalAttention][] = [
    ["claude 编号选择器", recognized("claude", "Which library?\n❯ 1. Zod\n  2. Yup\n  3. Type something.\n  4. Chat about this\nEnter to select · ↑/↓ to navigate · Esc to cancel", [], true)],
    ["kimi 方括号表单", recognized("kimi", KIMI_MULTI)],
    ["gemini 框线表单", recognized("gemini", [
      "╭──────────────────────────────────────────────╮",
      "│ Answer Questions                             │",
      "│ Which database should we use?                │",
      "│ ● 1.  Postgres                               │",
      "│   2.  SQLite                                 │",
      "│   3.  Enter a custom value                   │",
      "╰──────────────────────────────────────────────╯",
    ].join("\n"))],
  ];

  /// 走查 输入 A3：kimi 的提问此前落在提示卡上，徽章写「待批准」。
  it.each(cases)("%s：标题是「在等你回答」，徽章是「待回答」", (_name, attention) => {
    expect(attention.kind).toBe("question");
    render(<TerminalQuestionCard attention={attention} kind="question" live customText="" returnFocusTo={focus}
      onChoose={noop} onCustomText={noop} onCustomSubmit={noop} onCollapse={noop} />);
    const card = screen.getByRole("alertdialog");
    expect(card.getAttribute("aria-label")).toBe("Agent 正在等待你的回答");
    expect(card.querySelector(".chat-approval-badge")?.textContent).toBe("待回答");
    expect(card.querySelector(".chat-approval-badge")?.textContent).not.toBe("待批准");
  });

  it("计划模式的选择器：同一张卡，标题与徽章换成计划批准那一套", () => {
    render(<TerminalQuestionCard attention={cases[0][1]} kind="plan" live customText="" returnFocusTo={focus}
      onChoose={noop} onCustomText={noop} onCustomSubmit={noop} onCollapse={noop} />);
    const card = screen.getByRole("alertdialog");
    expect(card.getAttribute("aria-label")).toBe("Agent 正在等待计划审批");
    expect(card.querySelector(".chat-approval-badge")?.textContent).toBe("待批准");
  });

  it("kimi 多选：已勾的项画勾（标签里不再重复一个 ✓），去下一页的按钮叫「下一步」", () => {
    const onChoose = vi.fn();
    render(<TerminalQuestionCard attention={cases[1][1]} kind="question" live customText="" returnFocusTo={focus}
      onChoose={onChoose} onCustomText={noop} onCustomSubmit={noop} onCollapse={noop} />);
    const options = Array.from(document.querySelectorAll(".chat-approval-options button"));
    expect(options.map((button) => button.querySelector("b")?.textContent)).toEqual(["苹果", "香蕉", "橙子"]);
    expect(options.map((button) => button.classList.contains("is-selected"))).toEqual([false, true, false]);
    fireEvent.click(options[0]);
    expect(onChoose).toHaveBeenCalledWith(expect.objectContaining({ label: "苹果", input: "1" }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(onChoose).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "submit", input: "\t" }));
  });

  it("外部终端里跑的会话：选项不可点，并说明原因", () => {
    const onChoose = vi.fn();
    render(<TerminalQuestionCard attention={cases[1][1]} kind="question" live={false} customText="" returnFocusTo={focus}
      onChoose={onChoose} onCustomText={noop} onCustomSubmit={noop} onCollapse={noop} />);
    expect(screen.getByText(zh.chat.questionExternalHint)).toBeTruthy();
    for (const button of Array.from(document.querySelectorAll<HTMLButtonElement>(".chat-approval-options button"))) expect(button.disabled).toBe(true);
  });

  it("自定义答案输入框：输入法合成中的回车不提交，WebKit 只剩 keyCode 229 的那一下也不提交", () => {
    const withInput = cases.map(([, attention]) => attention).find((attention) => (attention.options ?? []).some((option) => option.kind === "input"))!;
    expect(withInput, "样本里应有带自定义答案的表单").toBeTruthy();
    const onCustomSubmit = vi.fn();
    render(<TerminalQuestionCard attention={withInput} kind="question" live customText="zhong" returnFocusTo={focus}
      onChoose={noop} onCustomText={noop} onCustomSubmit={onCustomSubmit} onCollapse={noop} />);
    const input = document.querySelector<HTMLInputElement>(".chat-approval-custom input")!;
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
    expect(onCustomSubmit).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    expect(onCustomSubmit).toHaveBeenCalledTimes(1);
  });

  it("手机页：同一张卡，同样的标题、徽章与选项", () => {
    markRemoteUi();
    render(<TerminalQuestionCard attention={cases[1][1]} kind="question" live customText="" returnFocusTo={focus}
      onChoose={noop} onCustomText={noop} onCustomSubmit={noop} onCollapse={noop} />);
    const card = screen.getByRole("alertdialog");
    expect(card.getAttribute("aria-label")).toBe("Agent 正在等待你的回答");
    expect(card.querySelector(".chat-approval-badge")?.textContent).toBe("待回答");
    expect(document.querySelectorAll(".chat-approval-options button")).toHaveLength(3);
  });
});

describe("提示卡（屏幕通道）：标题与徽章由种类决定", () => {
  const card = (attention: TerminalAttention, live = true, handlers: Partial<{ onChoose: () => void; onKey: (input: string) => void }> = {}) =>
    render(<TerminalPromptCard attention={attention} live={live} returnFocusTo={focus}
      onChoose={handlers.onChoose ?? noop} onKey={handlers.onKey ?? noop} onCollapse={noop} />);

  /// 走查 输入 A3：gemini 额度耗尽此前标题是「Agent 需要你完成一个启动步骤」，徽章「待批准」。
  it("gemini 额度耗尽：是「终端正在等待你的选择」，不是启动步骤，也不是待批准", () => {
    const attention = recognized("gemini", GEMINI_USAGE_LIMIT);
    card(attention);
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.getAttribute("aria-label")).toBe("终端正在等待你的选择");
    expect(dialog.getAttribute("aria-label")).not.toBe(zh.chat.terminalPromptTitle);
    expect(dialog.querySelector(".chat-approval-badge")?.textContent).toBe("待选择");
    // 选项没拆成按钮（框线选项，未声明文法）：终端原文摆出来，用户看得到回车选的是 Keep trying。
    // 框线是排版不是内容：去掉之后五行就能看全，回车选的是带 ● 的那一项。
    expect(dialog.querySelector("pre")?.textContent).toBe([
      "Usage limit reached for gemini-3.8-flash.",
      "/stats model for usage details",
      "/model to switch models.",
      "● 1. Keep trying",
      "  2. Stop",
    ].join("\n"));
  });

  it.each([
    ["信任页", () => recognized("codex", "Do you trust the contents of this directory?\n> 1. Yes, continue\n  2. No, quit", ["do you trust the contents of this directory"]), "是否信任此文件夹？", "待确认"],
    ["登录类启动提示", () => recognized("claude", "OAuth token has been revoked\nPress Enter to continue"), "Agent 需要你完成一个启动步骤", "待确认"],
    ["claude 计划批准", () => recognized("claude", "Claude has written up a plan and is ready to execute. Would you like to proceed?\n❯ 1. Yes, and bypass permissions\n  2. Yes, manually approve edits\n  3. Tell Claude what to change"), "Agent 正在等待计划审批", "待批准"],
    ["claude 长会话确认", () => recognized("claude", "This session is 2h 14m old and 148.2k tokens.\nResuming the full session will consume a substantial portion of your usage limits.\n❯ 1. Resume from summary (recommended)\n  2. Resume full session as-is\n  3. Don't ask me again"), "终端正在等待你的选择", "待选择"],
    ["opencode 的 Always allow 确认页", () => recognized("opencode", OPENCODE_ALWAYS_CONFIRM_SCREEN), "终端正在等待你的选择", "待选择"],
  ])("%s：标题「%s」徽章「%s」", (_name, make, title, badge) => {
    card(make());
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.getAttribute("aria-label")).toBe(title);
    expect(dialog.querySelector(".chat-approval-badge")?.textContent).toBe(badge);
  });

  it("认出选项时一项一个按钮，按钮上是终端原文；没认出时给确认与取消两键", () => {
    const onChoose = vi.fn();
    card(recognized("opencode", OPENCODE_ALWAYS_CONFIRM_SCREEN), true, { onChoose });
    expect(Array.from(document.querySelectorAll(".chat-approval-options button")).map((b) => b.textContent)).toEqual(["Confirm", "Cancel"]);
    expect(screen.queryByRole("button", { name: "确认" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(onChoose).toHaveBeenCalledWith(expect.objectContaining({ input: "\r" }));
    cleanup();
    const onKey = vi.fn();
    card(recognized("gemini", GEMINI_USAGE_LIMIT), true, { onKey });
    fireEvent.click(screen.getByRole("button", { name: "确认" }));
    fireEvent.click(screen.getByRole("button", { name: "取消 / 返回" }));
    expect(onKey.mock.calls.map(([input]) => input)).toEqual(["\r", "\x1b"]);
  });

  it("「等你选择」的卡把题干摆出来（选项之前的原文），其余种类只出选项", () => {
    const confirm = recognized("opencode", OPENCODE_ALWAYS_CONFIRM_SCREEN);
    expect(promptContext(confirm, "choice")).toBe("△ Always allow\nThis will allow the following patterns until OpenCode is restarted\n- C:\\Windows\\*");
    const trust = recognized("codex", "Do you trust the contents of this directory?\n> 1. Yes, continue\n  2. No, quit", ["do you trust the contents of this directory"]);
    expect(promptContext(trust, "trust")).toBe("");
    // 没认出选项：整段原文。
    expect(promptContext({ text: "raw prompt", options: undefined }, "startup")).toBe("raw prompt");
  });
});
