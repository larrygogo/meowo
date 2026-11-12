// 名单上直接对某个 agent 说话：名单底下的对话窗切到那条会话，用自己的发送（全部守卫）发出去。
// 用真实 ChatWindow：要钉的就是「守卫照常生效」，换成桩测不到。
import { describe, expect, it, vi } from "vitest";
import { act, render, waitFor } from "@testing-library/react";
import { adapter, invoke } from "./chatWindow.harness";
import { ChatWindow, type ChatShellBridge } from "./ChatWindow";

const shellWith = (extra: Partial<ChatShellBridge> = {}): ChatShellBridge => ({
  overlay: <div data-testid="roster" />,
  request: null, newSession: null, command: null, onSession: () => {}, onReveal: () => {}, onPageBack: () => {},
  ...extra,
});
const wait = (ms: number) => act(async () => { await new Promise((r) => setTimeout(r, ms)); });
const typed = (sid: number) => adapter.writes(sid).join("");

describe("名单上直接对 agent 说话", () => {
  it("切到那条会话、等它就绪后发出去；名单不让位", async () => {
    adapter.setup(31);
    const onReveal = vi.fn();
    const view = render(<ChatWindow shell={shellWith({ onReveal })} />);
    await wait(100);
    view.rerender(<ChatWindow shell={shellWith({ onReveal, request: { id: 32, seq: 1, send: "先跑测试" } })} />);
    // 就绪等待（SHELL_SEND_SETTLE_MS）之前一个字都不写。
    await wait(600);
    expect(typed(32)).toBe("");
    await waitFor(() => expect(typed(32)).toContain("先跑测试"), { timeout: 4000 });
    expect(onReveal).not.toHaveBeenCalled();
  });

  it("那条会话挂着审批：守卫拦下，不往终端写，名单让位给现场说明原因", async () => {
    const base = adapter.setup(33);
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => (
      command === "pending_interaction" && Number(args?.sessionId) === 34
        ? Promise.resolve({ approval: { sessionId: 34, requestId: "r-34", provider: "claude", toolName: "Bash", description: null, input: "{}", permissionSuggestions: [] }, question: null })
        : base(command, args)
    ));
    const onReveal = vi.fn();
    const view = render(<ChatWindow shell={shellWith({ onReveal })} />);
    await wait(100);
    view.rerender(<ChatWindow shell={shellWith({ onReveal, request: { id: 34, seq: 1, send: "继续" } })} />);
    await waitFor(() => expect(onReveal).toHaveBeenCalled(), { timeout: 4000 });
    expect(typed(34)).toBe("");
  });
});
