import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatUi } from "../../api";

const api = vi.hoisted(() => ({
  managedTerminalScreen: vi.fn<(sessionId: number) => Promise<string[] | null>>(),
  writeManagedTerminal: vi.fn<(sessionId: number, data: string) => Promise<void>>(),
}));
vi.mock("../../api", () => api);

import { anchorFrom, outgoingStore, type OutgoingPrompt } from "./outgoingPrompts";
import { useOutgoingUpkeep } from "./useOutgoingUpkeep";

const claudeUi = {
  draft_stash: { input: "\x13", composer_prompt: "❯", composer_border: "─", stashed_marker: "› stashed" },
} as unknown as ChatUi;

const echo = (id: number, text: string): OutgoingPrompt => ({
  id, kind: "echo", text, attachmentOnly: false, at: 0, phase: "uncertain", settledAt: null, anchor: anchorFrom([], null),
});

function mount(outgoing: OutgoingPrompt[], chatUi: ChatUi | null = claudeUi) {
  return renderHook(() => useOutgoingUpkeep({
    sessionId: 7, outgoing, loading: false, items: [], acks: [], lastUserText: null, ended: false,
    chatUi, isActive: () => true, running: () => false, onError: () => {},
  }));
}

afterEach(() => { vi.clearAllMocks(); outgoingStore.reset(); });

describe("useOutgoingUpkeep：未确认送达时读终端画面", () => {
  it("这段话还停在 CLI 输入框里就标卡住，提交按一次回车并转回已写入", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 上一轮的回复", "─────────", "❯ 帮我看看这个报错", "─────────"]);
    api.writeManagedTerminal.mockResolvedValue();
    const id = outgoingStore.add(7, { kind: "echo", text: "帮我看看这个报错", attachmentOnly: false, anchor: anchorFrom([], null), phase: "uncertain" });
    const { result } = mount([echo(id, "帮我看看这个报错")]);
    await waitFor(() => expect(result.current.stuck.has(id)).toBe(true));
    act(() => result.current.submit(id));
    await waitFor(() => expect(outgoingStore.get(7)[0].phase).toBe("pending"));
    expect(api.writeManagedTerminal).toHaveBeenCalledWith(7, "\r");
    expect(result.current.stuck.has(id)).toBe(false);
  });

  /// 做空第一轮 #5（2026-11-12）：窗口看不见这句时对账不判未确认，回显停在 pending；超时后照样读一次
  /// 画面，卡在输入框里就转未确认并给「提交」，否则这句会静默挂着没人发现。
  it("超时仍是 pending（窗口看不见）也读画面，卡住就转未确认", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 上一轮的回复", "─────────", "❯ 请把 README 改成英文", "─────────"]);
    const id = outgoingStore.add(7, { kind: "echo", text: "请把 README 改成英文", attachmentOnly: false, anchor: anchorFrom([], null), at: 0 });
    const { result } = mount([{ ...echo(id, "请把 README 改成英文"), phase: "pending" }]);
    await waitFor(() => expect(result.current.stuck.has(id)).toBe(true));
    expect(outgoingStore.get(7)[0].phase).toBe("uncertain");
  });

  it("没超时的 pending 不读画面", async () => {
    const { result } = mount([{ ...echo(5, "刚发的"), phase: "pending", at: Date.now() }]);
    await act(async () => {});
    expect(api.managedTerminalScreen).not.toHaveBeenCalled();
    expect(result.current.stuck.size).toBe(0);
  });

  it("输入框是空的、或插件认不出输入框，都不算卡住", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 回复", "─────────", "❯ ", "─────────"]);
    const { result } = mount([echo(3, "帮我看看这个报错")]);
    await waitFor(() => expect(api.managedTerminalScreen).toHaveBeenCalledTimes(1));
    await act(async () => {});
    expect(result.current.stuck.size).toBe(0);
    vi.clearAllMocks();
    mount([echo(4, "帮我看看这个报错")], {} as ChatUi);
    await act(async () => {});
    expect(api.managedTerminalScreen).not.toHaveBeenCalled();
  });
});
