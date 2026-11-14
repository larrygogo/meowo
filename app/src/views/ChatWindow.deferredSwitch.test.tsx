// 回合进行中选的模型 / 思考档 / 模式：排到这一轮之后，回合结束时**真的执行**。
//
// 走查输入 A1：Claude 会话在回合进行中选模型或思考档，界面说「这一轮结束后切换到 X」，回合结束后却
// 什么都没切——执行前要等「终端输入框认得出」，而 Claude 当时没有声明输入框规格，空等 10 秒后报
// 「认不出终端输入框，为免丢字没有重启」并丢掉这次选择。旧测试只断言到「排进队列」为止，夹具又默认
// 「输入框是空的」，所以一直没测出来。这里每一家都断言到底：CLI（或替它生效的后端命令）收到了切换。
//
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emptyComposerFrame, invoke, adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import composerFrames from "../test/fixtures/composer-frames.json";

const { history, setup, writes } = adapter;

type Frame = { name: string; lines: string[]; cursor?: { row: number; col: number; visible: boolean; dim: boolean } };
/// 共享样例里的实拍帧（与后端、读框单测同一份），转成帧接口的形状。
function liveFrame(name: string) {
  const frame = (composerFrames.frames as Frame[]).find((entry) => entry.name === name)!;
  return {
    lines: frame.lines,
    cursorRow: frame.cursor?.row ?? 0,
    cursorCol: frame.cursor?.col ?? 0,
    cursorVisible: frame.cursor?.visible ?? false,
    cursorDim: frame.cursor?.dim ?? false,
  };
}

/// 会话先在跑，`finish()` 之后转空闲；宿主的切换队列按真实语义模拟（排进去、取走即清）。
function runningSession(sessionId: number, provider: string, extra: (command: string, args?: Record<string, unknown>) => unknown = () => undefined, historyExtra: Record<string, unknown> = {}) {
  const base = setup(sessionId, provider, "running");
  let status = "running";
  let queued: unknown = null;
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    const custom = extra(command, args);
    if (custom !== undefined) return custom;
    if (command === "get_chat_history") return Promise.resolve({ ...history(sessionId, provider, status), ...historyExtra });
    if (command === "queue_session_settings") { queued = args?.request; return Promise.resolve(queued); }
    if (command === "queued_session_settings") return Promise.resolve(queued);
    if (command === "take_queued_session_settings") { const taken = queued; queued = null; return Promise.resolve(taken); }
    return base(command, args);
  });
  return { finish: () => { status = "waiting"; } };
}

describe("回合进行中选的切换，回合结束后真的执行", () => {
  it("claude：模型排队后在回合结束时写入 /model 并回车", async () => {
    const session = runningSession(701, "claude", (command) => command === "managed_terminal_frame"
      ? Promise.resolve(liveFrame("claude-2.1.325-placeholder-cursor")) : undefined);
    render(<ChatWindow />);
    await screen.findByText("适配-701");
    await chatUiReady();
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Sonnet/ }));
    await screen.findByText(/^这一轮结束后切换到「Sonnet/);
    // 回合还在跑：一个字都不往终端写。
    expect(writes(701)).toEqual([]);
    session.finish();
    await waitFor(() => expect(writes(701)).toContain("/model sonnet"), { timeout: 8000 });
    await waitFor(() => expect(writes(701).at(-1)).toBe("\r"), { timeout: 4000 });
    // 空输入框上不按暂存键（那是切换语义），也没有报「认不出终端输入框」。
    expect(writes(701)).not.toContain("\x13");
    expect(screen.queryByText(zh.chat.settingsComposerUnknown)).toBeNull();
    expect(screen.queryByText(zh.chat.settingsTerminalNotReady)).toBeNull();
    await waitFor(() => expect(screen.queryByText(/^这一轮结束后切换到/)).toBeNull());
  }, 15_000);

  it("claude：思考档排队后在回合结束时写入 /effort", async () => {
    const session = runningSession(702, "claude", (command) => command === "managed_terminal_frame"
      ? Promise.resolve(liveFrame("claude-2.1.325-placeholder-cursor")) : undefined, { effort: "low" });
    render(<ChatWindow />);
    await screen.findByText("适配-702");
    await chatUiReady();
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("radio", { name: "High" }));
    await screen.findByText(/^这一轮结束后切换到/);
    expect(writes(702)).toEqual([]);
    session.finish();
    await waitFor(() => expect(writes(702)).toContain("/effort high"), { timeout: 8000 });
  }, 15_000);

  it("claude：回合结束时终端输入框里有草稿，先暂存再写入切换命令", async () => {
    let stashed = false;
    const session = runningSession(703, "claude", (command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\x13") stashed = true;
      if (command === "managed_terminal_frame") return Promise.resolve(liveFrame(stashed ? "claude-2.1.325-stashed" : "claude-2.1.325-draft-multiline"));
      return undefined;
    });
    render(<ChatWindow />);
    await screen.findByText("适配-703");
    await chatUiReady();
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Sonnet/ }));
    await screen.findByText(/^这一轮结束后切换到「Sonnet/);
    session.finish();
    await waitFor(() => expect(writes(703)).toContain("/model sonnet"), { timeout: 8000 });
    const sent = writes(703);
    expect(sent.indexOf("\x13")).toBeGreaterThanOrEqual(0);
    expect(sent.indexOf("\x13")).toBeLessThan(sent.indexOf("/model sonnet"));
  }, 15_000);

  // 另外三种机制（会话服务、带参重启接回、TUI 对话框）由后端执行：前端在回合结束时把这次选择交给
  // apply_session_settings（它替 CLI 生效，后端另有各机制的测试）。断言到这条命令带着所选模型发出。
  it.each([
    ["codex", "server_settings"],
    ["kimi", "restart_resume"],
    ["gemini", "restart_resume"],
    ["opencode", "tui_dialog"],
  ])("%s（%s）：模型排队后在回合结束时交给后端执行", async (provider, _mechanism) => {
    const sessionId = 710 + ["codex", "kimi", "gemini", "opencode"].indexOf(provider);
    const session = runningSession(sessionId, provider, (command) => {
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "vendor/model-a", label: "当前模型", group: "g", efforts: [] },
        { id: "vendor/model-b", label: "另一个模型", group: "g", efforts: [] },
      ]);
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame(provider));
      if (command === "apply_session_settings") return Promise.resolve({ kind: "applied", confirmed: true });
      return undefined;
    });
    render(<ChatWindow />);
    await screen.findByText(`适配-${sessionId}`);
    await chatUiReady();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /另一个模型/ }));
    await screen.findByText(/^这一轮结束后切换到「另一个模型/);
    const applied = () => invoke.mock.calls.filter(([command]) => command === "apply_session_settings");
    expect(applied()).toEqual([]);
    session.finish();
    await waitFor(() => expect(applied().length).toBe(1), { timeout: 8000 });
    expect(JSON.stringify(applied()[0][1])).toContain("vendor/model-b");
    expect(JSON.stringify(applied()[0][1])).toContain(String(sessionId));
    await waitFor(() => expect(screen.queryByText(/^这一轮结束后切换到/)).toBeNull());
  }, 15_000);
});
