import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { invoke, adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";

describe("independent review draft protection", () => {
  it("a failed second state read preserves an uncertain message and prevents a second body write", async () => {
    const base = adapter.setup(9752, "codex");
    let rejectRead = false;
    let rejected = false;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "PARTIAL_DELIVERY") rejectRead = true;
      if (command === "managed_terminal_frame") return Promise.resolve({ lines: [rejectRead ? "› PARTIAL_DELIVERY" : "› ", "", "  GPT-6.1-Sol default · C:/repo"], cursorRow: 0, cursorCol: rejectRead ? 18 : 2, cursorVisible: true });
      if (command === "get_chat_history" && rejectRead && !rejected) {
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
    await screen.findByText(/终端输入框已有草稿/);
    expect(adapter.writes(9752)).toEqual(["PARTIAL_DELIVERY"]);
  });
  it.each(["codex", "gemini"])("%s refuses a native frame it cannot prove empty and preserves the web draft", async (provider) => {
    const base = adapter.setup(9751, provider);
    invoke.mockImplementation((command, args) => command === "managed_terminal_frame"
      ? Promise.resolve({ lines: ["native draft already present"], cursorRow: 0, cursorCol: 5, cursorVisible: true })
      : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-9751");
    await chatUiReady();
    fireEvent.change(adapter.box(), { target: { value: "WEB_NEW_MESSAGE" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await screen.findByText(/无法确认终端输入框是否为空/);
    expect(adapter.writes(9751)).toEqual([]);
    expect(adapter.box().value).toBe("WEB_NEW_MESSAGE");
    expect(invoke.mock.calls.some(([command]) => command === "managed_terminal_frame")).toBe(true);
  });
});
