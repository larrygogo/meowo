// 输入法确认候选词的那一下回车不能把消息发出去（走查输入 A7）。WebKit（macOS、iOS）上这一下回车的
// isComposing 已经是 false，只有 keyCode 还是 229；对话窗输入框与贴纸输入共用 imeComposing 这个助手。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";

const { setup, box, writes } = adapter;

describe("对话窗输入框的输入法守卫", () => {
  it("keyCode 229 的回车（WebKit 确认候选词）不发送，普通回车才发送", async () => {
    setup(850, "claude");
    render(<ChatWindow />);
    await screen.findByText("适配-850");
    await chatUiReady();
    fireEvent.change(box(), { target: { value: "ni hao" } });
    fireEvent.keyDown(box(), { key: "Enter", keyCode: 229 });
    fireEvent.keyDown(box(), { key: "Enter", isComposing: true });
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(writes(850)).toEqual([]);
    expect(box().value).toBe("ni hao");
    fireEvent.change(box(), { target: { value: "你好" } });
    fireEvent.keyDown(box(), { key: "Enter", keyCode: 13 });
    await waitFor(() => expect(writes(850)).toContain("你好"));
  });
});
