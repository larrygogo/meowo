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
import { chatUi } from "../../test/agents";

const claudeUi = { composer: chatUi("claude")!.composer } as unknown as ChatUi;

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
afterEach(() => { cleanup(); vi.clearAllMocks(); outgoingStore.reset(); localStorage.clear(); });

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

/** 2026-11-16 Kimi 实拍：运行中发的话被 Kimi 挪到输入框上方排队（`❯ 原文` + `↑ to edit · ctrl-s to steer
 *  immediately`）。Kimi 排队时不写会话记录、不触发提交 hook，Meowo 等不到证据，误报「未确认送达」，
 *  点重发又说「正文可能仍在终端输入框」。现在读画面认出排队区就标「已排队」。 */
describe("useOutgoingUpkeep：消息排在 CLI 自己的队列里", () => {
  const kimiUi = { composer: chatUi("kimi")!.composer, queued_hint: chatUi("kimi")!.queued_hint } as unknown as ChatUi;
  const kimiQueued = [
    "● Read 6 files · 1050 lines",
    "",
    "🌗 Tip: /goal for multi-step work with a clear finish line",
    "──────────────────────────────────────────────────────",
    "❯ AI Bot 沟通群里也发了很多涉及的仓库，你都看一下",
    "↑ to edit · ctrl-s to steer immediately",
    "╭────────────────────────────────────────────────────╮",
    "│ >                                                  │",
    "╰────────────────────────────────────────────────────╯",
    "Ask When Needed  K3.1 Pro thinking: high",
  ];
  const queued = (id: number, text: string, phase: OutgoingPrompt["phase"]): OutgoingPrompt => ({
    id, kind: "queued", text, attachmentOnly: false, at: 0, phase, settledAt: null, anchor: anchorFrom([], null), bodyWritten: true,
  } as OutgoingPrompt);
  const mountRunning = (outgoing: OutgoingPrompt[], ui: ChatUi) => renderHook(() => useOutgoingUpkeep({
    sessionId: 7, outgoing, loading: false, items: [], acks: [], lastUserText: null, ended: false,
    chatUi: ui, isActive: () => true, running: () => true, onError: () => {},
  }));

  it("未确认送达的排队消息：画面上排在 Kimi 队列里就标已排队，不算卡在输入框", async () => {
    api.managedTerminalScreen.mockResolvedValue(kimiQueued);
    const text = "AI Bot 沟通群里也发了很多涉及的仓库，你都看一下";
    const id = outgoingStore.add(7, { kind: "queued", text, attachmentOnly: false, anchor: anchorFrom([], null), phase: "uncertain", bodyWritten: true });
    const { result } = mountRunning([queued(id, text, "uncertain")], kimiUi);
    await waitFor(() => expect(outgoingStore.get(7)[0].phase).toBe("accepted"));
    expect(outgoingStore.get(7)[0].bodyWritten).toBe(false);
    expect(result.current.stuck.has(id)).toBe(false);
  });

  it("排队区里是别的话、或者这家没声明排队提示：不当成已排队", async () => {
    api.managedTerminalScreen.mockResolvedValue(kimiQueued);
    const id = outgoingStore.add(7, { kind: "queued", text: "另一句完全不同的话", attachmentOnly: false, anchor: anchorFrom([], null), phase: "uncertain", bodyWritten: true });
    mountRunning([queued(id, "另一句完全不同的话", "uncertain")], kimiUi);
    await waitFor(() => expect(api.managedTerminalScreen).toHaveBeenCalled());
    await act(async () => {});
    expect(outgoingStore.get(7)[0].phase).toBe("uncertain");
    vi.clearAllMocks();
    // 没声明排队提示的家（运行中的排队消息照旧等证据，不读画面）。
    mountRunning([queued(99, "AI Bot 沟通群里也发了很多涉及的仓库", "uncertain")], { composer: chatUi("kimi")!.composer } as unknown as ChatUi);
    await act(async () => {});
    expect(api.managedTerminalScreen).not.toHaveBeenCalled();
  });
});
