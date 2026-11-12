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
    <div data-testid="chat" data-request={shell?.request?.id ?? ""} data-draft={shell?.request?.draft ?? ""} data-send={shell?.request?.send ?? ""} data-resume={shell?.request?.resume ? "1" : ""} data-new-session={shell?.newSession?.message ?? ""} data-command={shell?.command?.name ?? ""}>
      {shell?.overlay}
      <button type="button" onClick={() => shell?.onReveal()}>reveal</button>
      <button type="button" onClick={() => shell?.onPageBack()}>page-back</button>
      {shell?.onBack && !shell.overlay && <button type="button" onClick={shell.onBack}>back</button>}
    </div>
  ),
}));

import { makeSession } from "../../demo/data";
import type { Item } from "../sticker/types";
import { setShellCovered } from "../chat/shellCover";
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
    // 最后一行来了审批、第一行出错：行不动，只有状态词变。
    sessions = [
      { ...sessions[0], errored: true },
      sessions[1],
      { ...sessions[2], pending_review: "approval", session: { ...sessions[2].session, last_event_at: Date.now() + 5000 } },
    ];
    await act(async () => { boardHandlers.get("board-urgent")?.({ payload: null }); });
    await flush();
    expect(names()).toEqual(["甲", "乙", "丙"]);
    expect(document.querySelector(`.roster-item[data-session-id="${sessions[2].session.id}"] .roster-word`)?.textContent).toBe("待批准");
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
    fireEvent.click(screen.getByRole("button", { name: "back" }));
    expect(names()).toEqual(["甲", "乙"]);
  });

  it("只开了一个会话：直接是现场，不出名单", async () => {
    sessions = [mk("甲")];
    render(<RosterShell />);
    await flush();
    expect(document.querySelector(".roster")).toBeNull();
    expect(screen.queryByRole("button", { name: "back" })).toBeNull();
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

  // leader 2026-11-12：主窗口上不放眼睛，眼睛只在灵动岛上；岛也不因主窗口在前台而收细条。
  it("主窗口里没有眼睛，也不再告诉岛「主窗口在前台」", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    await screen.findByText("甲");
    await act(async () => { focus.handler({ payload: false }); });
    expect(document.querySelector(".isl-eyes")).toBeNull();
    expect((emit.mock.calls as unknown[][]).some(([name]) => name === "roster-front")).toBe(false);
  });

  it("设置在名单顶部；进了现场「回到名单」由对话窗摆在顶栏上", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    fireEvent.click(await screen.findByRole("button", { name: "打开设置" }));
    expect(invoke).toHaveBeenCalledWith("open_settings");
    fireEvent.doubleClick(screen.getByText("甲"));
    fireEvent.click(screen.getByRole("button", { name: "back" }));
    expect(names()).toEqual(["甲", "乙"]);
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
    // 答完到名单刷新之前：写「已处理」，不摆「已回到终端」和那排终端按键（Esc 按下去是打断它）。
    await screen.findByText("已处理");
    expect(screen.queryByRole("button", { name: "Esc" })).toBeNull();
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
    // 「接管到 Meowo」只把人带进它的现场，接管的确认在那里做。
    fireEvent.click(screen.getByRole("button", { name: "接管到 Meowo" }));
    expect(screen.getByTestId("chat").getAttribute("data-request")).toBe(String(sessions[1].session.id));
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

  it("输入框常驻；没有行展开时提交 = 带着这句话去派新 agent", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    await screen.findByText("甲");
    const input = screen.getByPlaceholderText("交代一个结果，派一个 agent");
    fireEvent.change(input, { target: { value: "把登录页的校验收进一个 schema" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByTestId("chat").getAttribute("data-new-session")).toBe("把登录页的校验收进一个 schema");
  });

  it("展开一行后输入框指向它并一直写明任务名；提交 = 交给名单底下的对话窗发，人留在名单上，名单自己不往终端写", async () => {
    sessions = [mk("甲"), mk("乙", (s) => { s.pty_managed = true; s.session.status = "waiting"; })];
    const target = sessions[1].session.id;
    render(<RosterShell />);
    await screen.findByText("甲");
    // 先为「派新 agent」打了字，再点开一行：去向变了，输入框上面那行字要跟着变（有字时 placeholder 看不见）。
    const input = screen.getByPlaceholderText("交代一个结果，派一个 agent");
    fireEvent.change(input, { target: { value: "继续，先跑测试" } });
    expect(document.querySelector(".roster-composer-to")?.textContent).toBe("派一个新 agent");
    fireEvent.click(screen.getByText("乙"));
    expect(document.querySelector(".roster-composer-to")?.textContent).toBe("对「乙」说");
    fireEvent.keyDown(screen.getByLabelText("对「乙」说"), { key: "Enter" });
    const chat = screen.getByTestId("chat");
    expect(chat.getAttribute("data-request")).toBe(String(target));
    expect(chat.getAttribute("data-send")).toBe("继续，先跑测试");
    expect(chat.getAttribute("data-draft")).toBe("");
    expect(chat.getAttribute("data-new-session")).toBe("");
    expect(document.querySelector(".roster")).not.toBeNull();
    expect(document.querySelector(".roster-composer-to")?.textContent).toBe("已发给「乙」");
    expect(invoke.mock.calls.some(([cmd]) => cmd === "write_managed_terminal")).toBe(false);
  });

  it("一个会话都没有：空名单加输入框", async () => {
    sessions = [];
    render(<RosterShell />);
    await screen.findByText("还没有会话");
    expect(screen.getByPlaceholderText("交代一个结果，派一个 agent")).toBeTruthy();
  });

  it("界面上没有计数", async () => {
    sessions = [mk("甲", (s) => { s.pending_review = "approval"; }), mk("乙", (s) => { s.pending_review = "question"; })];
    render(<RosterShell />);
    await screen.findByText("甲");
    expect(document.querySelector(".roster-shell")?.textContent).not.toMatch(/\d+\s*个/);
  });

  it("顶部的「已完成」叫对话窗打开管理页；在那页点返回回到名单", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    await screen.findByText("甲");
    fireEvent.click(screen.getByRole("button", { name: "已完成" }));
    expect(screen.getByTestId("chat").dataset.command).toBe("done");
    // 管理页打开时对话窗会说「我被带走了」，名单让位。
    fireEvent.click(screen.getByRole("button", { name: "reveal" }));
    expect(document.querySelector(".roster")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "page-back" }));
    expect(names()).toEqual(["甲", "乙"]);
  });

  it("从现场里进的管理页点返回不会被拉回名单", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    fireEvent.doubleClick(await screen.findByText("甲"));
    expect(document.querySelector(".roster")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "page-back" }));
    expect(document.querySelector(".roster")).toBeNull();
  });

  it("名单上按 Ctrl+K 叫出搜索", async () => {
    sessions = [mk("甲"), mk("乙")];
    render(<RosterShell />);
    await screen.findByText("甲");
    // 对话窗是桩，不会自己置位「被名单盖着」。
    setShellCovered(true);
    try {
      fireEvent.keyDown(document.body, { key: "k", code: "KeyK", ctrlKey: true });
      expect(screen.getByTestId("chat").dataset.command).toBe("search");
    } finally {
      setShellCovered(false);
    }
  });

  it("窗口隐藏时不轮询：名单、展开行的速览和决定单都停", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const hidden = vi.spyOn(document, "hidden", "get");
    const visibility = vi.spyOn(document, "visibilityState", "get");
    try {
      sessions = [mk("甲"), mk("乙", (s) => { s.pending_review = "approval"; })];
      render(<RosterShell />);
      // 先后展开一条在跑的（速览 2 秒一拍）和一条待处理的（决定单 1 秒一拍），确认两种轮询都在转。
      fireEvent.click(await screen.findByText("甲"));
      await act(async () => { vi.advanceTimersByTime(2500); });
      fireEvent.click(screen.getByText("乙"));
      await act(async () => { vi.advanceTimersByTime(1500); });
      expect(invoke.mock.calls.some(([cmd]) => cmd === "pending_interaction")).toBe(true);
      hidden.mockReturnValue(true);
      visibility.mockReturnValue("hidden");
      invoke.mockClear();
      await act(async () => { vi.advanceTimersByTime(30_000); });
      expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual([]);
    } finally {
      hidden.mockRestore();
      visibility.mockRestore();
      vi.useRealTimers();
    }
  });

  it("告诉后端名单是否摆在眼前：名单上为真，进了现场为假，卸载时收回", async () => {
    sessions = [mk("甲"), mk("乙")];
    const showing = () => invoke.mock.calls.filter(([cmd]) => cmd === "set_roster_showing").map(([, args]) => (args as { showing: boolean }).showing);
    const view = render(<RosterShell />);
    fireEvent.doubleClick(await screen.findByText("甲"));
    expect(showing()).toEqual([true, false]);
    view.unmount();
    expect(showing().at(-1)).toBe(false);
  });

  it("出错的行：还活着的给「继续」（在名单上对它说一声继续），CLI 已经退出的给「恢复」", async () => {
    sessions = [
      mk("甲", (s) => { s.errored = true; s.error_label = "API Error"; s.session.status = "waiting"; }),
      mk("乙", (s) => { s.errored = true; s.error_label = "退出码 1"; s.session.status = "ended"; }),
    ];
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("甲"));
    fireEvent.click(screen.getByRole("button", { name: "继续" }));
    const chat = screen.getByTestId("chat");
    expect(chat.getAttribute("data-request")).toBe(String(sessions[0].session.id));
    expect(chat.getAttribute("data-send")).toBe("继续");
    expect(screen.queryByRole("button", { name: "恢复" })).toBeNull();
    fireEvent.click(screen.getByText("乙"));
    fireEvent.click(screen.getByRole("button", { name: "恢复" }));
    expect(chat.getAttribute("data-request")).toBe(String(sessions[1].session.id));
    expect(chat.getAttribute("data-resume")).toBe("1");
    // 两种都留在名单上。
    expect(document.querySelector(".roster")).not.toBeNull();
  });
});
