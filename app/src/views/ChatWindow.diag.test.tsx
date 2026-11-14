// 对话窗运行日志（验收 19）：这些路径真的会写日志，且日志里没有消息正文。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, adapter, chatUiReady, emptyComposerFrame } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi } from "../test/agents";

const diagLines = () => invoke.mock.calls.filter(([command]) => command === "diag_log")
  .map(([, args]) => args as { scope: string; message: string });
const lineOf = (kind: string) => diagLines().find((entry) => entry.message.includes(`kind=${kind}`));

describe("对话窗运行日志", () => {
  const { snapshot, setup, box } = adapter;

  it.each(["claude", "codex", "kimi", "gemini", "opencode"])("%s 发送失败：写一行，带 Provider、会话 id、原因，不带正文", async (provider) => {
    const base = setup(801, provider);
    const body = "机密正文 secret-payload-801";
    let fail!: (reason: string) => void;
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame(provider));
      if (command === "write_managed_terminal" && args?.data === body) return new Promise<void>((_, reject) => { fail = reject; });
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-801");
    fireEvent.change(box(), { target: { value: body } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(fail).toBeTypeOf("function"));
    // 失败时输入框里已有新内容，留下「发送失败」气泡（用户看到的最终结果）。
    fireEvent.change(box(), { target: { value: "新草稿" } });
    await act(async () => { fail("写入终端失败"); });
    expect(await screen.findByText(zh.vocab.delivery.failed)).toBeTruthy();
    await waitFor(() => expect(lineOf("send_failed")).toBeTruthy());
    const entry = lineOf("send_failed")!;
    expect(entry.scope).toBe("chat");
    expect(entry.message).toContain(`provider=${provider}`);
    expect(entry.message).toContain("session=801");
    expect(entry.message).toContain("reason=terminal_write_failed");
    for (const line of diagLines()) {
      expect(line.message).not.toContain("机密正文");
      expect(line.message).not.toContain("secret-payload-801");
    }
  });

  it("未确认送达：回显等到超时，写一行 echo_timeout", async () => {
    window.history.replaceState({}, "", "/?sessionId=802");
    const history = {
      sessionId: 802, title: "恢复吞字", status: "ended", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let started = false;
    invoke.mockImplementation((command: string) => {
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      if (command === "managed_terminal_snapshot") {
        // 拉起后有画面但正文永不回显：未被识别的恢复确认选择器吞输入的形态。
        return Promise.resolve(started
          ? { sessionId: 802, active: true, managed: true, data: btoa("resume this session?"), startOffset: 0, endOffset: 20, exited: false, exitCode: null }
          : { sessionId: 802, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "go on please" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByText(zh.vocab.delivery.uncertain, undefined, { timeout: 8_000 })).toBeTruthy();
    await waitFor(() => expect(lineOf("send_unconfirmed")).toBeTruthy());
    const entry = lineOf("send_unconfirmed")!;
    expect(entry.message).toContain("provider=claude");
    expect(entry.message).toContain("session=802");
    expect(entry.message).toMatch(/reason=(echo_timeout|terminal_write_stalled)/);
    expect(entry.message).not.toContain("go on please");
  }, 15_000);

  it("切模型未确认：没等到 CLI 的确认行，写一行 no_confirm_line", async () => {
    const base = setup(803);
    invoke.mockImplementation((command, args) => command === "managed_terminal_snapshot"
      ? Promise.resolve(snapshot(803, "")) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-803");
    await chatUiReady();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /^Sonnet/ }));
    await screen.findByText(zh.chat.modelSwitchUnconfirmed, {}, { timeout: 4000 });
    const entry = lineOf("switch_unconfirmed")!;
    expect(entry.message).toBe("kind=switch_unconfirmed · provider=claude · session=803 · reason=no_confirm_line · what=model");
  });

  it("切模式未确认：TUI 对话框回车后底栏没认出新值，写一行 tui_value_not_seen", async () => {
    const base = setup(804, "kimi");
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      if (command === "apply_session_settings") return Promise.resolve({ kind: "applied", sessionId: 804, confirmed: false, reason: null });
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "kimi-code/k3", label: "K3", group: "managed:kimi-code", efforts: [], default_effort: null, is_default: false, uncertain: false },
      ]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-804");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "K3" }));
    await screen.findByText(zh.chat.settingsUnconfirmed);
    expect(lineOf("switch_unconfirmed")!.message)
      .toBe("kind=switch_unconfirmed · provider=kimi · session=804 · reason=tui_value_not_seen · what=model");
  });

  it("切换被后端拒绝：记后端错误码，不记本地化后的句子", async () => {
    const base = setup(805, "codex");
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("codex"));
      if (command === "apply_session_settings") return Promise.reject("settings/model_unknown");
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "gpt-6.1-sol", label: "GPT-6.1-Sol", group: null, efforts: [], default_effort: null, is_default: false, uncertain: false },
      ]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-805");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "GPT-6.1-Sol" }));
    await waitFor(() => expect(lineOf("switch_failed")).toBeTruthy());
    expect(lineOf("switch_failed")!.message)
      .toBe("kind=switch_failed · provider=codex · session=805 · reason=settings/model_unknown · what=model");
  });
});
