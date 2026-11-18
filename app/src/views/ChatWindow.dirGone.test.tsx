import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";

// 目录已删除的会话（worktree 合并后被清理、目录被手动删了；2026-11-18 手机实拍）：打开照常看历史，
// 输入区换成「目录已删掉，不能继续」与「在原项目新开会话」，不给「恢复会话」——agent 按目录找会话记录，
// 换个目录续不上，原目录又没了。修复前这里是「会话已结束」+「恢复会话」，点了只得到一句启动失败。
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

const SESSION = 94;
let history: Record<string, unknown> = {};

beforeEach(() => {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
  });
  window.history.replaceState({}, "", `/?sessionId=${SESSION}`);
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({
      sessionId: SESSION, provider: "codex", status: "ended", title: "修好的支付回调", cwd: "C:/w/billing-fix", connected: false,
      supported: true, offset: 0, reset: false, pendingReview: null, ptyManaged: false, items: [], ...history,
    });
    if (command === "managed_terminal_snapshot") return Promise.resolve({
      sessionId: SESSION, active: false, managed: false, data: "", startOffset: 0, endOffset: 0,
      exited: false, exitCode: null, exitTail: null, cols: 0, rows: 0, modes: [], externalViewers: false, exitUnexpected: false,
    });
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "shell_terminal_list" || command === "browser_pane_list") return Promise.resolve([]);
    if (command === "start_managed_terminal") return Promise.reject(new Error("stop here"));
    if (command === "open_new_session_window") return Promise.resolve();
    // 目录还在时这几条会把改动数胶囊、「更改」入口、会话信息栏点亮；目录已删的会话一条都不该发。
    if (command === "git_diff_summary") return Promise.resolve({ isRepo: true, branch: "feat/x", files: [] });
    return Promise.resolve();
  });
});
afterEach(() => {
  cleanup();
  localStorage.clear();
  invoke.mockReset();
  history = {};
});

describe("目录已删除的会话", () => {
  it("推得出原项目：只读说明 + 「在原项目新开会话」，没有恢复入口，也没有输入框", async () => {
    history = { cwdMissing: true, originRoot: "C:/w/billing", archived: true };
    render(<ChatWindow />);
    expect(await screen.findByText(zh.chat.dirGoneBanner)).toBeTruthy();
    expect(screen.queryByText(zh.chat.composerGateEnded)).toBeNull();
    expect(screen.queryByRole("button", { name: zh.chat.resumeSession })).toBeNull();
    expect(document.querySelector(".chat-compose textarea")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: zh.chat.dirGoneNewIn("billing") }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_new_session_window", { cwd: "C:/w/billing", provider: "codex" }));
    // 不在别的目录偷偷恢复：整个过程没有发起过任何启动。
    expect(invoke.mock.calls.some(([command]) => command === "start_managed_terminal" || command === "takeover_managed_terminal")).toBe(false);
  });

  it("门卡就是整块输入区：不留输入框、+、权限、模型、上下文圈和发送键", async () => {
    history = { cwdMissing: true, originRoot: "C:/w/billing", contextPct: 18, model: "Opus 5" };
    render(<ChatWindow />);
    await screen.findByTestId("compose-dir-gone");
    const compose = document.querySelector(".chat-compose") as HTMLElement;
    expect(compose.classList.contains("is-dir-gone")).toBe(true);
    for (const sel of ["textarea", ".chat-attach-button", ".chat-send-button", ".chat-mode-button", ".chat-model-select"]) {
      expect(compose.querySelector(sel), sel).toBeNull();
    }
    // 门卡里只有说明和那一颗按钮。
    expect(compose.querySelectorAll("button").length).toBe(1);
  });

  it("不读已经不在的目录：不查改动数、标题栏没有改动胶囊和面板开关", async () => {
    history = { cwdMissing: true };
    render(<ChatWindow />);
    await screen.findByTestId("compose-dir-gone");
    await new Promise((resolve) => setTimeout(resolve, 50));
    for (const command of ["git_diff_summary", "git_changes_overview", "list_folder_editors"]) {
      expect(invoke.mock.calls.some(([c, args]) => c === command && JSON.stringify(args ?? {}).includes("billing-fix")), command).toBe(false);
    }
    expect(document.querySelector(".chat-panel-toggle")).toBeNull();
  });

  it("不当成在跑：库里残留 running、transcript 说有后台子任务，也没有运行条", async () => {
    history = { cwdMissing: true, status: "running", busySubagents: 2 };
    render(<ChatWindow />);
    await screen.findByTestId("compose-dir-gone");
    expect(document.querySelector(".chat-running")).toBeNull();
  });

  it("推不出原项目：给普通的「新建会话」，不拿已经没了的目录去预填", async () => {
    history = { cwdMissing: true };
    render(<ChatWindow />);
    // 侧栏头也有一颗同名的「新建会话」，限定在门卡里找。
    const gate = await screen.findByTestId("compose-dir-gone");
    fireEvent.click(within(gate).getByRole("button", { name: zh.chat.dirGoneNew }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_new_session_window", { cwd: null, provider: "codex" }));
  });

  it("标题菜单：已完成的这种会话「恢复进行中」置灰并说明原因", async () => {
    history = { cwdMissing: true, archived: true };
    render(<ChatWindow />);
    await screen.findByText(zh.chat.dirGoneBanner);
    fireEvent.click(document.querySelector(".chat-title-menu button") as HTMLButtonElement);
    const reopen = await screen.findByRole("menuitem", { name: new RegExp(zh.sticker.unarchive) });
    expect((reopen as HTMLButtonElement).disabled).toBe(true);
    expect(reopen.getAttribute("title")).toBe(zh.chat.dirGoneNoReopen);
  });

  it("进程还连着（目录被删时进程仍在）：照常可写，不显示只读说明", async () => {
    history = { cwdMissing: true, status: "running", connected: true, ptyManaged: true };
    render(<ChatWindow />);
    await waitFor(() => expect(document.querySelector(".chat-compose textarea")).toBeTruthy());
    expect(screen.queryByText(zh.chat.dirGoneBanner)).toBeNull();
    expect((document.querySelector(".chat-compose textarea") as HTMLTextAreaElement).disabled).toBe(false);
  });
});
