import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

// UI 调研 m3：手机上新建会话卡在启动（信任目录/登录询问把 CLI 卡住、认领迟迟不来）时只写一句
// 「启动等待中，请在桌面端确认」，违反「不推用户去桌面端」。临时负 id 在后端能取屏能写键
// （pty.rs screen_probe_snapshot 走 lookup 解析临时 id，issue #72），直接给终端画面 + 按键兜底。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (p: string) => p }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: vi.fn(() => Promise.resolve()), setTitle: vi.fn(() => Promise.resolve()), onDragDropEvent: () => Promise.resolve(() => {}) }),
}));
vi.mock("./ManagedTerminal", () => ({ ManagedTerminal: () => null }));

import { ChatWindow } from "./ChatWindow";
import { markRemoteUi } from "../remoteMode";
import { zh } from "../i18n/zh";

afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("手机新建会话卡在启动", () => {
  it("超时后就地给终端画面和按键，能完成信任确认，不再说去桌面端", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    markRemoteUi();
    window.history.replaceState({}, "", "/?sessionId=-5");
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_screen") {
        return Promise.resolve(["", " Do you trust the files in this folder?", " ❯ 1. Yes, proceed", "   2. No, exit", ""]);
      }
      return Promise.resolve(undefined);
    });
    render(<ChatWindow />);
    await screen.findByText(zh.chat.emptyStarting);
    await act(async () => { vi.advanceTimersByTime(8_100); });
    expect(await screen.findByText(/Do you trust the files in this folder/)).toBeTruthy();
    expect(screen.getByText(zh.chat.startingStuckRemote)).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/桌面端/);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.remoteKeyEnter }));
    expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: -5, data: "\r" });
  });
});
