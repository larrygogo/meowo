import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// 「Meowo 托管」会话路线已删除（leader 2026-11-13：对话依托官方 CLI）。旧库里留下的这类会话打开时只说
// 「这种会话已停用」；绑定过原生会话 id 的给「用官方 CLI 打开」，走的是原生恢复（start_managed_terminal），
// 没绑定过的只留说明，输入框锁住——托管通道已删，发不出去。
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

const SESSION = 93;
let history: Record<string, unknown> = {};

beforeEach(() => {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
  });
  window.history.replaceState({}, "", `/?sessionId=${SESSION}`);
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({
      sessionId: SESSION, provider: "codex", status: "ended", title: "旧托管会话", cwd: "C:/repo", connected: false,
      supported: true, offset: 0, reset: false, pendingReview: null, ptyManaged: false, items: [], ...history,
    });
    if (command === "managed_terminal_snapshot") return Promise.resolve({
      sessionId: SESSION, active: false, managed: false, data: "", startOffset: 0, endOffset: 0,
      exited: false, exitCode: null, exitTail: null, cols: 0, rows: 0, modes: [], externalViewers: false, exitUnexpected: false,
    });
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "shell_terminal_list" || command === "browser_pane_list") return Promise.resolve([]);
    if (command === "start_managed_terminal") return Promise.reject(new Error("stop here"));
    return Promise.resolve();
  });
});
afterEach(() => {
  cleanup();
  localStorage.clear();
  invoke.mockReset();
  history = {};
});

describe("已停用的「Meowo 托管」会话", () => {
  it("有原生会话 id：说明已停用，并给「用官方 CLI 打开」走原生恢复", async () => {
    history = { retiredManaged: { resumable: true } };
    render(<ChatWindow />);
    expect(await screen.findByText(zh.chat.managedRetired)).toBeTruthy();
    expect(screen.queryByText(zh.chat.composerGateEnded)).toBeNull();
    expect((document.querySelector(".chat-compose textarea") as HTMLTextAreaElement).placeholder).toBe(zh.chat.managedRetiredInputResumable);
    const open = screen.getByRole("button", { name: zh.chat.managedRetiredOpenCli });
    // 正常主操作用中性按钮，不用接管那种红描边。
    expect(open.className).toBe("sbtn");
    fireEvent.click(open);
    await waitFor(() => expect(invoke.mock.calls.some(([command, args]) =>
      command === "start_managed_terminal" && (args as { sessionId: number }).sessionId === SESSION)).toBe(true));
  });

  it("没绑定过原生 id（读不到原生记录）：只显示说明，没有可点的恢复入口，输入框锁住", async () => {
    history = { retiredManaged: { resumable: false }, supported: false };
    render(<ChatWindow />);
    expect(await screen.findByText(zh.chat.managedRetired)).toBeTruthy();
    expect(screen.queryByRole("button", { name: zh.chat.managedRetiredOpenCli })).toBeNull();
    expect(screen.queryByRole("button", { name: zh.chat.resumeSession })).toBeNull();
    const input = document.querySelector(".chat-compose textarea") as HTMLTextAreaElement;
    expect(input.disabled).toBe(true);
    expect(input.placeholder).toBe(zh.chat.managedRetiredInputUnbound);
    expect((document.querySelector(".chat-compose .chat-attach-button") as HTMLButtonElement).disabled).toBe(true);
    expect((document.querySelector(".chat-compose .chat-send-button") as HTMLButtonElement).disabled).toBe(true);
    expect(invoke.mock.calls.some(([command]) => command === "start_managed_terminal")).toBe(false);
  });

  // 做空 R4：「用官方 CLI 打开」之后它就是普通原生会话。后端在原生 PTY 活着时不再下发停用标记；
  // 前端也只在门卡还挂着时用停用文案，两层都要守住——任何一层漏掉，+ 菜单就永久置灰、占位还说「已停用」。
  it.each([
    ["后端已撤掉停用标记", { retiredManaged: null }],
    ["停用标记还残留（旧后端/轮询竞态）", { retiredManaged: { resumable: true } }],
  ])("用官方 CLI 恢复后（%s）：占位与 + 菜单恢复正常", async (_label, extra) => {
    history = { status: "running", connected: true, ptyManaged: true, ...extra };
    render(<ChatWindow />);
    await waitFor(() => expect(document.querySelector(".chat-compose textarea")).toBeTruthy());
    const input = document.querySelector(".chat-compose textarea") as HTMLTextAreaElement;
    await waitFor(() => expect(input.placeholder).not.toBe(""));
    expect(screen.queryByText(zh.chat.managedRetired)).toBeNull();
    expect(input.disabled).toBe(false);
    expect([zh.chat.managedRetiredInputResumable, zh.chat.managedRetiredInputUnbound]).not.toContain(input.placeholder);
    expect((document.querySelector(".chat-compose .chat-attach-button") as HTMLButtonElement).disabled).toBe(false);
  });
});
