import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// 关对话窗前的确认（chat/windowClose.ts）：面板终端里有程序在跑时先问，取消就不关。
// 此前 ChatWindow 的关闭直接 getCurrentWindow().close()，后端销毁窗口时 kill_all 面板终端连子进程。
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
import { resetTrackedShellsForTest } from "./chat/windowClose";

const SESSION = 77;
let busy: { id: number; title: string; processes: { pid: number; name: string }[] }[] = [];
let confirmAnswer = false;
let shells: { id: number; owner: number; title: string; exited: boolean }[] = [];

beforeEach(() => {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
  });
  window.history.replaceState({}, "", `/?sessionId=${SESSION}`);
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({
      sessionId: SESSION, provider: "claude", status: "waiting", title: "关窗确认", cwd: "C:/repo", connected: true,
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "shell_terminal_list") return Promise.resolve(shells);
    if (command === "browser_pane_list") return Promise.resolve([]);
    if (command === "shell_terminal_busy") return Promise.resolve(busy);
    if (command === "confirm_dialog") return Promise.resolve(confirmAnswer);
    return Promise.resolve();
  });
});
afterEach(() => {
  cleanup();
  localStorage.clear();
  resetTrackedShellsForTest();
  win.close.mockClear();
  win.unlisten.mockClear();
  win.closeHandler = null;
  invoke.mockReset();
});

const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command);
const RUNNING = [{ id: 5, title: "pwsh", processes: [{ pid: 4242, name: "node" }] }];

// 标题栏右上角那颗（面板页签也有「关闭」，按类名取窗口控制组的）。
const closeButton = () => document.querySelector<HTMLButtonElement>(".win-close")!;

async function mount() {
  render(<ChatWindow />);
  await screen.findByText("关窗确认");
  // 面板终端清单回来、登记进关窗账本之后，才有确认可言。
  await waitFor(() => expect(calls("shell_terminal_list").length).toBeGreaterThan(0));
}

describe("关对话窗前确认面板终端", () => {
  it("标题栏关闭：有程序在跑 → 弹确认，取消就不关", async () => {
    shells = [{ id: 5, owner: SESSION, title: "pwsh", exited: false }];
    busy = RUNNING;
    confirmAnswer = false;
    await mount();
    fireEvent.click(closeButton());
    await waitFor(() => expect(calls("confirm_dialog")).toHaveLength(1));
    expect(String(calls("confirm_dialog")[0][1]?.message)).toContain("node (PID 4242)");
    await act(async () => { await Promise.resolve(); });
    expect(win.close).not.toHaveBeenCalled();
  });

  it("标题栏关闭：确认后才关，且先摘掉原生关闭的拦截（不会再进确认）", async () => {
    shells = [{ id: 5, owner: SESSION, title: "pwsh", exited: false }];
    busy = RUNNING;
    confirmAnswer = true;
    await mount();
    fireEvent.click(closeButton());
    await waitFor(() => expect(win.close).toHaveBeenCalledTimes(1));
    expect(win.unlisten).toHaveBeenCalled();
    expect(win.unlisten.mock.invocationCallOrder[0]).toBeLessThan(win.close.mock.invocationCallOrder[0]);
    expect(calls("confirm_dialog")).toHaveLength(1);
  });

  it("系统原生关闭：拦下后确认，取消不关；确认后摘拦截再关", async () => {
    shells = [{ id: 5, owner: SESSION, title: "pwsh", exited: false }];
    busy = RUNNING;
    confirmAnswer = false;
    await mount();
    await waitFor(() => expect(win.closeHandler).toBeTruthy());
    const cancelled = vi.fn();
    window.addEventListener("meowo:window-close-cancelled", cancelled);
    const first = { preventDefault: vi.fn() };
    await act(async () => { await win.closeHandler!(first); });
    expect(first.preventDefault).toHaveBeenCalled();
    await waitFor(() => expect(calls("confirm_dialog")).toHaveLength(1));
    await waitFor(() => expect(cancelled).toHaveBeenCalled());
    expect(win.close).not.toHaveBeenCalled();
    window.removeEventListener("meowo:window-close-cancelled", cancelled);

    confirmAnswer = true;
    const second = { preventDefault: vi.fn() };
    await act(async () => { await win.closeHandler!(second); });
    expect(second.preventDefault).toHaveBeenCalled();
    await waitFor(() => expect(win.close).toHaveBeenCalledTimes(1));
    expect(win.closeHandler).toBeNull();
  });

  it("终端都空闲：不弹确认，直接关", async () => {
    shells = [{ id: 5, owner: SESSION, title: "pwsh", exited: false }];
    busy = [];
    await mount();
    fireEvent.click(closeButton());
    await waitFor(() => expect(win.close).toHaveBeenCalledTimes(1));
    expect(calls("confirm_dialog")).toHaveLength(0);
  });

  it("面板里没开过终端：不挂原生关闭拦截，按钮直接关", async () => {
    shells = [];
    await mount();
    expect(win.closeHandler).toBeNull();
    fireEvent.click(closeButton());
    await waitFor(() => expect(win.close).toHaveBeenCalledTimes(1));
    expect(calls("shell_terminal_busy")).toHaveLength(0);
    expect(calls("confirm_dialog")).toHaveLength(0);
  });
});
