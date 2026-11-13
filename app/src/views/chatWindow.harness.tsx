// ChatWindow 系列测试的共享夹具：原 ChatWindow.test.tsx（6178 行、195 例，单文件跑 88s）
// 拆成 7 个文件后靠 vitest 按文件并行把全量墙钟从 94s 压下来（2026-11-10 实测）。
// vi.mock 写在这里也生效：测试文件先 import 本文件，mock 先注册，再 import ./ChatWindow。
// 所以每个测试文件里 import 本文件必须排在 import ChatWindow 之前。
// vi.hoisted 的变量不能直接 export（vitest 报 Cannot export hoisted variable），统一在文件末尾导出。
import { afterEach, expect, vi } from "vitest";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { chatUi } from "../test/agents";
import type { TerminalScreenFrameDto } from "../api";
import { outgoingStore } from "./chat/outgoingPrompts";

const invoke = vi.hoisted(() => vi.fn());
const openDialog = vi.hoisted(() => vi.fn());
const saveDialog = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog, save: saveDialog }));
// 捕获事件回调：审批类用例需要手动投递 pending-approval，验证「别的会话要授权时不切窗」。
const eventListeners = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, cb: (event: { payload: unknown }) => void) => {
    eventListeners.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));
const setTitleMock = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: vi.fn(() => Promise.resolve()), setTitle: setTitleMock, onDragDropEvent: () => Promise.resolve(() => {}) }),
}));
// 记录挂载次数：切 tab 不应重建终端（重建=dispose+new Terminal+全量 backlog 重放）。
const terminalMounts = vi.hoisted(() => ({ count: 0 }));
// 真实组件的屏幕识别在这里跑不动（没有 xterm）。把它的回调与入参暴露出来，测试就能
// 用**真实解析器**产出的 attention 驱动 ChatWindow，验证渲染与按键下发这一段。
const terminalProps = vi.hoisted(() => ({ current: null as null | Record<string, unknown> }));
vi.mock("./ManagedTerminal", async () => {
  const { useEffect } = await import("react");
  return {
    ManagedTerminal: (props: { sessionId: number }) => {
      terminalProps.current = props as unknown as Record<string, unknown>;
      useEffect(() => { terminalMounts.count += 1; }, []);
      return <div>PTY {props.sessionId}</div>;
    },
  };
});

/// UTF-8 → base64。btoa 只吃 latin1,含中文的终端回显必须先转字节,否则直接抛
/// InvalidCharacterError(生产端 decodeBase64 按 UTF-8 字节解码,两头对齐)。
/// 选附件：agent 有命令或支持提及时「+」是菜单（对照 Kimi 的 addMenu），从「文件」一项进；
/// 否则「+」一点就是选文件。
export function pickAttachments() {
  const menu = screen.queryByRole("button", { name: "添加" });
  if (menu) {
    fireEvent.click(menu);
    fireEvent.click(screen.getByRole("menuitem", { name: "文件" }));
  } else {
    fireEvent.click(screen.getByRole("button", { name: "添加图片或文件" }));
  }
}

export function b64utf8(text: string): string {
  return btoa(String.fromCharCode(...new TextEncoder().encode(text)));
}

export function respondWithHistory(history: unknown, approval: unknown = null) {
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    // 增量语义必须模拟：组件把每次轮询返回的 items **追加**进 transcript，真实后端
    // 只回 offset 之后的新事件。无视 offset 每轮都回整批，慢机上测试跨过 650ms
    // 轮询间隔时同一批会被追加两遍——Windows CI 上「找到多个元素」的抖动即源于此。
    if (command === "get_chat_history") {
      // connected 是 DTO 必填字段(存活校正的数据源):老夹具没写它时补 true,
      // 避免 mock 形状与真实后端分叉、被 `?? false` 静默降级成离线语义。
      const h = { connected: true, ...(history as { offset?: number; items?: unknown[]; connected?: boolean }) };
      const cursor = (args?.offset as number) ?? 0;
      // cursor=0 是首读/全量重读，恒回整批；追平后的轮询才回空。
      if (cursor > 0 && cursor >= (h.offset ?? 0)) {
        return Promise.resolve({ ...h, items: [], hasMore: false });
      }
      return Promise.resolve(h);
    }
    if (command === "pending_interaction") return Promise.resolve({ approval, question: null });
    if (command === "agent_chat_ui") return Promise.resolve(chatUi((history as { provider: string }).provider));
    if (command === "managed_terminal_binding") return Promise.resolve(null);
    if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 1, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
    return Promise.resolve();
  });
}

/// 等对话页能力（agent_chat_ui）落进组件再操作。运行中发送的排队键、模型菜单的预设都取自它，
/// 没到就按回车会报「运行中发不出」、点模型菜单是空的。原单文件里这几条排在中段，后半程
/// 整份文件独占机器才侥幸赶上；拆开后各文件并发跑就会抢在它返回前动手（2026-11-10 实测：
/// 「运行中插话显示排队回执…」在原文件里用 -t 单跑也稳定失败）。mock 立即 resolve，
/// 看到调用后再冲一次 act 就一定已提交。
export async function chatUiReady() {
  await waitFor(() => expect(invoke.mock.calls.some(([command]) => command === "agent_chat_ui")).toBe(true));
  await act(async () => {});
}

afterEach(() => {
  cleanup();
  invoke.mockReset();
  openDialog.mockReset();
  eventListeners.clear();
  // 发出消息的仓库是模块级、按会话分仓的：几个用例共用会话号，不清会串。
  outgoingStore.reset();
  window.history.replaceState({}, "", "/");
  // 侧栏折叠状态持久化在 localStorage，不清会串到下一个用例。
  localStorage.clear();
});

// 「ChatWindow 适配回归」两份文件共用的会话夹具（原 describe 头部的五个辅助，原样搬出）。
const adapterHelpers = () => {
  const history = (sessionId: number, provider = "claude", status = "waiting", screenState: string | null = null) => ({
    sessionId, provider, status, screenState, title: `适配-${sessionId}`, cwd: "C:/repo", connected: true,
    supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "当前模型",
  });
  const snapshot = (sessionId: number, text = "") => ({
    sessionId, active: true, managed: true, data: b64utf8(text), startOffset: 0,
    endOffset: text.length, exited: false, exitCode: null,
  });
  const setup = (sessionId: number, provider = "claude", status = "waiting", screenState: string | null = null) => {
    window.history.replaceState({}, "", `/?sessionId=${sessionId}`);
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve(history(Number(args?.sessionId), provider, status, screenState));
      if (command === "agent_chat_ui") return Promise.resolve(chatUi(provider));
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(Number(args?.sessionId)));
      return Promise.resolve();
    });
    return invoke.getMockImplementation()!;
  };
  const box = () => screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
  const writes = (sessionId: number) => invoke.mock.calls
    .filter(([command, args]) => command === "write_managed_terminal" && args.sessionId === sessionId)
    .map(([, args]) => args.data);
  return { history, snapshot, setup, box, writes };
};
export const adapter = adapterHelpers();

export { invoke, openDialog, saveDialog, eventListeners, setTitleMock, terminalMounts, terminalProps };

// 2026-11-12 真实空框裁图，光标坐标随裁图平移；不替未知 API 默认返回“安全空框”。
const NATIVE_EMPTY_FRAMES: Record<string, TerminalScreenFrameDto> = {
  "kimi": {
    "lines": [
      " ╭────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮",
      " │ >                                                                                                                  │",
      " ╰────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯"
    ],
    "cursorRow": 1,
    "cursorCol": 5,
    "cursorVisible": true
  },
  "opencode": {
    "lines": [
      "                       ┃                                                                                                ",
      "                       ┃  Ask anything… \"Fix a TODO in the codebase\"                                                    ",
      "                       ┃                                                                                                ",
      "                       ┃  Build · Claude Sonnet 4.6 GitHub Copilot                                                      ",
      "                       ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀                      "
    ],
    "cursorRow": 1,
    "cursorCol": 26,
    "cursorVisible": true
  },
  "codex": {
    "lines": [
      "› Ask Codex to do anything",
      "",
      "  GPT-6.1-Sol default · ~\\Desktop\\workspace\\meowo\\target\\provider-compat-lead\\target\\lead-provider-probes\\composer\\runs…",
      "  ← for agents · ? for shortcuts"
    ],
    "cursorRow": 0,
    "cursorCol": 2,
    "cursorVisible": true
  },
  "gemini": {
    "lines": [
      "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
      " >   Type your message or @path/to/file",
      "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────"
    ],
    "cursorRow": 1,
    "cursorCol": 3,
    "cursorVisible": true
  }
};
export function emptyComposerFrame(provider: string): TerminalScreenFrameDto | null {
  const frame = NATIVE_EMPTY_FRAMES[provider];
  return frame ? { ...frame, lines: [...frame.lines] } : null;
}
