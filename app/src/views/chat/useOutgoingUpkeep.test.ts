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
