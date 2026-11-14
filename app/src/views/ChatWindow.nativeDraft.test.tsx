// 终端输入框里有草稿时从对话页发送（走查输入 B6）、手敲撤回命令（A4）、选模型途中切会话（A5）。
//
// 终端画面用共享样例里的实拍帧回放（test/fixtures/composer-frames.json，2026-11-14 本机真实 CLI），
// 按键之后切到哪一帧照真机的反应写。夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emptyComposerFrame, invoke, eventListeners, adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import composerFrames from "../test/fixtures/composer-frames.json";

const { setup, box, writes, history } = adapter;

type Sample = { name: string; lines: string[]; cursor?: { row: number; col: number; visible: boolean; dim: boolean } };
function liveFrame(name: string) {
  const frame = (composerFrames.frames as Sample[]).find((entry) => entry.name === name)!;
  return {
    lines: frame.lines,
    cursorRow: frame.cursor?.row ?? 0,
    cursorCol: frame.cursor?.col ?? 0,
    cursorVisible: frame.cursor?.visible ?? false,
    cursorDim: frame.cursor?.dim ?? false,
  };
}
const errorBar = () => document.querySelector(".chat-send-error[role=alert], .chat-send-error")!;

describe("终端输入框里有草稿时从对话页发送", () => {
  // 四家各自取证过的清空键：按下去草稿清掉，内容搬进对话页输入框，然后消息照常写入并回车。
  const cases = [
    { provider: "codex", draft: "codex-0.167.2-same-text-draft", empty: "codex-0.167.2-placeholder-cursor", clear: ["\x03"], cleared: "\x03", text: "Ask Codex to do anything" },
    { provider: "kimi", draft: "kimi-2.6.1-draft", empty: "kimi-2.6.1-cleared", clear: ["\x03"], cleared: "\x03", text: "DRAFT-TWO" },
    { provider: "gemini", draft: "gemini-0.68-same-text-draft", empty: "gemini-0.68-placeholder-cursor", clear: ["\x0b", "\x15"], cleared: "\x15", text: "Type your message or @path/to/file" },
    { provider: "opencode", draft: "opencode-1.19.5-draft-multiline", empty: "opencode-1.19.5-placeholder-cursor", clear: ["\x03"], cleared: "\x03", text: "DRAFT-ONE\nDRAFT-TWO" },
  ];
  it.each(cases)("$provider：Meowo 自己清掉草稿、内容进对话页输入框，消息照常发出", async ({ provider, draft, empty, clear, cleared, text }) => {
    const sessionId = 800 + cases.findIndex((entry) => entry.provider === provider);
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
    fireEvent.change(box(), { target: { value: "对话页的新消息" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    // 清空键 → 正文 → 回车，一个多余的键都没有。
    await waitFor(() => expect(writes(sessionId)).toEqual([...clear, "对话页的新消息", "\r"]), { timeout: 4000 });
    // 草稿没丢：搬进了对话页输入框，并有一句说明。
    await waitFor(() => expect(box().value).toBe(text));
    expect(screen.getByText(zh.chat.nativeDraftMoved)).toBeTruthy();
    // 没有「请先在终端发送或清空该草稿」这类无出口提示。
    expect(document.body.textContent).not.toMatch(/请先在终端|终端输入框已有草稿/);
  }, 10_000);

  it.each(cases)("$provider：清不掉时不写正文，错误条说明原因并给重试与看终端画面", async ({ provider, draft }) => {
    const sessionId = 810 + cases.findIndex((entry) => entry.provider === provider);
    const base = setup(sessionId, provider);
    invoke.mockImplementation((command, args) => (command === "managed_terminal_frame" ? Promise.resolve(liveFrame(draft)) : base(command, args)));
    render(<ChatWindow />);
    await screen.findByText(`适配-${sessionId}`);
    await chatUiReady();
    fireEvent.change(box(), { target: { value: "对话页的新消息" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await screen.findByText(zh.chat.composerDraftStuck, {}, { timeout: 8000 });
    expect(writes(sessionId)).not.toContain("对话页的新消息");
    expect(writes(sessionId)).not.toContain("\r");
    // 原文留在对话页，出路都在对话页上。
    expect(box().value).toBe("对话页的新消息");
    expect([...errorBar().querySelectorAll("button")].map((button) => button.textContent)).toEqual([zh.chat.errorRetry, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
  }, 15_000);

  it("gemini 停在 shell 模式：先按 Esc 退出再发，不把消息当成 shell 命令", async () => {
    const base = setup(820, "gemini");
    let left = false;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\x1b") left = true;
      if (command === "managed_terminal_frame") return Promise.resolve(liveFrame(left ? "gemini-0.68-placeholder-cursor" : "gemini-0.68-shell-mode"));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-820");
    await chatUiReady();
    fireEvent.change(box(), { target: { value: "你好" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(820)).toEqual(["\x1b", "你好", "\r"]), { timeout: 4000 });
  });

  it("gemini 开着 vim 模式的空输入框算空的，照常发送（此前被当成「已有草稿」）", async () => {
    const base = setup(821, "gemini");
    invoke.mockImplementation((command, args) => (command === "managed_terminal_frame"
      ? Promise.resolve(liveFrame("gemini-0.68-vim-insert-placeholder")) : base(command, args)));
    render(<ChatWindow />);
    await screen.findByText("适配-821");
    await chatUiReady();
    fireEvent.change(box(), { target: { value: "你好" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(821)).toEqual(["你好", "\r"]), { timeout: 4000 });
  });
});

describe("手敲撤回命令走撤回流程", () => {
  // 走查输入 A4：OpenCode 的 `/undo` 原样发出去没有「文件会一并还原」的确认，CLI 放回它自己输入框的原文
  // 也没人清，下一条消息被拦住。现在与消息上的「撤回」按钮同一条路。
  it("opencode：敲 /undo 回车先弹文件会还原的确认，撤完原文回到对话页、终端里那份清掉", async () => {
    const base = setup(830, "opencode");
    let submitted = false;
    let cleared = false;
    const confirms: unknown[] = [];
    const items = [
      { type: "user_text", id: "u1", timestamp: null, text: "先做甲" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" },
    ];
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return base(command, args).then((h: object) => (submitted ? { ...h, reset: true, offset: 0, items: [] } : { ...h, offset: 5, items }));
      if (command === "confirm_dialog") { confirms.push(args); return Promise.resolve(true); }
      if (command === "write_managed_terminal" && args?.data === "\r") submitted = true;
      if (command === "write_managed_terminal" && args?.data === "\x03") cleared = true;
      // 真机形态（1.19.5）：撤回后原文回到 CLI 输入框。
      if (command === "managed_terminal_screen") return Promise.resolve(["  ┃  先做甲", "     ▣  Build · model", "  ┃", submitted && !cleared ? "  ┃  先做甲" : "  ┃", "  ┃", "  ┃  Build · model", "  ╹▀▀▀▀"]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("甲做完了");
    await chatUiReady();
    fireEvent.change(box(), { target: { value: "/undo" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(JSON.stringify(confirms)).toContain(zh.chat.undoConfirmFiles));
    await waitFor(() => expect(writes(830)).toContain("/undo"), { timeout: 4000 });
    await waitFor(() => expect(box().value).toBe("先做甲"), { timeout: 4000 });
    await screen.findByText(zh.chat.undoDone, {}, { timeout: 4000 });
    expect(cleared).toBe(true);
  }, 15_000);

  it("opencode：确认框点了取消就什么都不发", async () => {
    const base = setup(831, "opencode");
    const items = [{ type: "user_text", id: "u1", timestamp: null, text: "先做甲" }, { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" }];
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return base(command, args).then((h: object) => ({ ...h, offset: 5, items }));
      if (command === "confirm_dialog") return Promise.resolve(false);
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("opencode"));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("甲做完了");
    await chatUiReady();
    fireEvent.change(box(), { target: { value: "/undo" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(invoke.mock.calls.some(([command]) => command === "confirm_dialog")).toBe(true));
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(writes(831)).toEqual([]);
  });
});

describe("用终端菜单选模型途中切会话", () => {
  // 走查输入 A5：兜底路线（发 /model 再替用户点菜单）等菜单的 12 秒里切到别的会话，转圈一直转，
  // 超时的「没等到切换确认」挂到新会话上，条上的「重试」还会在新会话里按旧 id 选模型。
  it("转圈与超时错误条都不串到新会话", async () => {
    const base = setup(840, "kimi");
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return Promise.resolve(history(Number(args?.sessionId), "kimi"));
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "kimi-code/a", label: "当前模型", group: "g", efforts: [] },
        { id: "kimi-code/b", label: "另一个模型", group: "g", efforts: [] },
      ]);
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      // 会话够不着切换服务：退回终端菜单兜底。
      if (command === "apply_session_settings") return Promise.resolve({ kind: "unsupported" });
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-840");
    await chatUiReady();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /另一个模型/ }));
    // 兜底路线发出了 /model，按钮开始转圈。
    await waitFor(() => expect(writes(840)).toContain("/model"), { timeout: 4000 });
    await waitFor(() => expect(document.querySelector(".chat-model-spinner")).toBeTruthy());
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 841 }));
    await screen.findByText("适配-841");
    // 新会话：不转圈。
    await waitFor(() => expect(document.querySelector(".chat-model-spinner")).toBeNull());
    // 过了 12 秒的超时点：错误条没有出现，也没有往新会话的终端写任何东西。
    await new Promise((resolve) => setTimeout(resolve, 12_500));
    expect(screen.queryByText(zh.chat.modelSwitchUnconfirmed)).toBeNull();
    expect(writes(841)).toEqual([]);
  }, 30_000);
});
