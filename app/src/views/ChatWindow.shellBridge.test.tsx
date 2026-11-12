// 做空审查第二轮：真实 ChatWindow 上的名单接线复现（修后留作回归）。
import { describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "./chatWindow.harness";
import { ChatWindow, type ChatShellBridge } from "./ChatWindow";
import { chatUi, descriptors } from "../test/agents";

const shellWith = (covered: boolean, extra: Partial<ChatShellBridge> = {}): ChatShellBridge => ({
  overlay: covered ? <div data-testid="roster"><button type="button" className="roster-row">行</button></div> : null,
  request: null, newSession: null, command: null, onSession: () => {}, onReveal: () => {}, onPageBack: () => {},
  ...extra,
});
const setup = (sid: number, withApproval = true) => {
  window.history.replaceState({}, "", "/?sessionId=" + sid);
  let pending: unknown = withApproval ? { sessionId: sid, requestId: "req-hidden", provider: "claude", toolName: "Bash", description: "危险", input: "rm -rf build", permissionSuggestions: [] } : null;
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({ sessionId: sid, title: "被盖住的会话", status: "running", provider: "claude", cwd: "C:/repo", supported: true, offset: 0, reset: false, pendingReview: withApproval ? "approval" : null, items: [], connected: true });
    if (command === "pending_interaction") return Promise.resolve({ approval: pending, question: null });
    if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
    if (command === "recent_cwds") return Promise.resolve(["C:/repo"]);
    if (command === "list_agents") return Promise.resolve(descriptors(["claude", "codex"]));
    if (command === "check_provider_hooks") return Promise.resolve("installed");
    if (command === "resolve_pending_approval") { pending = null; return Promise.resolve(); }
    return Promise.resolve();
  });
};
const resolves = () => invoke.mock.calls.filter(([c]) => c === "resolve_pending_approval").map(([, a]) => a);
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 80)); });

describe("对话窗接线", () => {
  it("C1 新建页已经开在名单底下时，名单输入框再交代一句：名单不让位，这句话没了去处", async () => {
    setup(21);
    const onReveal = vi.fn();
    const view = render(<ChatWindow shell={shellWith(true, { onReveal })} />);
    await screen.findByRole("button", { name: "允许一次", hidden: true });
    // 名单输入框第一次交代：新建页打开，名单让位。
    view.rerender(<ChatWindow shell={shellWith(true, { onReveal, newSession: { message: "第一句", seq: 1 } })} />);
    await settle();
    expect(onReveal).toHaveBeenCalledTimes(1);
    // 让位后（现场里是新建页），人点黑轨上的「回到名单」：名单重新盖上，新建页还开在底下。
    // 再在名单输入框交代第二句。
    onReveal.mockClear();
    view.rerender(<ChatWindow shell={shellWith(true, { onReveal, newSession: { message: "第二句", seq: 2 } })} />);
    await settle();
    expect(onReveal).toHaveBeenCalledTimes(1);
  });

  it("C2 管理页已经开在名单底下时，点名单顶部「已完成」：名单不让位", async () => {
    setup(22);
    const onReveal = vi.fn();
    const view = render(<ChatWindow shell={shellWith(true, { onReveal })} />);
    await screen.findByRole("button", { name: "允许一次", hidden: true });
    view.rerender(<ChatWindow shell={shellWith(true, { onReveal, command: { name: "admin", seq: 1 } })} />);
    await settle();
    expect(onReveal).toHaveBeenCalledTimes(1);
    onReveal.mockClear();
    view.rerender(<ChatWindow shell={shellWith(true, { onReveal, command: { name: "done", seq: 2 } })} />);
    await settle();
    expect(onReveal).toHaveBeenCalledTimes(1);
  });

  it("C3 搜索面板开着（名单仍盖着对话窗）时按 1 / Esc Esc：不会动被盖住的审批", async () => {
    setup(23);
    const view = render(<ChatWindow shell={shellWith(true)} />);
    await screen.findByRole("button", { name: "允许一次", hidden: true });
    view.rerender(<ChatWindow shell={shellWith(true, { command: { name: "search", seq: 1 } })} />);
    await settle();
    expect(document.querySelector(".chat-switcher-list")).not.toBeNull();
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(document.body, { key: "1" });
    fireEvent.keyDown(document.body, { key: "Escape" });
    fireEvent.keyDown(document.body, { key: "Escape" });
    await settle();
    expect(resolves()).toEqual([]);
  });

  it("C4 名单带过来的话覆盖掉这条会话输入框里原有的草稿", async () => {
    setup(24, false);
    const view = render(<ChatWindow shell={shellWith(false)} />);
    await settle();
    const box = () => document.querySelector<HTMLTextAreaElement>(".chat-window textarea")!;
    fireEvent.change(box(), { target: { value: "写了一半的长指令，还没发" } });
    await settle();
    expect(box().value).toBe("写了一半的长指令，还没发");
    // 回到名单，点开这一行，在名单输入框里说一句。
    view.rerender(<ChatWindow shell={shellWith(false, { request: { id: 24, seq: 1, draft: "继续" } })} />);
    await settle();
    expect(box().value).toContain("写了一半的长指令，还没发");
  });
});
