// 做空审查第二轮：展开「跑完没看过」的行之后眼睛是否离开；「接管到 Meowo」带了什么。
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatShellBridge } from "../ChatWindow";

const invoke = vi.hoisted(() => vi.fn());
const handlers = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ isFocused: () => Promise.resolve(true), onFocusChanged: () => Promise.resolve(() => {}) }) }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(() => Promise.resolve()), listen: vi.fn((n: string, h: (e: { payload: unknown }) => void) => { handlers.set(n, h); return Promise.resolve(() => {}); }) }));
vi.mock("../ChatWindow", () => ({
  ChatWindow: ({ shell }: { shell?: ChatShellBridge }) => <div data-testid="chat" data-request={JSON.stringify(shell?.request ?? null)}>{shell?.overlay}</div>,
}));
import { makeSession } from "../../demo/data";
import type { Item } from "../sticker/types";
import { RosterShell } from "./RosterShell";

let sessions: Item[] = [];
const wait = (ms: number) => act(async () => { await new Promise((r) => setTimeout(r, ms)); });
describe("眼睛与接管", () => {
  afterEach(cleanup);
  it("E1 展开看过一条「跑完没看」的行：眼睛仍站在它旁边，下一条没看的得不到指", async () => {
    const a = makeSession({ title: "甲跑完", project: "p", status: "waiting", lastAi: "做完了" });
    const b = makeSession({ title: "乙跑完", project: "p", status: "waiting", lastAi: "也做完了" });
    (a as Item).unread = true; (b as Item).unread = true;
    sessions = [a, b];
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "get_live_sessions_page" ? { items: sessions, next_cursor: null } : cmd === "sidebar_git_badges" ? [] : cmd === "get_chat_history" ? { items: [] } : undefined));
    window.history.replaceState({}, "", "/?roster=1");
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("甲跑完"));
    await wait(2300);
    await act(async () => { handlers.get("board-changed")?.({ payload: null }); });
    await wait(600);
    const cmds = [...new Set(invoke.mock.calls.map(([c]) => c))];
    expect(document.querySelector(".roster-eyes")?.getAttribute("data-at")).not.toBe(String(a.session.id));
  }, 10_000);

  it("E2 「接管到 Meowo」和「进入现场」交给对话窗的东西一模一样", async () => {
    const ext = makeSession({ title: "外部的", project: "p" });
    ext.pending_review = "approval"; ext.pty_managed = false; ext.pid = 4321;
    sessions = [makeSession({ title: "甲", project: "p" }), ext];
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "get_live_sessions_page" ? { items: sessions, next_cursor: null } : cmd === "sidebar_git_badges" ? [] : cmd === "pending_interaction" ? { approval: null, question: null } : undefined));
    window.history.replaceState({}, "", "/?roster=1");
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("外部的"));
    fireEvent.click(await screen.findByRole("button", { name: "接管到 Meowo" }));
    const req = screen.getByTestId("chat").getAttribute("data-request");
    const called = invoke.mock.calls.map(([c]) => c).filter((c) => /takeover/.test(c));
    expect(req).toMatch(/takeover/);
  });
});
