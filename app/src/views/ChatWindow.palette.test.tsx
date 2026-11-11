// UI 优化 #53：Ctrl+K 里的「切换模型」「切换权限模式」等于点底栏那颗按钮，面板命令走面板快捷键同一组动作。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";

async function openPalette() {
  fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true });
  await screen.findByPlaceholderText(zh.chat.switcherPlaceholder);
}

describe("ChatWindow 命令面板", () => {
  it("「切换模型」打开底栏模型菜单", async () => {
    adapter.setup(531);
    render(<ChatWindow />);
    await screen.findByText("适配-531");
    await chatUiReady();
    await openPalette();
    fireEvent.click(await screen.findByRole("option", { name: zh.chat.switchModel }));
    expect(await screen.findByRole("menuitem", { name: /^Sonnet/ })).toBeTruthy();
  });

  it("「切换权限模式」打开权限菜单，面板命令带快捷键", async () => {
    adapter.setup(532);
    render(<ChatWindow />);
    await screen.findByText("适配-532");
    await chatUiReady();
    await openPalette();
    // 有工作目录：面板页签命令都在，右侧标着与面板快捷键一致的键位。
    expect((await screen.findByRole("option", { name: new RegExp(zh.chat.shortcutPanel) })).querySelector("kbd")?.textContent).toBe("Ctrl+Shift+B");
    expect(screen.getByRole("option", { name: new RegExp(zh.chat.shortcutShell) }).querySelector("kbd")?.textContent).toBe("Ctrl+`");
    fireEvent.click(screen.getByRole("option", { name: zh.chat.cmdSwitchPermission }));
    await waitFor(() => expect(document.querySelector(".chat-mode-button[aria-expanded='true']")).not.toBeNull());
  });
});
