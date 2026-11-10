import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

// 二期 #7：CLI 意外退出时对话页不再只说「会话已结束」，要说明是意外退出、退出码多少，
// 并附上最后几行输出（判定在后端 exit_report::is_unexpected_exit，随快照的 exitUnexpected 带来）。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path, Channel: class { onmessage = () => {}; } }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    close: () => Promise.resolve(),
    setTitle: () => Promise.resolve(),
    onDragDropEvent: () => Promise.resolve(() => {}),
    onResized: () => Promise.resolve(() => {}),
    isMaximized: () => Promise.resolve(false),
    onCloseRequested: () => Promise.resolve(() => {}),
  }),
}));
vi.mock("./ManagedTerminal", () => ({ ManagedTerminal: () => <div>PTY</div> }));
vi.mock("./chat/SidePanel", () => ({ SidePanel: () => null }));

import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";

const SESSION = 91;
let snapshot: Record<string, unknown> = {};

beforeEach(() => {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
  });
  window.history.replaceState({}, "", `/?sessionId=${SESSION}`);
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({
      sessionId: SESSION, provider: "claude", status: "ended", title: "崩了的会话", cwd: "C:/repo", connected: false,
      supported: true, offset: 0, reset: false, pendingReview: null, ptyManaged: false, items: [],
    });
    if (command === "managed_terminal_snapshot") return Promise.resolve({
      sessionId: SESSION, active: false, managed: false, data: "", startOffset: 0, endOffset: 0,
      exited: true, exitCode: null, exitTail: null, cols: 0, rows: 0, modes: [], externalViewers: false, exitUnexpected: false,
      ...snapshot,
    });
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "shell_terminal_list" || command === "browser_pane_list") return Promise.resolve([]);
    return Promise.resolve();
  });
});
afterEach(() => {
  cleanup();
  localStorage.clear();
  invoke.mockReset();
  snapshot = {};
});

describe("对话页显示 CLI 意外退出（二期 #7）", () => {
  it("意外退出：显示退出码并附最后几行输出", async () => {
    snapshot = {
      exitCode: 3,
      exitUnexpected: true,
      exitTail: "第一行\n第二行\nthread 'main' panicked at src/main.rs:1\n内存不足\n最后一行",
    };
    render(<ChatWindow />);
    expect(await screen.findByText(zh.chat.agentCrashed(3))).toBeTruthy();
    const tail = document.querySelector(".chat-compose-gate-tail");
    expect(tail?.textContent).toContain("最后一行");
    expect(tail?.textContent).toContain("panicked");
    // 只附最后几行，不把整段输出铺满输入框上方。
    expect(tail?.textContent).not.toContain("第一行");
    expect(screen.queryByText(zh.chat.composerGateEnded)).toBeNull();
  });

  it("用户结束 / 拿不到退出码（exitUnexpected=false）时仍是普通的已结束", async () => {
    snapshot = { exitCode: 1, exitUnexpected: false, exitTail: "bye" };
    render(<ChatWindow />);
    expect(await screen.findByText(zh.chat.composerGateEnded)).toBeTruthy();
    await new Promise((r) => setTimeout(r, 30));
    expect(screen.queryByText(zh.chat.agentCrashed(1))).toBeNull();
    expect(document.querySelector(".chat-compose-gate-tail")).toBeNull();
  });
});
