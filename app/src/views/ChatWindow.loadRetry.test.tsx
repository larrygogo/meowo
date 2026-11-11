// UI 调研 chat #13：读取对话失败态只有一行红字「读取对话失败，正在重试」，没有重试按钮也没有原因。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";

describe("读取对话失败", () => {
  it("给「立即重试」，点了马上再读一次，成功后恢复；连续失败写出原因", async () => {
    window.history.replaceState({}, "", "/?sessionId=71");
    let fail = true;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") {
        return fail ? Promise.reject("transcript/unreadable") : Promise.resolve({
          sessionId: 71, title: "恢复了", status: "idle", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 1, reset: false, pendingReview: null, connected: true, ptyManaged: false,
          items: [{ type: "user_text", id: "u1", timestamp: null, text: "你好" }],
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const retry = await screen.findByRole("button", { name: "立即重试" });
    // 连续失败（轮询 650ms 一次，三次即同步中断）后写出原因。
    await waitFor(() => expect(document.querySelector(".chat-load-reason")?.textContent).toBeTruthy(), { timeout: 4000 });
    const calls = () => invoke.mock.calls.filter(([c]) => c === "get_chat_history").length;
    const before = calls();
    fail = false;
    fireEvent.click(retry);
    expect(calls()).toBe(before + 1);
    expect(await screen.findByText("你好")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "立即重试" })).toBeNull();
  });
});
