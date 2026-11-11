import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatShellBridge } from "../ChatWindow";

// 名单外壳自己的行为：顺序不随事件跳、展开只由人触发且一次一行、只有一个会话直接是现场、
// 点具体会话进来不落名单。对话窗换成一个只回显接线的桩（它自己的行为由 ChatWindow.*.test 管）。
const invoke = vi.hoisted(() => vi.fn());
const boardHandlers = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
const emit = vi.hoisted(() => vi.fn(() => Promise.resolve()));
const focus = vi.hoisted(() => ({ handler: (_event: { payload: boolean }) => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFocused: () => Promise.resolve(true),
    onFocusChanged: (handler: (event: { payload: boolean }) => void) => { focus.handler = handler; return Promise.resolve(() => {}); },
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit,
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    boardHandlers.set(name, handler);
    return Promise.resolve(() => {});
  }),
}));
vi.mock("../ChatWindow", () => ({
  ChatWindow: ({ shell }: { shell?: ChatShellBridge }) => (
    <div data-testid="chat" data-request={shell?.request?.id ?? ""}>
      {shell?.overlay}
      <button type="button" onClick={() => shell?.onReveal()}>reveal</button>
    </div>
  ),
}));

import { makeSession } from "../../demo/data";
import type { Item } from "../sticker/types";
import { RosterShell } from "./RosterShell";

let sessions: Item[] = [];
const mk = (title: string, patch: (s: Item) => void = () => {}) => {
  const s = makeSession({ title, project: "p" });
  patch(s);
  return s;
};
const names = () => [...document.querySelectorAll(".roster-name b")].map((el) => el.textContent);
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

describe("名单外壳", () => {
  afterEach(cleanup);
  beforeEach(() => {
    localStorage.clear();
    boardHandlers.clear();
    invoke.mockReset();
    emit.mockClear();
    invoke.mockImplementation((cmd: string) => Promise.resolve(
      cmd === "get_live_sessions_page" ? { items: sessions, next_cursor: null } : cmd === "sidebar_git_badges" ? [] : undefined,
    ));
    window.history.replaceState({}, "", "/?roster=1&sessionId=1");
  });

  it("首屏是名单；状态变化不改变行的顺序", async () => {
    sessions = [mk("甲"), mk("乙"), mk("丙")];
    render(<RosterShell />);
    await screen.findByText("甲");
    expect(names()).toEqual(["甲", "乙", "丙"]);
    // 最后一行来了审批、第一行出错：行不动，只有眼睛走过去。
    sessions = [
      { ...sessions[0], errored: true },
      sessions[1],
      { ...sessions[2], pending_review: "approval", session: { ...sessions[2].session, last_event_at: Date.now() + 5000 } },
    ];
    await act(async () => { boardHandlers.get("board-urgent")?.({ payload: null }); });
    await flush();
    expect(names()).toEqual(["甲", "乙", "丙"]);
    expect(document.querySelector(".roster-eyes")?.getAttribute("data-at")).toBe(String(sessions[2].session.id));
  });

  it("展开只由点击触发，同一时间只展开一行", async () => {
    sessions = [mk("甲", (s) => { s.pending_review = "approval"; }), mk("乙")];
    render(<RosterShell />);
    await screen.findByText("甲");
    // 有待批准的行也不自己展开。
    expect(document.querySelectorAll(".roster-sheet")).toHaveLength(0);
    fireEvent.click(screen.getByText("甲"));
    fireEvent.click(screen.getByText("乙"));
    expect(document.querySelectorAll(".roster-sheet")).toHaveLength(1);
    expect(document.querySelector(".roster-item.is-open .roster-name b")?.textContent).toBe("乙");
  });

  it("「进入现场」把那个会话交给对话窗，名单让位", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("乙"));
    fireEvent.click(screen.getByRole("button", { name: "进入现场" }));
    expect(screen.getByTestId("chat").getAttribute("data-request")).toBe(String(sessions[1].session.id));
    expect(document.querySelector(".roster")).toBeNull();
    // 现场里能回到名单。
    fireEvent.click(screen.getByRole("button", { name: "回到名单" }));
    expect(names()).toEqual(["甲", "乙"]);
  });

  it("只开了一个会话：直接是现场，不出名单", async () => {
    sessions = [mk("甲")];
    render(<RosterShell />);
    await flush();
    expect(document.querySelector(".roster")).toBeNull();
    expect(screen.queryByRole("button", { name: "回到名单" })).toBeNull();
  });

  it("冲着某个会话打开的窗口直接进现场；被带去别处时名单让位", async () => {
    sessions = [mk("甲"), mk("乙")];
    window.history.replaceState({}, "", "/?sessionId=1");
    const first = render(<RosterShell />);
    await flush();
    expect(document.querySelector(".roster")).toBeNull();
    first.unmount();

    window.history.replaceState({}, "", "/?roster=1&sessionId=1");
    render(<RosterShell />);
    await screen.findByText("甲");
    fireEvent.click(screen.getByRole("button", { name: "reveal" }));
    expect(document.querySelector(".roster")).toBeNull();
  });

  it("窗口在前台时黑轨上有眼睛并告诉岛；失焦后眼睛离开黑轨", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    await screen.findByText("甲");
    expect(document.querySelectorAll(".roster-rail .isl-eyes")).toHaveLength(1);
    expect(emit).toHaveBeenCalledWith("roster-front", true);
    await act(async () => { focus.handler({ payload: false }); });
    expect(document.querySelector(".roster-rail .isl-eyes")).toBeNull();
    expect(emit).toHaveBeenLastCalledWith("roster-front", false);
  });

  it("挂载时注册名单租约（会话号 0），卸载时注销同一个", async () => {
    sessions = [mk("甲"), mk("乙")];
    const view = render(<RosterShell />);
    await screen.findByText("甲");
    const call = invoke.mock.calls.find(([cmd]) => cmd === "register_approval_consumer");
    expect(call?.[1]).toMatchObject({ sessionId: 0 });
    view.unmount();
    expect(invoke).toHaveBeenCalledWith("unregister_approval_consumer", { consumerId: (call?.[1] as { consumerId: string }).consumerId });
  });

  it("行内批准：不进那个会话，决定按它的会话号和请求号发出去", async () => {
    sessions = [mk("甲"), mk("乙", (s) => { s.pending_review = "approval"; })];
    const target = sessions[1].session.id;
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((cmd: string, args?: { sessionId?: number }) => (
      cmd === "pending_interaction"
        ? Promise.resolve({ approval: args?.sessionId === target ? { sessionId: target, requestId: "req-9", provider: "claude", toolName: "Bash", description: null, input: JSON.stringify({ command: "git push" }), permissionSuggestions: [], answerable: false } : null, question: null })
        : base(cmd, args)
    ));
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("乙"));
    fireEvent.click(await screen.findByRole("button", { name: "允许一次" }));
    expect(invoke).toHaveBeenCalledWith("resolve_pending_approval", { sessionId: target, requestId: "req-9", choice: "allow_once" });
    // 没有切会话：对话窗没收到「去那个会话」的请求。
    expect(screen.getByTestId("chat").getAttribute("data-request")).toBe("");
  });

  it("行内按 Esc 只收起这一行，不发任何决定", async () => {
    sessions = [mk("甲"), mk("乙", (s) => { s.pending_review = "approval"; })];
    const target = sessions[1].session.id;
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((cmd: string, args?: { sessionId?: number }) => (
      cmd === "pending_interaction"
        ? Promise.resolve({ approval: { sessionId: target, requestId: "req-9", provider: "claude", toolName: "Bash", description: null, input: "{}", permissionSuggestions: [], answerable: false }, question: null })
        : base(cmd, args)
    ));
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("乙"));
    const deny = await screen.findByRole("button", { name: "拒绝" });
    fireEvent.keyDown(deny, { key: "Escape" });
    expect(document.querySelectorAll(".roster-sheet")).toHaveLength(0);
    expect(invoke.mock.calls.some(([cmd]) => cmd === "resolve_pending_approval")).toBe(false);
  });

  it("在外部终端里跑的会话：行内如实说明，给「跳到终端」和进入现场", async () => {
    sessions = [mk("甲"), mk("乙", (s) => { s.pending_review = "approval"; s.pty_managed = false; s.pid = 4321; })];
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((cmd: string, args?: unknown) => (
      cmd === "pending_interaction" ? Promise.resolve({ approval: null, question: null })
        : cmd === "focus_session" ? Promise.resolve("alive_but_not_found")
          : base(cmd, args)
    ));
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("乙"));
    await screen.findByText(/它在外部终端里运行/);
    expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull();
    expect(screen.getByRole("button", { name: "进入现场" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "跳到终端" }));
    expect(invoke).toHaveBeenCalledWith("focus_session", expect.objectContaining({ pid: 4321 }));
    // 没切过去要说出来，不能点了没反应。
    await screen.findByText("没找到那个终端窗口");
  });

  it("终端不在 Meowo 手里、进程也不在了：行内说明，给进入现场", async () => {
    sessions = [mk("甲"), mk("乙", (s) => { s.pending_review = "approval"; s.pty_managed = false; s.pid = null; })];
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((cmd: string, args?: unknown) => (
      cmd === "pending_interaction" ? Promise.resolve({ approval: null, question: null }) : base(cmd, args)
    ));
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("乙"));
    await screen.findByText("这条审批在终端里，进入现场处理");
    expect(screen.queryByRole("button", { name: "跳到终端" })).toBeNull();
    expect(screen.getByRole("button", { name: "进入现场" })).toBeTruthy();
  });

  it("MCP 工具名写成「服务 · 工具」，不摆双下划线原名", async () => {
    sessions = [mk("甲", (s) => { s.current_activity = "mcp__kimi-cu__get_app_state"; }), mk("乙")];
    render(<RosterShell />);
    await screen.findByText("kimi-cu · get_app_state");
  });

  it("Meowo 持有终端的会话答不了时：行内给终端画面和按键，不让人去找终端", async () => {
    sessions = [mk("甲"), mk("乙", (s) => { s.pending_review = "question"; s.pty_managed = true; })];
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((cmd: string, args?: unknown) => (
      cmd === "pending_interaction" ? Promise.resolve({ approval: null, question: null })
        : cmd === "managed_terminal_screen" ? Promise.resolve(["Pick one", "1. Yes", "2. No"])
          : base(cmd, args)
    ));
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("乙"));
    await screen.findByText(/Pick one/);
    expect(screen.getByRole("button", { name: "Esc" })).toBeTruthy();
  });

  it("界面上没有计数", async () => {
    sessions = [mk("甲", (s) => { s.pending_review = "approval"; }), mk("乙", (s) => { s.pending_review = "question"; })];
    render(<RosterShell />);
    await screen.findByText("甲");
    expect(document.querySelector(".roster-shell")?.textContent).not.toMatch(/\d+\s*个/);
  });
});
