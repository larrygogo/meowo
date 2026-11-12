// 名单盖着对话窗时，对话窗不接键盘（做空审查 2026-11-12 H1 / M7 的复现，修后留作回归）。
// 用真实 ChatWindow：它挂在 window 上的快捷键是这里要防的东西，换成桩就测不到。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "./chatWindow.harness";
import { ChatWindow, type ChatShellBridge } from "./ChatWindow";
import { chatUi } from "../test/agents";

const shellWith = (covered: boolean, extra: Partial<ChatShellBridge> = {}): ChatShellBridge => ({
  overlay: covered ? <div data-testid="roster"><button type="button" className="roster-row">行</button></div> : null,
  request: null, newSession: null, command: null, onSession: () => {}, onReveal: () => {}, onPageBack: () => {},
  ...extra,
});
const setup = (sid: number) => {
  window.history.replaceState({}, "", "/?sessionId=" + sid);
  let pending: unknown = { sessionId: sid, requestId: "req-hidden", provider: "claude", toolName: "Bash", description: "危险", input: "rm -rf build", permissionSuggestions: [] };
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve({ sessionId: sid, title: "被盖住的会话", status: "running", provider: "claude", cwd: "C:/repo", supported: true, offset: 0, reset: false, pendingReview: "approval", items: [], connected: true });
    if (command === "pending_interaction") return Promise.resolve({ approval: pending, question: null });
    if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
    if (command === "resolve_pending_approval") { pending = null; return Promise.resolve(); }
    return Promise.resolve();
  });
};
const resolves = () => invoke.mock.calls.filter(([c]) => c === "resolve_pending_approval").map(([, a]) => a);
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 50)); });

describe("名单盖着时对话窗不接键盘", () => {
  it("数字键 1 不会批准被盖住的审批", async () => {
    setup(13);
    render(<ChatWindow shell={shellWith(true)} />);
    await screen.findByRole("button", { name: "允许一次", hidden: true });
    // 真实 WebView 里 inert 子树不能持有焦点（jsdom 不实现），手动失焦来对齐：按键落在 body 上。
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(document.body, { key: "1" });
    await settle();
    expect(resolves()).toEqual([]);
  });

  it("在名单行上连按两下 Esc 不会拒绝被盖住的审批", async () => {
    setup(14);
    render(<ChatWindow shell={shellWith(true)} />);
    await screen.findByRole("button", { name: "允许一次", hidden: true });
    const row = screen.getByRole("button", { name: "行" });
    row.focus();
    fireEvent.keyDown(row, { key: "Escape" });
    fireEvent.keyDown(row, { key: "Escape" });
    await settle();
    expect(resolves()).toEqual([]);
  });

  it("Ctrl+2 不会把被盖住的对话窗切到终端视图", async () => {
    setup(15);
    const { container } = render(<ChatWindow shell={shellWith(true)} />);
    await screen.findByRole("button", { name: "允许一次", hidden: true });
    fireEvent.keyDown(document.body, { key: "2", code: "Digit2", ctrlKey: true });
    await settle();
    expect(container.querySelector(".chat-window")?.classList.contains("is-terminal")).toBe(false);
  });

  it("名单带过来的话落进这条会话的输入框，由人在这里发", async () => {
    setup(17);
    const shell: ChatShellBridge = { ...shellWith(false), request: { id: 17, seq: 1, draft: "继续，先跑测试" } };
    const { container } = render(<ChatWindow shell={shell} />);
    await screen.findByRole("button", { name: "允许一次" });
    await settle();
    expect(container.querySelector<HTMLTextAreaElement>(".chat-window textarea")?.value).toBe("继续，先跑测试");
    expect(invoke.mock.calls.some(([c]) => c === "write_managed_terminal")).toBe(false);
  });

  it("没盖着（进了现场）时快捷键照常：数字键 1 批准当前这张卡", async () => {
    setup(16);
    render(<ChatWindow shell={shellWith(false)} />);
    await screen.findByRole("button", { name: "允许一次" });
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(document.body, { key: "1" });
    await settle();
    expect(resolves()).toEqual([{ sessionId: 16, requestId: "req-hidden", choice: "allow_once" }]);
  });

  it("名单叫出的搜索面板收得到自己的 Esc；面板关掉后数字键仍然批不了被盖住的审批", async () => {
    setup(17);
    const view = render(<ChatWindow shell={shellWith(true)} />);
    await screen.findByRole("button", { name: "允许一次", hidden: true });
    view.rerender(<ChatWindow shell={shellWith(true, { command: { name: "search", seq: 1 } })} />);
    await settle();
    const list = document.querySelector(".chat-switcher-list");
    expect(list).not.toBeNull();
    fireEvent.keyDown(list!.parentElement!.querySelector("input")!, { key: "Escape" });
    await settle();
    expect(document.querySelector(".chat-switcher-list")).toBeNull();
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(document.body, { key: "1" });
    await settle();
    expect(resolves()).toEqual([]);
  });
});
