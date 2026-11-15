import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";

const errorBarButtons = () => [...document.querySelector(".chat-send-error")!.querySelectorAll("button")].map((button) => button.textContent);

describe("independent review draft protection", () => {
  // 上一条的正文写进了终端、提交前状态读取失败：正文还停在 CLI 输入框里。再发时 Meowo 自己用 CLI 的清空键
  // 清掉那份残稿再写（此前报「终端输入框已有草稿…请先在终端发送或清空」，没有出口，走查输入 B6）。
  // 残稿是 Meowo 自己写进去的，不当成用户的草稿搬进对话页输入框。
  it("a failed second state read leaves the body in the terminal; the retry clears it and writes once more", async () => {
    const base = adapter.setup(9752, "codex");
    let stuck = false;
    let rejected = false;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "PARTIAL_DELIVERY") stuck = true;
      if (command === "write_managed_terminal" && args?.data === "\u0003") stuck = false;
      if (command === "write_managed_terminal" && args?.data === "\r") stuck = false;
      if (command === "managed_terminal_frame") return Promise.resolve({ lines: [stuck ? "› PARTIAL_DELIVERY" : "› ", "", "  GPT-6.1-Sol default · C:/repo"], cursorRow: 0, cursorCol: stuck ? 18 : 2, cursorVisible: true });
      if (command === "get_chat_history" && stuck && !rejected) {
        rejected = true;
        return Promise.reject(new Error("independent transient connection failure"));
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-9752");
    await chatUiReady();
    fireEvent.change(adapter.box(), { target: { value: "PARTIAL_DELIVERY" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await screen.findByText("independent transient connection failure");
    expect(adapter.box().value).toBe("");
    expect(adapter.writes(9752)).toEqual(["PARTIAL_DELIVERY"]);
    fireEvent.change(adapter.box(), { target: { value: "PARTIAL_DELIVERY" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    // 清空键 → 正文 → 回车：终端里只留下一份正文，并且真的提交了。
    await waitFor(() => expect(adapter.writes(9752)).toEqual(["PARTIAL_DELIVERY", "\u0003", "PARTIAL_DELIVERY", "\r"]), { timeout: 10_000 });
    expect(adapter.box().value).toBe("");
    expect(screen.queryByText(zh.chat.nativeDraftMoved)).toBeNull();
  }, 20_000);

  // 认不出输入框（菜单、对话框、布局漂移）：不往里按键、不写正文，原文留在对话页。错误条说明原因，并给能在
  // 对话页完成的出路：看终端画面，或者人看过之后点「仍然发送」。
  it.each(["codex", "gemini"])("%s refuses a native frame it cannot recognize, keeps the web draft, and offers a way out", async (provider) => {
    const base = adapter.setup(9751, provider);
    invoke.mockImplementation((command, args) => command === "managed_terminal_frame"
      ? Promise.resolve({ lines: ["native draft already present"], cursorRow: 0, cursorCol: 5, cursorVisible: true })
      : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-9751");
    await chatUiReady();
    fireEvent.change(adapter.box(), { target: { value: "WEB_NEW_MESSAGE" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await screen.findByText(zh.chat.composerUnrecognized);
    expect(adapter.writes(9751)).toEqual([]);
    expect(adapter.box().value).toBe("WEB_NEW_MESSAGE");
    expect(invoke.mock.calls.some(([command]) => command === "managed_terminal_frame")).toBe(true);
    expect(errorBarButtons()).toEqual([zh.chat.composerSendAnyway, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.composerSendAnyway }));
    await waitFor(() => expect(adapter.writes(9751)).toContain("WEB_NEW_MESSAGE"), { timeout: 4000 });
    await waitFor(() => expect(adapter.box().value).toBe(""));
  });
});
