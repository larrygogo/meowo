// 终端草稿读不全时从对话页发送（做空①）：不清终端、说明原因、出路都在对话页上，并写运行日志（做空⑧）。
//
// 终端画面是本机真实 CLI 的帧（test/fixtures/composer-frames.json 里名字带 -form- 的，
// tests/probe_draft_fidelity.rs 2026-11-14 实拍）。夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi } from "../test/agents";
import composerFrames from "../test/fixtures/composer-frames.json";

const { setup, box, writes } = adapter;

type Sample = { name: string; lines: string[]; cols?: number; rows?: number; cursor?: { row: number; col: number; visible: boolean; dim: boolean } };
function liveFrame(name: string) {
  const frame = (composerFrames.frames as Sample[]).find((entry) => entry.name === name);
  if (!frame) throw new Error(`没有这一帧：${name}`);
  return {
    lines: frame.lines,
    cursorRow: frame.cursor?.row ?? 0,
    cursorCol: frame.cursor?.col ?? 0,
    cursorVisible: frame.cursor?.visible ?? false,
    cursorDim: frame.cursor?.dim ?? false,
    cols: frame.cols,
    rows: frame.rows,
  };
}
const errorBar = () => document.querySelector(".chat-send-error")!;
const buttons = () => [...errorBar().querySelectorAll("button")].map((button) => button.textContent);
const diagLines = () => invoke.mock.calls.filter(([command]) => command === "diag_log").map(([, args]) => (args as { message: string }).message);

// 四家各取一种读不全的形态：粘贴占位、被框宽折行、超出可视高度。
const cases = [
  { provider: "codex", draft: "codex-0.167.2-form-paste_big_line", empty: "codex-0.167.2-placeholder-cursor", message: zh.chat.composerDraftPaste, why: "paste", cleared: "\x03", clear: ["\x03"] },
  { provider: "kimi", draft: "kimi-2.6.1-form-wrap_words", empty: "kimi-2.6.1-cleared", message: zh.chat.composerDraftWrapped, why: "wrap", cleared: "\x03", clear: ["\x03"] },
  { provider: "gemini", draft: "gemini-0.68.0-form-tall", empty: "gemini-0.68-placeholder-cursor", message: zh.chat.composerDraftOverflow, why: "overflow", cleared: "\x15", clear: ["\x0b", "\x15"] },
  { provider: "opencode", draft: "opencode-1.19.5-form-paste_small", empty: "opencode-1.19.5-placeholder-cursor", message: zh.chat.composerDraftPaste, why: "paste", cleared: "\x03", clear: ["\x03"] },
];
const BODY = "对话页的新消息 body-0519";

async function sendOver(provider: string, draft: string, empty: string, cleared: string, sessionId: number) {
  const base = setup(sessionId, provider);
  let draftGone = false;
  invoke.mockImplementation((command, args) => {
    if (command === "write_managed_terminal" && args?.data === cleared) draftGone = true;
    if (command === "managed_terminal_frame") return Promise.resolve(liveFrame(draftGone ? empty : draft));
    return base(command, args);
  });
  render(<ChatWindow />);
  await screen.findByText(`适配-${sessionId}`);
  await chatUiReady();
  fireEvent.change(box(), { target: { value: BODY } });
  fireEvent.keyDown(box(), { key: "Enter" });
}

describe("终端草稿读不全时从对话页发送", () => {
  it.each(cases)("$provider：不清终端、不写正文，说明原因，给两个选择、看终端画面和收起；写一行日志", async ({ provider, draft, empty, cleared, message, why }) => {
    const sessionId = 830 + cases.findIndex((entry) => entry.provider === provider);
    await sendOver(provider, draft, empty, cleared, sessionId);
    await screen.findByText(message, {}, { timeout: 8000 });
    // 一个键也没按：终端里的草稿原封不动。
    expect(writes(sessionId)).toEqual([]);
    // 消息还在对话页输入框里，没有把读到的半截草稿塞进来。
    expect(box().value).toBe(BODY);
    expect(screen.queryByText(zh.chat.nativeDraftMoved)).toBeNull();
    expect(buttons()).toEqual([zh.chat.composerSendWithDraft, zh.chat.composerDiscardDraft, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
    await waitFor(() => expect(diagLines().some((line) => line.includes("kind=send_blocked"))).toBe(true));
    expect(diagLines().find((line) => line.includes("kind=send_blocked")))
      .toBe(`kind=send_blocked · provider=${provider} · session=${sessionId} · reason=composer_draft_incomplete · why=${why}`);
    for (const line of diagLines()) expect(line).not.toContain("body-0519");
  }, 15_000);

  it.each(cases)("$provider：点「连同草稿发送」，草稿留着，换行隔开后正文照常发出", async ({ provider, draft, empty, cleared, message }) => {
    const sessionId = 840 + cases.findIndex((entry) => entry.provider === provider);
    await sendOver(provider, draft, empty, cleared, sessionId);
    await screen.findByText(message, {}, { timeout: 8000 });
    fireEvent.click(screen.getByRole("button", { name: zh.chat.composerSendWithDraft }));
    const newline = chatUi(provider)!.newline_input!;
    await waitFor(() => expect(writes(sessionId).slice(0, 2)).toEqual([newline, BODY]), { timeout: 6000 });
    await waitFor(() => expect(writes(sessionId)).toContain("\r"), { timeout: 6000 });
    // 全程没有按清空键。
    expect(writes(sessionId)).not.toContain(cleared);
  }, 20_000);

  it.each(cases)("$provider：点「放弃草稿后发送」要再确认一次，确认后清掉草稿再发，草稿不进对话页", async ({ provider, draft, empty, cleared, clear, message }) => {
    const sessionId = 850 + cases.findIndex((entry) => entry.provider === provider);
    await sendOver(provider, draft, empty, cleared, sessionId);
    await screen.findByText(message, {}, { timeout: 8000 });
    fireEvent.click(screen.getByRole("button", { name: zh.chat.composerDiscardDraft }));
    // 第一下只换成确认文案，什么都没发。
    expect(writes(sessionId)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.composerDiscardConfirm }));
    await waitFor(() => expect(writes(sessionId)).toEqual([...clear, BODY, "\r"]), { timeout: 6000 });
    expect(screen.queryByText(zh.chat.nativeDraftMoved)).toBeNull();
  }, 20_000);
});
