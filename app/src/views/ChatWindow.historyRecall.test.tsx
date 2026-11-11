// UI 调研 chat #12：召回一条多行提示词想改中间某一行，按 ↑ 就被换成另一条消息。
// shell 与 Claude Code CLI 的惯例：光标在首行才翻上一条，↓ 同理只在末行生效。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";

const MULTI = "第一行\n第二行\n第三行";

describe("↑ 召回历史", () => {
  it("多行消息里光标不在首行时 ↑ 是行间移动，不翻上一条；↓ 同理只在末行翻", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    respondWithHistory({
      sessionId: 61, title: "召回", status: "idle", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 2, reset: false, pendingReview: null, connected: true, ptyManaged: true,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "更早的一条" },
        { type: "user_text", id: "u2", timestamp: null, text: MULTI },
      ],
    });
    const { container } = render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("召回")).toBeTruthy());
    const box = container.querySelector<HTMLTextAreaElement>("textarea")!;
    box.focus();
    fireEvent.keyDown(box, { key: "ArrowUp" });
    await waitFor(() => expect(box.value).toBe(MULTI));

    // 光标在第三行：↑ 不抢，交给原生光标上移。
    box.setSelectionRange(MULTI.length, MULTI.length);
    const up = fireEvent.keyDown(box, { key: "ArrowUp" });
    expect(up).toBe(true); // 没被 preventDefault
    expect(box.value).toBe(MULTI);

    // ↓ 光标在第一行：也不翻。
    box.setSelectionRange(1, 1);
    expect(fireEvent.keyDown(box, { key: "ArrowDown" })).toBe(true);
    expect(box.value).toBe(MULTI);

    // 光标在第一行：↑ 翻上一条。
    box.setSelectionRange(1, 1);
    fireEvent.keyDown(box, { key: "ArrowUp" });
    await waitFor(() => expect(box.value).toBe("更早的一条"));

    // 单行消息光标在末行：↓ 翻回下一条。
    fireEvent.keyDown(box, { key: "ArrowDown" });
    await waitFor(() => expect(box.value).toBe(MULTI));
  });
});
