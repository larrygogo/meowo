// 发送文案与发送结果对应（验收 2）。夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, adapter, chatUiReady, emptyComposerFrame } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi } from "../test/agents";
import { runningPlaceholder, runningReceipt, runningSendMode } from "./chat/sendCopy";

const REMOTE_FLAG = "__MEOWO_REMOTE__";
afterEach(() => { delete (globalThis as Record<string, unknown>)[REMOTE_FLAG]; });

describe("软拦横幅与实际一致（走查输入 A2）", () => {
  it.each(["claude", "codex", "kimi", "gemini", "opencode"])("%s：消息被拦下没发，横幅说没有发出", async (provider) => {
    const base = adapter.setup(771, provider);
    invoke.mockImplementation((command, args) => command === "get_chat_history"
      ? Promise.resolve({ ...adapter.history(771, provider), pendingReview: "permission" })
      : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-771");
    fireEvent.change(adapter.box(), { target: { value: "这条会被拦下" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    // 发送结果：终端里一个字都没写。
    await screen.findByText(zh.chat.cardLockApproval);
    expect(adapter.writes(771)).toEqual([]);
    // 文案：横幅说的是「没有发出」，页面上不出现「消息已发出」。
    expect(screen.getByText(zh.chat.unrecognizedPromptNotice).textContent).toContain("没有发出");
    expect(document.body.textContent).not.toContain("消息已发出");
    // 原文还在输入框里，用户不用重打。
    expect(adapter.box().value).toBe("这条会被拦下");
  });
});

describe("运行中发送的占位与回执同词（走查输入 B5）", () => {
  it.each([
    ["claude", false], ["codex", false], ["kimi", false], ["gemini", false], ["opencode", false],
    ["kimi", true], ["codex", true],
  ] as const)("%s（手机=%s）：占位许诺的去向就是发出后回执说的去向", async (provider, remote) => {
    if (remote) (globalThis as Record<string, unknown>)[REMOTE_FLAG] = true;
    const base = adapter.setup(772, provider, "running");
    // 四家发送前要读终端画面确认输入框是空的。
    invoke.mockImplementation((command, args) => command === "managed_terminal_frame"
      ? Promise.resolve(emptyComposerFrame(provider))
      : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-772");
    await chatUiReady();
    const mode = runningSendMode(chatUi(provider));
    const copy = zh.chat.runningSend[mode as "insert" | "queue" | "handoff"];
    // 发之前：占位。
    await waitFor(() => expect(adapter.box().getAttribute("placeholder")).toContain(copy.verb));
    expect([true, false].map((canInterrupt) => runningPlaceholder(zh, mode, { canInterrupt, remote })))
      .toContain(adapter.box().getAttribute("placeholder"));
    // 发出去。
    fireEvent.change(adapter.box(), { target: { value: "运行中再发一条" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await waitFor(() => expect(adapter.writes(772)).toContain("运行中再发一条"), { timeout: 3_000 });
    // 发之后：排队栈回执，与占位同一个动词。
    const receipt = await screen.findByText(runningReceipt(zh, mode, 1), {}, { timeout: 3_000 });
    expect(receipt.textContent).toContain(copy.verb);
    if (provider === "kimi") {
      // Kimi 没声明送达时机：占位与回执都不许诺「排队」。
      expect(adapter.box().getAttribute("placeholder")).not.toContain("排队");
      expect(receipt.textContent).not.toContain("排队");
    }
  });
});
