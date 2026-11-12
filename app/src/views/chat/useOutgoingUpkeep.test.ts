import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem, ChatUi } from "../../api";

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

// 卸载 hook：它每秒计时的 setInterval 不收掉，会在测试环境拆掉后触发（window is not defined）。
afterEach(() => { cleanup(); vi.clearAllMocks(); outgoingStore.reset(); });

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

/// 做空第二轮 N1（2026-11-12，复现 redteam2/new.rt.test.tsx）：发出后加载窗口只剩发送之后的 200 条
/// assistant 记录，hook 记的是后发的另一句。这句看不见、不超时也不收起，修前一小时后仍挂着。
/// 兄弟场景：回显与排队插话；认得出输入框（claude）与认不出（{}）；运行中与空闲；窗口看得见的未确认。
describe("useOutgoingUpkeep：窗口看不见的消息由终端画面定去向", () => {
  const sentAt = Date.now() - 3_600_000;
  const items = Array.from({ length: 200 }, (_, i) => ({
    type: "assistant", id: `b${i}`, text: "x", timestamp: new Date(sentAt + 5_000 + i * 1_000).toISOString(),
  })) as unknown as ChatItem[];
  const anchor = { lastItemId: "a0", lastItemAt: sentAt - 1_000, priorItemIds: new Set(["a0"]), priorUserText: "上一句" };
  const add = (kind: "echo" | "queued", text = "请把 README 改成英文") =>
    outgoingStore.add(7, { kind, text, attachmentOnly: false, anchor, at: sentAt });
  const mountUnseen = (chatUi: ChatUi | null, running: { current: boolean } = { current: false }) => renderHook(() => useOutgoingUpkeep({
    sessionId: 7, outgoing: outgoingStore.get(7), loading: false, items, acks: [], lastUserText: "第二句", ended: false,
    chatUi, isActive: () => true, running: () => running.current, onError: () => {},
  }));

  it("回显：画面确认输入框里没有这句，当已送达收起", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 回复", "─────────", "❯ ", "─────────"]);
    add("echo");
    mountUnseen(claudeUi);
    await waitFor(() => expect(outgoingStore.get(7)).toHaveLength(0));
  });

  it("回显：还停在输入框里就照旧标未确认并给「提交」，不收起", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 回复", "─────────", "❯ 请把 README 改成英文", "─────────"]);
    const id = add("echo");
    const { result } = mountUnseen(claudeUi);
    await waitFor(() => expect(result.current.stuck.has(id)).toBe(true));
    expect(outgoingStore.get(7).map((p) => p.phase)).toEqual(["uncertain"]);
  });

  it("回显：CLI 认不出输入框，没法核对也没有「提交」可给，当已送达收起", async () => {
    add("echo");
    mountUnseen({} as ChatUi);
    await waitFor(() => expect(outgoingStore.get(7)).toHaveLength(0));
    expect(api.managedTerminalScreen).not.toHaveBeenCalled();
  });

  it("排队插话：运行中留着，转空闲后输入框里没有就收起", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 回复", "─────────", "❯ ", "─────────"]);
    add("queued");
    const running = { current: true };
    const { rerender } = mountUnseen(claudeUi, running);
    await act(async () => {});
    expect(outgoingStore.get(7).map((p) => p.phase)).toEqual(["pending"]);
    expect(api.managedTerminalScreen).not.toHaveBeenCalled();
    running.current = false;
    rerender();
    await waitFor(() => expect(outgoingStore.get(7)).toHaveLength(0));
  });

  it("排队插话：空闲后还在输入框里（Esc 打断退回），转成未确认的回显并给「提交」", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 回复", "─────────", "❯ 请把 README 改成英文", "─────────"]);
    const id = add("queued");
    const { result } = mountUnseen(claudeUi);
    await waitFor(() => expect(result.current.stuck.has(id)).toBe(true));
    expect(outgoingStore.get(7).map((p) => [p.kind, p.phase])).toEqual([["echo", "uncertain"]]);
  });

  it("窗口看得见的未确认回显：输入框空着也不收起（transcript 里确实没有这句）", async () => {
    api.managedTerminalScreen.mockResolvedValue(["● 回复", "─────────", "❯ ", "─────────"]);
    const id = outgoingStore.add(7, { kind: "echo", text: "帮我看看这个报错", attachmentOnly: false, anchor: anchorFrom([], null), phase: "uncertain" });
    mount([echo(id, "帮我看看这个报错")]);
    await waitFor(() => expect(api.managedTerminalScreen).toHaveBeenCalledTimes(1));
    await act(async () => {});
    expect(outgoingStore.get(7).map((p) => p.phase)).toEqual(["uncertain"]);
  });
});
