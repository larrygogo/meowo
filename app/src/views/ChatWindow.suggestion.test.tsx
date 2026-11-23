// 建议回复接到对话页（.lead/20261123-next-step 验收 4，独立审查第 1、3 条）。夹具与 mock 见 chatWindow.harness.tsx。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import composerFrames from "../test/fixtures/composer-frames.json";

const SUGGESTION = "加个测试跑一下";
const frame = (() => {
  const shot = composerFrames.frames.find((entry) => entry.name === "claude-2.1.330-suggestion-zh")!;
  return { lines: shot.lines, cursorRow: shot.cursor!.row, cursorCol: shot.cursor!.col, cursorVisible: true, cursorDim: true, cols: 120, rows: 32 };
})();
const chip = () => document.querySelector<HTMLButtonElement>(".chat-suggest-chip");

describe("建议回复：对话页输入框上方", () => {
  it("读到 Claude 的建议后出现；Ctrl+Tab 不填；Tab 与点按钮只填不发；发出去后不再冒出来", async () => {
    const base = adapter.setup(773, "claude");
    invoke.mockImplementation((command, args) => command === "managed_terminal_frame"
      ? Promise.resolve({ ...frame, lines: [...frame.lines] })
      : command === "get_chat_history" ? Promise.resolve({ ...adapter.history(773, "claude"), ptyManaged: true })
      : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-773");
    await chatUiReady();
    await waitFor(() => expect(chip()?.textContent).toContain(SUGGESTION), { timeout: 4_000 });

    // 带修饰键的 Tab 归别人（Ctrl+Tab 切面板页签），不填。
    fireEvent.keyDown(adapter.box(), { key: "Tab", ctrlKey: true });
    expect(adapter.box().value).toBe("");

    // 空输入框按 Tab：填进去，不发；按钮随之收起。
    fireEvent.keyDown(adapter.box(), { key: "Tab" });
    expect(adapter.box().value).toBe(SUGGESTION);
    expect(chip()).toBeNull();
    expect(adapter.writes(773)).toEqual([]);

    // 清空后按钮回来；点按钮同样只填。
    fireEvent.change(adapter.box(), { target: { value: "" } });
    expect(chip()).not.toBeNull();
    fireEvent.click(chip()!);
    expect(adapter.box().value).toBe(SUGGESTION);
    expect(adapter.writes(773)).toEqual([]);

    // 发出去：终端画面还是那句建议（CLI 还没反应过来），对话页不再拿出来，Tab 也不再填回。
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await waitFor(() => expect(adapter.writes(773)).toContain(SUGGESTION), { timeout: 3_000 });
    await waitFor(() => expect(adapter.box().value).toBe(""));
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 1_800)); });
    expect(chip()).toBeNull();
    fireEvent.keyDown(adapter.box(), { key: "Tab" });
    expect(adapter.box().value).toBe("");
  }, 15_000);
});
