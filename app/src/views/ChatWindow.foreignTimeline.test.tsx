// 对话页不显示别的会话的内容（2026-11-26 用户实拍：新建会话的第一条消息上方出现了同工作区另一个会话的旧回复）。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, render, waitFor } from "@testing-library/react";
import { invoke, eventListeners } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { chatUi, descriptors } from "../test/agents";
import { rememberHistory } from "./chat/historyCache";
import type { ChatHistory, ChatItem } from "../api";

const ts = (time: string) => `2026-11-26T${time}Z`;
const OLD: ChatItem = { type: "assistant_text", id: "a-old", timestamp: ts("06:29:07"), text: "别的会话的旧回复" };
const NEW: ChatItem = { type: "user_text", id: "u-new", timestamp: ts("09:06:19"), text: "新会话的第一句" };

function historyOf(sessionId: number, items: ChatItem[], offset: number): ChatHistory {
  return {
    sessionId, title: `会话${sessionId}`, status: "waiting", provider: "claude", cwd: "C:/repo",
    supported: true, offset, reset: false, pendingReview: null, items, model: null,
    connected: true, predecessorId: null, supersededBy: null, agentModes: [],
  } as unknown as ChatHistory;
}

/** 61 是旧会话、71 是新会话；新会话的 transcript 里只有一句话。 */
function mockBackend() {
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    const sid = Number(args?.sessionId);
    if (command === "get_chat_history") {
      const offset = Number(args?.offset ?? 0);
      if (sid === 61) return Promise.resolve(historyOf(61, offset > 0 ? [] : [OLD], 100));
      if (sid === 71) return Promise.resolve(historyOf(71, offset > 0 ? [] : [NEW], 50));
      return Promise.reject(new Error("no rows"));
    }
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
    if (command === "list_agents") return Promise.resolve(descriptors(["claude"]));
    if (command === "managed_terminal_binding") return Promise.resolve(null);
    if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: sid, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
    return Promise.resolve();
  });
}

// 正文渲染会切成多段（逐段渐显），按整页文字判断有没有。
const bodyText = () => document.body.textContent ?? "";
const shows = (text: string) => waitFor(() => expect(bodyText()).toContain(text));

const foreignLogs = () => invoke.mock.calls
  .filter(([command, args]) => command === "diag_log" && String((args as { message: string }).message).includes("kind=foreign_timeline"))
  .map(([, args]) => String((args as { message: string }).message));

describe("ChatWindow 不串会话", () => {
  it("快照缓存里存的是别的会话的历史：不采用，按新会话自己的记录显示，并记日志", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    mockBackend();
    rememberHistory(71, { history: historyOf(61, [OLD], 100), items: [OLD], offset: 100, earliest: 0, hasEarlier: false });
    render(<ChatWindow />);
    await shows("别的会话的旧回复");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    await shows("新会话的第一句");
    expect(bodyText()).not.toContain("别的会话的旧回复");
    const logs = foreignLogs();
    expect(logs.some((line) => line.includes("reason=snapshot_mismatch") && line.includes("session=71") && line.includes("from=61"))).toBe(true);
    // 日志不含消息正文。
    expect(logs.join("\n")).not.toContain("旧回复");
  });

  it("首读之前列表里已混进别的内容：以从头读到的记录整段替换，不接在后面", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    mockBackend();
    // 模拟那条没定位的竞态：新会话名下已经挂着一份不属于它的消息，读取位置却还在开头。
    rememberHistory(71, { history: historyOf(71, [OLD], 0), items: [OLD], offset: 0, earliest: 0, hasEarlier: false });
    render(<ChatWindow />);
    await shows("别的会话的旧回复");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    await shows("新会话的第一句");
    await waitFor(() => expect(bodyText()).not.toContain("别的会话的旧回复"));
    // 残留出处的线索：从哪个会话切过来、残留的时间，不含正文。
    const leftover = foreignLogs().find((line) => line.includes("reason=first_read_leftover"));
    expect(leftover).toContain("session=71");
    expect(leftover).toContain("from=61");
    expect(leftover).toContain("first=2026-11-26T06:29:07Z");
    expect(leftover).not.toContain("旧回复");
  });

  it("正常切换会话不写 foreign_timeline 日志", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    mockBackend();
    render(<ChatWindow />);
    await shows("别的会话的旧回复");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    await shows("新会话的第一句");
    expect(bodyText()).not.toContain("别的会话的旧回复");
    // 再切回去走快照、再切过来，都不该误报。
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 61 }));
    await shows("别的会话的旧回复");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    await shows("新会话的第一句");
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(foreignLogs()).toEqual([]);
  });

  it("响应里写的是别的会话：一条都不收，并记日志", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    mockBackend();
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      // 请求 71 却拿回 61 的历史（审查探针 P3 的形态）。
      if (command === "get_chat_history" && Number(args?.sessionId) === 71) return Promise.resolve(historyOf(61, [OLD, NEW], 100));
      return base(command, args);
    });
    render(<ChatWindow />);
    await shows("别的会话的旧回复");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    await waitFor(() => expect(foreignLogs().some((line) => line.includes("reason=response_mismatch") && line.includes("session=71") && line.includes("from=61"))).toBe(true));
    expect(bodyText()).not.toContain("别的会话的旧回复");
  });

  it("切走后旧会话的结果才到：不显示，带内容的记一行", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    mockBackend();
    const base = invoke.getMockImplementation()!;
    let release: (() => void) | null = null;
    let first = true;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history" && Number(args?.sessionId) === 61 && first) {
        first = false;
        // 61 的首读挂起，等切到 71 之后才放回。
        return new Promise((resolve) => { release = () => resolve(historyOf(61, [OLD], 100)); });
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await waitFor(() => expect(release).not.toBeNull());
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    act(() => release?.());
    await waitFor(() => expect(foreignLogs().some((line) => line.includes("reason=late_discarded") && line.includes("session=71") && line.includes("from=61"))).toBe(true), { timeout: 4_000 });
    await shows("新会话的第一句");
    expect(bodyText()).not.toContain("别的会话的旧回复");
  });
});
