// 提问卡的第二识别源（chat/useQuestionFormProbe）：xterm 可视区认不出终端表单时，读后端按 PTY
// 尺寸重建的屏幕认表单；后端屏幕上也没有时照实说明并给出路。夹具见 chatWindow.harness.tsx。
//
// 兄弟场景：桌面对话页、桌面终端页（展示卡默认收成提示条，识别结果同一个 terminalAttention）、
// 手机远程页（没有终端页，认出后作答卡直接出现在手机上；认不出时卡内给终端画面与按键）。
import { afterEach, describe, expect, it } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { invoke, eventListeners, terminalProps } from "./chatWindow.harness";
import { terminalAttention } from "../terminalAttention";
import { ChatWindow } from "./ChatWindow";
import { chatUi } from "../test/agents";

/// 11-03 10:15 实拍（claude 2.1.317，单题三选项带描述）的终端表单形态，正文脱敏。
/// 那次 broker 挂起满 300s 放行后，题面卡退成展示卡，xterm 网格错位，10 分钟没认出表单。
const FORM_1103 = [
  "────────────────────────────────────────────────",
  " ☐ 用法",
  "",
  "这个配置你指的是哪种用法？",
  "",
  "❯ 1. 区分两类",
  "     录入时选类型，下拉里带标签",
  "  2. 建任务时自动套用",
  "     新建任务时表单自动填好",
  "  3. 一条配置用于多个任务",
  "     同一条配置挂到几个任务上",
  "  4. Type something.",
  "────────────────────────────────────────────────",
  "  5. Chat about this",
  "",
  "Enter to select · ↑/↓ to navigate · Esc to cancel",
];

/// 11-04 09:19:29 诊断日志里记下的屏幕末尾形态：题面卡在场，终端停在空闲输入框，没有表单。
const IDLE_SCREEN = [
  "✻ Crunched for 14s · done 17:18",
  "────────────────────────────────────────────────",
  "❯ ",
  "────────────────────────────────────────────────",
  "  Opus 5.5 · 47% ctx",
  "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent",
];

const QUESTION = {
  questions: [{
    header: "用法", question: "这个配置你指的是哪种用法？", multiSelect: false,
    options: [
      { label: "区分两类", description: "录入时选类型，下拉里带标签" },
      { label: "建任务时自动套用", description: "新建任务时表单自动填好" },
      { label: "一条配置用于多个任务", description: "同一条配置挂到几个任务上" },
    ],
  }],
};

function mount(sessionId: number, screenLines: () => string[] | null, xtermActive = false) {
  window.history.replaceState({}, "", `/?sessionId=${sessionId}`);
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({
      sessionId, title: "提问识别", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      ptyManaged: true,
    });
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "managed_terminal_binding") return Promise.resolve(null);
    // xterm 那条识别源什么都看不见：快照没有任何字节（对应网格错位、可视区不在底部、未挂载）。
    if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId, active: xtermActive, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
    if (command === "managed_terminal_screen") return Promise.resolve(screenLines());
    // 插件声明的选择器锚点（Type something / Chat about this）随 chatUi 下发，识别靠它认出纯编号表单。
    if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
    return Promise.resolve();
  });
  render(<ChatWindow />);
}

function ask(sessionId: number) {
  act(() => {
    eventListeners.get("interactive-question")?.({ payload: {
      sessionId, requestId: `request-${sessionId}`, provider: "claude", toolName: "AskUserQuestion",
      description: null, answerable: false, input: JSON.stringify(QUESTION), permissionSuggestions: [],
    } });
  });
}

const writes = () => invoke.mock.calls.filter((call) => call[0] === "write_managed_terminal");

afterEach(() => { delete (globalThis as Record<string, unknown>).__MEOWO_REMOTE__; });

describe("提问卡：后端屏幕识别终端表单", () => {
  it("11-03：xterm 认不出时，后端屏幕上的表单接管成作答卡，点选直接落键", async () => {
    mount(51, () => FORM_1103);
    await waitFor(() => expect(screen.getByText("提问识别")).toBeTruthy());
    ask(51);
    // 后端屏幕认出表单：作答卡出现，选项来自终端表单（含 Type something / Chat about this）。
    // 「Chat about this」只在终端表单里有，题面展示卡没有：它出现说明作答卡已接管。
    expect(await screen.findByRole("button", { name: "先和 Agent 讨论" }, { timeout: 3_000 })).toBeTruthy();
    expect(screen.queryByText(/表单就绪后自动作答/)).toBeNull();
    const option = screen.getByRole("button", { name: /建任务时自动套用/ });
    const before = writes().length;
    act(() => { option.click(); });
    // ❯ 停在第 1 项：第 2 项下移一格回车。
    await waitFor(() => expect(writes().length).toBeGreaterThan(before));
    expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 51, data: "\x1b[B\r" });
  });

  it("手机远程：后端屏幕认出表单，作答卡直接出现在手机上", async () => {
    (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = true;
    mount(52, () => FORM_1103);
    await waitFor(() => expect(screen.getByText("提问识别")).toBeTruthy());
    ask(52);
    expect(await screen.findByRole("button", { name: "先和 Agent 讨论" }, { timeout: 3_000 })).toBeTruthy();
    expect(screen.getByRole("button", { name: /一条配置用于多个任务/ })).toBeTruthy();
  });

  it("11-04：后端屏幕上也没有表单，8 秒后照实说明，手机上卡内给终端画面与按键", async () => {
    (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = true;
    mount(53, () => IDLE_SCREEN);
    await waitFor(() => expect(screen.getByText("提问识别")).toBeTruthy());
    ask(53);
    expect(await screen.findByText(/这个配置你指的是哪种用法/)).toBeTruthy();
    expect(screen.queryByText(/没认出终端里的表单/)).toBeNull();
    expect(await screen.findByText("没认出终端里的表单，可用下方按键作答", undefined, { timeout: 12_000 })).toBeTruthy();
    // 终端画面与按键就在卡里（RemoteTerminalKeys），手机上不是死路。
    expect(screen.getByLabelText("终端画面")).toBeTruthy();
  }, 20_000);

  /// xterm 那条识别源由输出事件驱动、总在 CLI 重画之后扫；它先认出来时，轮询读到的可能是
  /// 重画之前的一帧，不能拿来覆盖焦点（多选连点时焦点会被拨回，落键落到别的项上）。
  it("xterm 先认出表单时探针让位，不用轮询的旧帧覆盖焦点", async () => {
    mount(55, () => FORM_1103, true);
    await waitFor(() => expect(screen.getByText("提问识别")).toBeTruthy());
    await waitFor(() => expect(terminalProps.current?.onAttention).toBeTruthy());
    ask(55);
    // xterm 看到的是焦点已移到第 2 项的新画面。
    const moved = FORM_1103.map((line) => line.replace("❯ 1.", "  1.").replace("  2. 建任务", "❯ 2. 建任务"));
    const fresh = terminalAttention(moved.join("\n"), [], true, false, { provider: "claude", selectorAnchors: chatUi("claude")?.selector_anchors ?? [], attentionPatterns: [] });
    expect(fresh?.options?.[1].focused).toBe(true);
    act(() => { (terminalProps.current?.onAttention as (a: unknown) => void)(fresh); });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 2_000)); });
    act(() => { screen.getByRole("button", { name: /建任务时自动套用/ }).click(); });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 55, data: "\r" }));
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 55, data: "\x1b[B\r" });
  });

  it("桌面：后端屏幕上也没有表单，提示改为说明原因，「去终端页」仍在", async () => {
    mount(54, () => IDLE_SCREEN);
    await waitFor(() => expect(screen.getByText("提问识别")).toBeTruthy());
    ask(54);
    expect(await screen.findByText("没认出终端里的表单，可在终端页直接选", undefined, { timeout: 12_000 })).toBeTruthy();
    expect(screen.getByRole("button", { name: "去终端页" })).toBeTruthy();
  }, 20_000);
});
