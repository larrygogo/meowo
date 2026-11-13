import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, adapter, emptyComposerFrame } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";

describe("跨Provider发送保护", () => {
  it.each(["codex", "kimi", "gemini", "opencode"])("%s：已知待批、题面未知时不写正文或Enter，原文可找回", async (provider) => {
    const base = adapter.setup(751, provider);
    invoke.mockImplementation((command, args) => command === "get_chat_history"
      ? Promise.resolve({ ...adapter.history(751, provider), pendingReview: "permission" })
      : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-751");
    fireEvent.change(adapter.box(), { target: { value: "不能误答审批" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await screen.findByText(zh.chat.terminalNeedsAttention);
    expect(adapter.writes(751)).toEqual([]);
    expect(adapter.box().value).toBe("不能误答审批");
  });

  // 审批已答、命令在跑时的过期 approval 标记由后端 pending_review_live 统一校正（四个消费点同源），
  // 见 session_query::a_settled_permission_request_stops_claiming_the_agent_is_waiting。前端只要保证：
  // 收到的标记仍在时一律拦住（屏幕挂着审批框，或屏幕状态只是回退猜测）。
  it.each([["blocked", false], ["working", true]] as const)("待批标记在、屏幕 %s（回退=%s）时仍拦住", async (screenState, assumed) => {
    const base = adapter.setup(762, "codex", "running", screenState);
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("codex"));
      if (command === "get_chat_history") return Promise.resolve({ ...adapter.history(762, "codex", "running", screenState), pendingReview: "approval", screenAssumed: assumed });
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-762");
    fireEvent.change(adapter.box(), { target: { value: "不能误答审批" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await screen.findByText(zh.chat.terminalNeedsAttention);
    expect(adapter.writes(762)).toEqual([]);
  });

  it("正文落下后从idle变running，落键时使用实时排队键", async () => {
    const base = adapter.setup(752, "codex");
    let running = false;
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve({ lines: ["› ", "", "  GPT-6.1-Sol default · C:/repo"], cursorRow: 0, cursorCol: 2, cursorVisible: true });
      if (command === "get_chat_history") return Promise.resolve(adapter.history(752, "codex", running ? "running" : "waiting"));
      if (command === "write_managed_terminal" && args?.data === "输入期间状态变化") running = true;
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-752");
    await waitFor(() => expect(document.querySelector(".chat-model-label")).toBeTruthy());
    fireEvent.change(adapter.box(), { target: { value: "输入期间状态变化" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await waitFor(() => expect(adapter.writes(752)).toContain("\t"));
    expect(adapter.writes(752)).not.toContain("\r");
  });

  it("正文落下后出现未识别审批，禁止提交键", async () => {
    const base = adapter.setup(753, "codex");
    let pending = false;
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve({ lines: ["› ", "", "  GPT-6.1-Sol default · C:/repo"], cursorRow: 0, cursorCol: 2, cursorVisible: true });
      if (command === "get_chat_history") return Promise.resolve({ ...adapter.history(753, "codex"), pendingReview: pending ? "permission" : null });
      if (command === "write_managed_terminal" && args?.data === "这条不能确认权限") pending = true;
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-753");
    fireEvent.change(adapter.box(), { target: { value: "这条不能确认权限" } });
    fireEvent.keyDown(adapter.box(), { key: "Enter" });
    await screen.findByText(zh.chat.terminalNeedsAttention);
    expect(adapter.writes(753)).toContain("这条不能确认权限");
    expect(adapter.writes(753)).not.toContain("\r");
  });
});
