import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";

// 对话窗 2s 兜底轮询（测试里 650ms）此前不看 document.hidden：窗口最小化、手机锁屏后照样整份拉
// transcript。改成隐藏时跳过、重新可见立即补拉一次（hooks/useVisibleInterval.ts）。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path, Channel: class { onmessage = () => {}; } }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
const win = vi.hoisted(() => ({
  close: vi.fn(() => Promise.resolve()),
  unlisten: vi.fn(() => Promise.resolve()),
  closeHandler: null as null | ((event: { preventDefault: () => void }) => unknown),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    close: win.close,
    setTitle: () => Promise.resolve(),
    onDragDropEvent: () => Promise.resolve(() => {}),
    onResized: () => Promise.resolve(() => {}),
    isMaximized: () => Promise.resolve(false),
    onCloseRequested: (handler: (event: { preventDefault: () => void }) => unknown) => {
      win.closeHandler = handler;
      return Promise.resolve(() => {
        win.closeHandler = null;
        return win.unlisten();
      });
    },
  }),
}));
vi.mock("./ManagedTerminal", () => ({ ManagedTerminal: () => <div>PTY</div> }));
// 面板内容（xterm 等）与这里无关：只验关窗确认，面板终端的清单由 usePanel 照常向后端要。
vi.mock("./chat/SidePanel", () => ({ SidePanel: () => null }));

import { ChatWindow } from "./ChatWindow";

const SESSION = 78;
let hidden = false;

beforeEach(() => {
  hidden = false;
  Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
  });
  window.history.replaceState({}, "", `/?sessionId=${SESSION}`);
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({
      sessionId: SESSION, provider: "claude", status: "waiting", title: "轮询可见性", cwd: "C:/repo", connected: true,
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "shell_terminal_list" || command === "browser_pane_list") return Promise.resolve([]);
    return Promise.resolve();
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  localStorage.clear();
  invoke.mockReset();
  delete (document as { hidden?: boolean }).hidden;
});

const pulls = () => invoke.mock.calls.filter(([name]) => name === "get_chat_history").length;
const setHidden = (value: boolean) => {
  hidden = value;
  document.dispatchEvent(new Event("visibilitychange"));
};

describe("对话窗兜底轮询看页面可见性", () => {
  it("隐藏时不拉；重新可见立即补拉一次，之后照常轮询", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    render(<ChatWindow />);
    await screen.findByText("轮询可见性");
    // 可见时照常轮询（650ms 一拍）。
    const visibleStart = pulls();
    await act(async () => { await vi.advanceTimersByTimeAsync(650 * 3); });
    expect(pulls()).toBeGreaterThanOrEqual(visibleStart + 2);

    act(() => setHidden(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(50); });
    const hiddenStart = pulls();
    await act(async () => { await vi.advanceTimersByTimeAsync(650 * 6); });
    expect(pulls()).toBe(hiddenStart);

    act(() => setHidden(false));
    await act(async () => { await Promise.resolve(); });
    expect(pulls()).toBe(hiddenStart + 1);
    await act(async () => { await vi.advanceTimersByTimeAsync(650 * 2); });
    expect(pulls()).toBeGreaterThan(hiddenStart + 1);
  });
});
